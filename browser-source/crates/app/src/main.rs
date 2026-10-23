#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Pip — Main Application Entrypoint

mod accelerator;
mod adblock;
mod adblock_updates;
mod app_updates;
mod cert;
mod chrome;
#[cfg(target_os = "windows")]
mod doh_proxy;
mod downloads;
mod error_ui;
#[cfg(any(test, debug_assertions))]
#[cfg(debug_assertions)]
mod headless_ui_tests;
mod home_ui;
mod loading_ui;
mod passwords;
mod settings_ui;
mod sidebar_ui;
mod site_zoom;
mod sync;
mod sync_v2;
mod translate;
mod window;

use chrome::{create_bounds, normalize_url, CHROME_HEIGHT};
use evergreen_core::env::{detect_webview2_runtime, is_process_elevated};
use evergreen_core::ipc::{HostToUiMessage, UiToHostMessage};
use evergreen_core::tabs::{TabId, TabManager, TabState};
use evergreen_core::updater::run_local_command;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Fullscreen, Window, WindowId};
use wry::{WebView, WebViewBuilder, WebViewBuilderExtWindows};

#[derive(serde::Serialize)]
#[serde(tag = "event", content = "data")]
enum BorrowedHostToUiMessage<'a> {
    TabStateSync {
        tabs: &'a [TabState],
        active_tab_id: Option<TabId>,
    },
}

const NAV_WATCHER_SCRIPT: &str = r#"
(() => {
  // Native WebView2 SourceChanged/DocumentTitleChanged already report navigation.
  // Bottom Status Link Preview Bubble
  let previewTooltip = null;
  function ensureTooltip() {
    if (!previewTooltip) {
      previewTooltip = document.createElement('div');
      previewTooltip.id = '__evergreen_status_bubble';
      previewTooltip.style.cssText = 'position:fixed;bottom:8px;left:10px;max-width:550px;padding:4px 10px;background:#fff;border:1px solid #f1e2d5;border-radius:7px;color:#344054;font-size:11px;font-family:system-ui,-apple-system,sans-serif;z-index:2147483647;pointer-events:none;display:none;box-shadow:0 4px 14px rgba(98,57,28,0.14);white-space:nowrap;overflow:hidden;text-overflow:ellipsis;';
      document.documentElement.appendChild(previewTooltip);
    }
    return previewTooltip;
  }

  document.addEventListener('mouseover', (e) => {
    if(window.__pipLinkPreviewEnabled===false)return;
    const a = e.target.closest('a');
    if (a && a.href) {
      const tip = ensureTooltip();
      tip.textContent = a.href;
      tip.style.display = 'block';
    }
  });

  document.addEventListener('mouseout', (e) => {
    if(window.__pipLinkPreviewEnabled===false)return;
    const a = e.target.closest('a');
    if (a) {
      const tip = ensureTooltip();
      tip.style.display = 'none';
    }
  });
})();
"#;

const SESSION_AUTOSAVE_INTERVAL: Duration = Duration::from_secs(15);
const AUTO_DISCARD_IDLE_SECS: u64 = 30 * 60;
const LOW_MEMORY_AVAILABLE_BYTES: u64 = 512 * 1024 * 1024;

const DISCARD_SAFETY_SCRIPT: &str = r#"
(() => {
  const protectedFrames = window === window.top ? new Map() : null;
  let hasBeforeUnload = false;
  const activeTracks = new Set();
  const pruneStoppedTracks = () => {
    for (const track of activeTracks) {
      if (track.readyState !== 'live') activeTracks.delete(track);
    }
  };
  const hasDirtyPageState = () => {
    const dirtyInput = [...document.querySelectorAll('input, textarea')]
      .some(el => !el.disabled && el.value !== el.defaultValue);
    const dirtyChoice = [...document.querySelectorAll('input[type="checkbox"], input[type="radio"]')]
      .some(el => !el.disabled && el.checked !== el.defaultChecked);
    const selectedFiles = [...document.querySelectorAll('input[type="file"]')]
      .some(el => !el.disabled && el.files?.length > 0);
    const dirtySelect = [...document.querySelectorAll('select')]
      .some(el => [...el.options].some(option => option.selected !== option.defaultSelected));
    const editableContent = [...document.querySelectorAll('[contenteditable="true"], [contenteditable=""]')]
      .some(el => el.textContent.trim().length > 0);
    const playingMedia = [...document.querySelectorAll('video, audio')]
      .some(media => !media.paused && !media.ended);
    return dirtyInput || dirtyChoice || selectedFiles || dirtySelect || editableContent || playingMedia;
  };
  const hasProtectedState = () => {
    pruneStoppedTracks();
    return hasBeforeUnload
      || typeof window.onbeforeunload === 'function'
      || hasDirtyPageState()
      || activeTracks.size > 0;
  };
  const reportFrameProtection = () => {
    if (window !== window.top) window.top.postMessage({ pipDiscardProtection: hasProtectedState() }, '*');
  };
  if (window === window.top) {
    addEventListener('message', event => {
      if (event.source && event.source !== window && typeof event.data?.pipDiscardProtection === 'boolean') {
        if (event.data.pipDiscardProtection) protectedFrames.set(event.source, true);
        else protectedFrames.delete(event.source);
      }
    });
  } else {
    addEventListener('DOMContentLoaded', reportFrameProtection, { once: true });
    for (const event of ['input', 'change', 'reset', 'submit', 'playing', 'pause', 'ended']) {
      document.addEventListener(event, reportFrameProtection, true);
    }
  }
  const add = window.addEventListener;
  window.addEventListener = function(type, ...args) {
    if (type === 'beforeunload') {
      hasBeforeUnload = true;
      reportFrameProtection();
    }
    return add.call(this, type, ...args);
  };
  const mediaDevices = navigator.mediaDevices;
  if (mediaDevices?.getUserMedia) {
    const getUserMedia = mediaDevices.getUserMedia.bind(mediaDevices);
    mediaDevices.getUserMedia = async (...args) => {
      const stream = await getUserMedia(...args);
      for (const track of stream.getTracks()) {
        if (track.readyState !== 'live') continue;
        activeTracks.add(track);
        const releaseTrack = () => {
          activeTracks.delete(track);
          reportFrameProtection();
        };
        track.addEventListener('ended', releaseTrack);
        // MediaStreamTrack.stop() changes readyState but does not dispatch `ended`.
        // Wrap it so iframe protection is cleared immediately when a page releases capture.
        try {
          const stop = track.stop.bind(track);
          Object.defineProperty(track, 'stop', {
            configurable: true,
            value: (...stopArgs) => {
              const result = stop(...stopArgs);
              releaseTrack();
              return result;
            },
          });
        } catch (_) {}
      }
      reportFrameProtection();
      return stream;
    };
  }
  window.__pipCanDiscard = () => {
    const childSafe = window !== window.top || [...protectedFrames.values()].every(value => !value);
    return !hasProtectedState() && childSafe;
  };
})();
"#;

fn persist_session_snapshot(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<bool> {
    if std::fs::read(path).is_ok_and(|current| current == bytes) {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let staging = path.with_extension("json.tmp");
    std::fs::write(&staging, bytes)?;
    std::fs::rename(staging, path)?;
    Ok(true)
}

fn update_session_slot<T>(slots: &mut Vec<Vec<T>>, slot: usize, value: Vec<T>) {
    if slots.len() <= slot {
        slots.resize_with(slot + 1, Vec::new);
    }
    slots[slot] = value;
}

#[cfg(target_os = "windows")]
fn available_physical_memory_bytes() -> Option<u64> {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

    let mut status = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    unsafe { GlobalMemoryStatusEx(&mut status).ok()? };
    Some(status.ullAvailPhys)
}

#[cfg(not(target_os = "windows"))]
fn available_physical_memory_bytes() -> Option<u64> {
    None
}

fn eligible_for_memory_discard(tab: &TabState, now_secs: u64) -> bool {
    tab.status == evergreen_core::tabs::TabStatus::Suspended
        && !tab.keep_awake
        && !tab.pinned
        && !tab.is_loading
        && !tab.is_audio_playing
        && !tab.memory_discard_protected
        && (tab.url.starts_with("https://") || tab.url.starts_with("http://"))
        && now_secs.saturating_sub(tab.last_active_timestamp_secs) >= AUTO_DISCARD_IDLE_SECS
}

fn eligible_for_memory_suspend(tab: &TabState, now_secs: u64, idle_after: u64) -> bool {
    tab.status != evergreen_core::tabs::TabStatus::Suspended
        && !tab.keep_awake
        && !tab.pinned
        && !tab.is_loading
        && !tab.is_audio_playing
        && !tab.memory_discard_protected
        && (tab.url.starts_with("https://") || tab.url.starts_with("http://"))
        && now_secs.saturating_sub(tab.last_active_timestamp_secs) >= idle_after
}

fn close_tab_for_window(
    tab_manager: &mut TabManager,
    target_id: TabId,
    private: bool,
) -> Option<TabId> {
    let next_active = tab_manager.close_tab(target_id);
    if private {
        tab_manager.clear_closed_history();
    }
    next_active
}

fn should_confirm_close_window(warn_on_close_tabs: bool, tab_count: usize) -> bool {
    warn_on_close_tabs && tab_count > 1
}

fn confirm_close_window_tabs(warn_on_close_tabs: bool, tab_count: usize) -> bool {
    if !should_confirm_close_window(warn_on_close_tabs, tab_count) {
        return true;
    }
    #[cfg(target_os = "windows")]
    {
        use windows::core::w;
        use windows::Win32::UI::WindowsAndMessaging::{
            MessageBoxW, IDYES, MB_ICONWARNING, MB_YESNO,
        };
        unsafe {
            MessageBoxW(
                None,
                w!("Cửa sổ này đang có nhiều tab. Bạn có muốn đóng cửa sổ không?"),
                w!("Đóng nhiều tab — Pip"),
                MB_YESNO | MB_ICONWARNING,
            ) == IDYES
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        true
    }
}

fn pop_closed_tab_for_window(tab_manager: &mut TabManager, private: bool) -> Option<String> {
    if private {
        None
    } else {
        tab_manager.pop_last_closed()
    }
}

fn parse_memory_safety_result(result: &str) -> Option<(bool, String)> {
    let parsed = serde_json::from_str::<serde_json::Value>(result).ok()?;
    let value = if parsed.is_object() {
        parsed
    } else {
        serde_json::from_str::<serde_json::Value>(parsed.as_str()?).ok()?
    };
    Some((
        value["safe"].as_bool()?,
        value["href"].as_str()?.to_string(),
    ))
}

#[cfg(target_os = "windows")]
fn webview_container_hwnd(webview: &WebView) -> Option<windows::Win32::Foundation::HWND> {
    use wry::WebViewExtWindows;
    let mut hwnd = windows::Win32::Foundation::HWND::default();
    unsafe { webview.controller().ParentWindow(&mut hwnd).ok()? };
    (!hwnd.0.is_null()).then_some(hwnd)
}

#[cfg(target_os = "windows")]
fn raise_webview(webview: &WebView) {
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, HWND_TOP, SWP_ASYNCWINDOWPOS, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER,
        SWP_NOSIZE,
    };
    if let Some(hwnd) = webview_container_hwnd(webview) {
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOP),
                0,
                0,
                0,
                0,
                SWP_ASYNCWINDOWPOS | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOOWNERZORDER | SWP_NOSIZE,
            );
        }
    }
}

#[cfg(target_os = "windows")]
#[allow(dead_code)] // Native z-order diagnostics are exercised by the debug-only hidden UI test.
fn webview_is_top(webview: &WebView) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{GetWindow, GW_HWNDPREV};
    webview_container_hwnd(webview)
        .is_some_and(|hwnd| unsafe { GetWindow(hwnd, GW_HWNDPREV).is_err() })
}

#[derive(Debug)]
pub(crate) enum BrowserEvent {
    Ipc(WindowId, UiToHostMessage),
    PopupTabRequested {
        window_id: WindowId,
        url: String,
        opener_url: String,
    },
    TabTitleChanged(WindowId, TabId, String),
    TabNavigated(WindowId, TabId, String),
    TabAudioChanged(WindowId, TabId, bool),
    HistoryChanged(WindowId, TabId, bool, bool),
    CommandDone(String, bool, String),
    BrowsingDataClearDone(WindowId, BrowsingDataClearKind, bool),
    GoogleDnsCheckDone(WindowId, Result<(), String>),
    GoogleDnsHealthCheckDone(Result<(), String>),
    GoogleDnsResolverReport(doh_proxy::DnsResolverReport),
    AppUpdateCheckDone(WindowId, Result<app_updates::AppUpdateInfo, String>),
    AdblockFilterUpdateDone(WindowId, Result<adblock_updates::UpdateSummary, String>),
    TabSuspendResult(WindowId, TabId, bool),
    TabSuspendSafetyResult(WindowId, TabId, bool, String),
    TabDiscardSafetyResult(WindowId, TabId, bool, String),
    SyncBookmarksDone(
        WindowId,
        String,
        bool,
        Box<Result<sync::SyncOutcome, String>>,
    ),
    SyncDisabled(WindowId, String, String, Result<(), String>),
    Shortcut(WindowId, String),
    DownloadRequested(WindowId, u64),
    DownloadProgress {
        window_id: WindowId,
        download_id: u64,
        filename: String,
        path: String,
        received_bytes: i64,
        total_bytes: i64,
        state: String,
    },
    PasswordFillDone(WindowId, bool),
    BrowserExtensionsLoaded(WindowId, Result<Vec<BrowserExtensionInfo>, String>),
    BrowserExtensionOperationDone(WindowId, Result<(), String>),
    PermissionPrompt {
        window_id: WindowId,
        permission_id: u64,
        origin: String,
        permission_kind: String,
    },
    SitePermissionsLoaded(WindowId, Result<Vec<SitePermission>, String>),
    SitePermissionReset(WindowId, bool),
    TabCrashed(WindowId, TabId, i32),
    ServerCertificateError {
        window_id: WindowId,
        tab_id: TabId,
        request_uri: String,
        error_status: i32,
    },
}

#[derive(Debug, Clone, Copy)]
enum BrowsingDataClearKind {
    Cache,
    Passwords,
    SiteData,
    Selected {
        clear_pip_history: bool,
        clear_pip_download_history: bool,
        clear_pip_passwords: bool,
    },
}

#[derive(Debug, Clone, serde::Serialize)]
struct SitePermission {
    origin: String,
    kind_id: i32,
    kind_name: &'static str,
    state_name: &'static str,
}

#[derive(Debug, Clone, serde::Serialize)]
struct BrowserExtensionInfo {
    id: String,
    name: String,
    enabled: bool,
}

fn site_permission_kind_name(kind: i32) -> &'static str {
    match kind {
        1 => "Micrô",
        2 => "Camera",
        3 => "Vị trí",
        4 => "Thông báo",
        5 => "Cảm biến",
        6 => "Đọc bảng nhớ tạm",
        7 => "Nhiều lượt tải tự động",
        8 => "Đọc và ghi tệp",
        9 => "Tự phát media",
        10 => "Phông chữ cục bộ",
        11 => "MIDI hệ thống",
        12 => "Quản lý cửa sổ",
        _ => "Quyền khác",
    }
}

fn site_permission_state_name(state: i32) -> &'static str {
    match state {
        1 => "Được phép",
        2 => "Bị chặn",
        _ => "Mặc định",
    }
}

fn valid_permission_origin(origin: &str) -> bool {
    if origin.len() > 2048 || origin.chars().any(char::is_control) {
        return false;
    }
    let Some((scheme, authority)) = origin.split_once("://") else {
        return false;
    };
    matches!(scheme, "http" | "https")
        && !authority.is_empty()
        && !authority.contains('/')
        && !authority.contains('?')
        && !authority.contains('#')
        && !authority.contains('@')
        && !authority.chars().any(char::is_whitespace)
}

struct WindowContext {
    downloads: HashMap<u64, serde_json::Value>,
    proxy: EventLoopProxy<BrowserEvent>,
    profile_id: String,
    session_slot: usize,
    dns_proxy_port: Option<u16>,
    dns_proxy_required: bool,
    private: bool,
    id: WindowId,
    window: Arc<Window>,
    tab_manager: TabManager,
    chrome_webview: Option<WebView>,
    sidebar_webview: Option<WebView>,
    is_sidebar_open: bool,
    sidebar_mode: String,
    tabs: HashMap<TabId, WebView>,
    tab_persisted: HashMap<TabId, bool>,
    site_zoom: site_zoom::SiteZoomStore,
    window_width: f64,
    window_height: f64,
    is_fullscreen: bool,
    find_bar_open: bool,
    permission_bar_open: bool,
    profile_menu_open: bool,
}

#[derive(Debug, PartialEq)]
struct WindowLayout {
    chrome_height: f64,
    content_width: f64,
    content_height: f64,
    sidebar_visible: bool,
}

fn calculate_window_layout(
    width: f64,
    height: f64,
    chrome_height: f64,
    sidebar_open: bool,
    fullscreen: bool,
) -> WindowLayout {
    let chrome_height = if fullscreen { 0.0 } else { chrome_height };
    let sidebar_visible = sidebar_open && !fullscreen;
    let content_width = if sidebar_visible {
        (width - 320.0).max(1.0)
    } else {
        width
    };
    WindowLayout {
        chrome_height,
        content_width,
        content_height: (height - chrome_height).max(1.0),
        sidebar_visible,
    }
}

impl Drop for WindowContext {
    fn drop(&mut self) {
        accelerator::cancel_permissions(self.id, None, None);
        downloads::close_window(self.id);
    }
}

impl WindowContext {
    pub fn toggle_fullscreen(&mut self) {
        self.is_fullscreen = !self.is_fullscreen;
        self.window.set_fullscreen(if self.is_fullscreen {
            Some(Fullscreen::Borderless(None))
        } else {
            None
        });
        self.update_layout();
    }

    fn handle_escape(&mut self) {
        if self.is_fullscreen {
            self.toggle_fullscreen();
        } else if self.is_sidebar_open {
            self.close_sidebar();
            if let Some(chrome) = &self.chrome_webview {
                let _ = chrome.evaluate_script(
                    "if (window.__syncSidebarState) window.__syncSidebarState(null);",
                );
            }
        } else if let Some(active) = self.tab_manager.active_tab() {
            if let Some(view) = self.tabs.get(&active.id) {
                let _ = view.evaluate_script("window.stop()");
            }
        }
    }

    pub fn current_chrome_height(&self) -> f64 {
        CHROME_HEIGHT
            + if self.find_bar_open { 38.0 } else { 0.0 }
            + if self.permission_bar_open { 38.0 } else { 0.0 }
    }

    pub fn update_layout(&self) {
        let layout = calculate_window_layout(
            self.window_width,
            self.window_height,
            self.current_chrome_height(),
            self.is_sidebar_open,
            self.is_fullscreen,
        );
        let chrome_h = layout.chrome_height;
        if let Some(chrome) = &self.chrome_webview {
            let chrome_height = if self.profile_menu_open && !self.is_fullscreen {
                chrome_h.max(440.0)
            } else {
                chrome_h
            };
            let chrome_bounds = create_bounds(0.0, 0.0, self.window_width, chrome_height);
            let _ = chrome.set_visible(!self.is_fullscreen);
            let _ = chrome.set_bounds(chrome_bounds);
            if self.profile_menu_open && !self.is_fullscreen {
                raise_webview(chrome);
            }
        }

        let sidebar_visible = layout.sidebar_visible;
        let content_width = layout.content_width;

        let content_bounds = create_bounds(0.0, chrome_h, content_width, layout.content_height);
        // Hidden tabs do not paint. Resize only the active WebView; a hidden tab
        // receives the current bounds when `handle_switch_tab` makes it active
        // and calls `update_layout` below. This avoids one COM call per open tab
        // during every resize/sidebar change without changing tab state.
        if let Some(active) = self
            .tab_manager
            .active_tab_id()
            .and_then(|id| self.tabs.get(&id))
        {
            let _ = active.set_bounds(content_bounds);
        }
        if !self.profile_menu_open {
            if let Some(active) = self
                .tab_manager
                .active_tab_id()
                .and_then(|id| self.tabs.get(&id))
            {
                raise_webview(active);
            }
        }

        if let Some(sidebar) = &self.sidebar_webview {
            let _ = sidebar.set_visible(sidebar_visible);
        }
        if sidebar_visible {
            if let Some(sidebar) = &self.sidebar_webview {
                let sidebar_bounds =
                    create_bounds(content_width, chrome_h, 320.0, layout.content_height);
                let _ = sidebar.set_bounds(sidebar_bounds);
            }
        }
    }

    pub fn handle_focus_omnibox(&self) {
        if let Some(chrome) = &self.chrome_webview {
            #[cfg(target_os = "windows")]
            {
                use webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC;
                use wry::WebViewExtWindows;
                let _ = unsafe {
                    chrome
                        .controller()
                        .MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC)
                };
            }
            let _ =
                chrome.evaluate_script("if (window.__focusOmnibox) { window.__focusOmnibox(); }");
        }
    }

    pub fn sync_ui_state(&self) {
        if let Some(chrome) = &self.chrome_webview {
            let sync_msg = BorrowedHostToUiMessage::TabStateSync {
                tabs: self.tab_manager.tabs(),
                active_tab_id: self.tab_manager.active_tab().map(|t| t.id),
            };
            if let Ok(json_str) = serde_json::to_string(&sync_msg) {
                let script = format!(
                    "if (window.__shellUpdate) {{ window.__shellUpdate({}); }}",
                    json_str
                );
                let _ = chrome.evaluate_script(&script);
            }
        }
    }

    pub fn sync_zoom(&self, factor: f64) {
        if let Some(chrome) = &self.chrome_webview {
            let _ = chrome.evaluate_script(&format!(
                "if (window.__syncZoom) {{ window.__syncZoom({}); }}",
                factor
            ));
        }
        if let Some(sidebar) = &self.sidebar_webview {
            let _ = sidebar.evaluate_script(&format!(
                "if (window.__syncZoom) {{ window.__syncZoom({}); }}",
                factor
            ));
        }
    }

    pub fn get_active_zoom(&self) -> f64 {
        #[cfg(target_os = "windows")]
        {
            if let Some(active) = self.tab_manager.active_tab() {
                if let Some(wv) = self.tabs.get(&active.id) {
                    use wry::WebViewExtWindows;
                    unsafe {
                        let controller = wv.controller();
                        let mut factor = 1.0;
                        if controller.ZoomFactor(&mut factor).is_ok() {
                            return factor;
                        }
                    }
                }
            }
        }
        1.0
    }

    pub fn set_active_zoom(&mut self, factor: f64) {
        let clamped = site_zoom::clamp_zoom(factor);
        if let Some(active) = self.tab_manager.active_tab() {
            if let Some(origin) = site_zoom::origin_for_url(&active.url) {
                self.site_zoom.set(origin, clamped);
                let _ = self
                    .site_zoom
                    .save(&site_zoom_path(&self.profile_id), self.private);
            }
        }
        #[cfg(target_os = "windows")]
        {
            if let Some(active) = self.tab_manager.active_tab() {
                if let Some(wv) = self.tabs.get(&active.id) {
                    use wry::WebViewExtWindows;
                    unsafe {
                        let controller = wv.controller();
                        let _ = controller.SetZoomFactor(clamped);
                    }
                }
            }
        }
        self.sync_zoom(clamped);
    }

    pub fn apply_tab_zoom(&self, tab_id: TabId, factor: f64) {
        let clamped = site_zoom::clamp_zoom(factor);
        #[cfg(target_os = "windows")]
        if let Some(wv) = self.tabs.get(&tab_id) {
            use wry::WebViewExtWindows;
            unsafe {
                let _ = wv.controller().SetZoomFactor(clamped);
            }
        }
        if self.tab_manager.active_tab_id() == Some(tab_id) {
            self.sync_zoom(clamped);
        }
    }

    pub fn reset_active_zoom(&mut self, default_zoom: f64) {
        if let Some(active) = self.tab_manager.active_tab() {
            if let Some(origin) = site_zoom::origin_for_url(&active.url) {
                self.site_zoom.remove(&origin);
                let _ = self
                    .site_zoom
                    .save(&site_zoom_path(&self.profile_id), self.private);
            }
            self.apply_tab_zoom(active.id, default_zoom);
        } else {
            self.sync_zoom(default_zoom);
        }
    }

    fn zoom_for_url(&self, url: &str, default_zoom: f64) -> f64 {
        site_zoom::origin_for_url(url)
            .and_then(|origin| self.site_zoom.get(&origin))
            .unwrap_or(default_zoom)
    }

    pub fn handle_find_in_page(&self, query: &str, forward: bool) {
        if let Some(active) = self.tab_manager.active_tab() {
            if let Some(wv) = self.tabs.get(&active.id) {
                let query_json = serde_json::to_string(query).unwrap();
                let script = format!(
                    r#"(()=>{{
                    const q={query_json};
                    if(!q)return {{current:0,total:0}};
                    const found=window.find(q,false,!{forward},true,false,false,false);
                    const text=document.body?.innerText||'';
                    const lower=text.toLocaleLowerCase(),needle=q.toLocaleLowerCase();
                    let count=0,offset=0;
                    while((offset=lower.indexOf(needle,offset))!==-1){{count++;offset+=needle.length;}}
                    return {{current:found?1:0,total:count}};
                }})()"#
                );
                let proxy = self.proxy.clone();
                let window_id = self.id;
                let _ = wv.evaluate_script_with_callback(&script, move |reply| {
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&reply) {
                        {
                            let current =
                                value["current"].as_u64().unwrap_or(0).min(1_000_000) as usize;
                            let total =
                                value["total"].as_u64().unwrap_or(0).min(1_000_000) as usize;
                            let _ = proxy.send_event(BrowserEvent::Ipc(
                                window_id,
                                UiToHostMessage::FindResult { current, total },
                            ));
                        }
                    }
                });
            }
        }
    }

    pub fn handle_close_find_in_page(&self) {
        if let Some(active) = self.tab_manager.active_tab() {
            if let Some(wv) = self.tabs.get(&active.id) {
                let _ = wv.evaluate_script(
                    "if (window.getSelection) { window.getSelection().removeAllRanges(); }",
                );
            }
        }
    }

    pub fn ensure_sidebar_webview(
        &mut self,
        proxy: EventLoopProxy<BrowserEvent>,
    ) -> Result<(), wry::Error> {
        if self.sidebar_webview.is_none() {
            let window = self.window.clone();
            let chrome_h = self.current_chrome_height();
            let content_width = (self.window_width - 320.0).max(1.0);
            let sidebar_bounds = create_bounds(
                content_width,
                chrome_h,
                320.0,
                (self.window_height - chrome_h).max(1.0),
            );
            let proxy_ipc = proxy.clone();
            let win_id = self.id;
            let builder = WebViewBuilder::new()
                .with_additional_browser_args(webview_browser_args(
                    self.dns_proxy_required,
                    self.dns_proxy_port,
                ))
                .with_bounds(sidebar_bounds)
                .with_browser_extensions_enabled(true);
            let wv = builder
                .with_html(sidebar_ui::SIDEBAR_HTML.as_str())
                .with_navigation_handler(|url| {
                    url == "about:blank" || url.starts_with("data:text/html")
                })
                .with_transparent(true)
                .with_background_color((30, 30, 38, 255))
                .with_devtools(true)
                .with_ipc_handler(move |req: wry::http::Request<String>| {
                    if let Ok(msg) = serde_json::from_str::<UiToHostMessage>(req.body()) {
                        let _ = proxy_ipc.send_event(BrowserEvent::Ipc(win_id, msg));
                    }
                })
                .build_as_child(window.as_ref())?;

            accelerator::attach_accelerator_keys(&wv, win_id, proxy);
            self.sidebar_webview = Some(wv);
        }
        Ok(())
    }

    pub fn update_sidebar_sync(&self, cert_cache: &cert::CertificateCache) {
        if !self.is_sidebar_open {
            return;
        }
        if let Some(sidebar) = &self.sidebar_webview {
            let snapshot =
                serde_json::to_string(&self.downloads.values().collect::<Vec<_>>()).unwrap();
            let private = self.private;
            let recently_closed = if private {
                Vec::<String>::new()
            } else {
                self.tab_manager.recently_closed_newest_first()
            };
            let recently_closed_json = serde_json::to_string(&recently_closed)
                .unwrap_or_else(|_| "[]".to_string())
                .replace('<', "\\u003c")
                .replace('\u{2028}', "\\u2028")
                .replace('\u{2029}', "\\u2029");
            let active = self.tab_manager.active_tab();
            let keep_awake = active.is_some_and(|tab| tab.keep_awake);
            let url = active.map(|t| t.url.clone()).unwrap_or_default();
            let sec_info = cert_cache.query_or_default(&url);

            let sync_msg = HostToUiMessage::SidebarStateSync {
                open: self.is_sidebar_open,
                mode: self.sidebar_mode.clone(),
                security_info: Some(Box::new(sec_info)),
            };

            if let Ok(json_str) = serde_json::to_string(&sync_msg) {
                let script = format!(
                    "if(window.__syncDownloads)window.__syncDownloads({snapshot});if(window.__renderPrivacy)window.__renderPrivacy({private});if(window.__setKeepAwake)window.__setKeepAwake({keep_awake});if(window.__setRecentlyClosed)window.__setRecentlyClosed({recently_closed_json});if(window.__sidebarSync)window.__sidebarSync({json_str});"
                );
                let _ = sidebar.evaluate_script(&script);
            }
        }
    }

    pub fn open_sidebar(
        &mut self,
        mode: &str,
        cert_cache: &cert::CertificateCache,
        proxy: EventLoopProxy<BrowserEvent>,
    ) {
        self.is_sidebar_open = true;
        self.sidebar_mode = mode.to_string();

        let _ = self.ensure_sidebar_webview(proxy);
        self.update_layout();
        if let Some(sidebar) = &self.sidebar_webview {
            let _ = sidebar.set_visible(true);
        }
        self.update_sidebar_sync(cert_cache);
    }

    pub fn close_sidebar(&mut self) {
        self.is_sidebar_open = false;
        if let Some(sidebar) = &self.sidebar_webview {
            let _ = sidebar.set_visible(false);
        }
        self.update_layout();
    }

    pub fn toggle_sidebar(
        &mut self,
        mode: &str,
        cert_cache: &cert::CertificateCache,
        proxy: EventLoopProxy<BrowserEvent>,
    ) {
        if self.is_sidebar_open && self.sidebar_mode == mode {
            self.close_sidebar();
        } else {
            self.open_sidebar(mode, cert_cache, proxy);
        }
    }

    pub fn switch_to_tab_index(&mut self, idx: usize, cert_cache: &cert::CertificateCache) {
        let tabs = self.tab_manager.tabs();
        if idx < tabs.len() {
            let id = tabs[idx].id;
            self.handle_switch_tab(id, cert_cache);
        }
    }

    pub fn wake_active(&self) {
        if let Some(active) = self.tab_manager.active_tab() {
            if let Some(wv) = self.tabs.get(&active.id) {
                accelerator::wake_webview(wv);
            }
        }
        if let Some(chrome) = &self.chrome_webview {
            accelerator::wake_webview(chrome);
        }
        if self.is_sidebar_open {
            if let Some(sidebar) = &self.sidebar_webview {
                accelerator::wake_webview(sidebar);
            }
        }
    }

    pub fn handle_switch_tab(&mut self, target_id: TabId, cert_cache: &cert::CertificateCache) {
        if !self.tabs.contains_key(&target_id)
            && self.tab_manager.get_tab_by_id(target_id).is_some()
        {
            let _ = self.proxy.send_event(BrowserEvent::Ipc(
                self.id,
                UiToHostMessage::SwitchTab { id: target_id },
            ));
            return;
        }
        let previous_id = self.tab_manager.active_tab_id();
        if self
            .tab_manager
            .switch_tab(target_id, BrowserApp::now_secs())
        {
            if previous_id != Some(target_id) {
                if let Some(wv) = previous_id.and_then(|id| self.tabs.get(&id)) {
                    accelerator::hide_webview(wv);
                }
            }
            if let Some(wv) = self.tabs.get(&target_id) {
                let _ = wv.set_visible(true);
                accelerator::wake_webview(wv);
                accelerator::focus_webview(wv);
            }
            self.update_layout();
            self.sync_ui_state();
            self.update_sidebar_sync(cert_cache);
            let active_zoom = self.get_active_zoom();
            self.apply_tab_zoom(target_id, active_zoom);
        }
    }
}

pub(crate) fn extract_host(url: &str) -> Option<String> {
    let uri = url.parse::<wry::http::Uri>().ok()?;
    matches!(uri.scheme_str(), Some("https" | "http")).then_some(())?;
    Some(uri.host()?.to_ascii_lowercase())
}

fn favicon_url(url: &str) -> Option<String> {
    let uri = url.parse::<wry::http::Uri>().ok()?;
    let scheme = uri.scheme_str()?;
    matches!(scheme, "https" | "http").then_some(())?;
    Some(format!("{scheme}://{}/favicon.ico", uri.authority()?))
}

fn get_data_directory() -> std::path::PathBuf {
    let exe_dir = std::env::current_exe()
        .map(|p| p.parent().unwrap_or(&p).to_path_buf())
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    let args: Vec<String> = std::env::args().collect();
    evergreen_core::env::resolve_data_directory(&exe_dir, &args)
}

fn download_history_path(profile_id: &str) -> std::path::PathBuf {
    get_data_directory()
        .join("profiles")
        .join(profile_id)
        .join("downloads.json")
}

fn site_zoom_path(profile_id: &str) -> std::path::PathBuf {
    get_data_directory()
        .join("profiles")
        .join(profile_id)
        .join("site-zoom.json")
}

fn load_site_zoom(profile_id: &str, private: bool) -> site_zoom::SiteZoomStore {
    site_zoom::SiteZoomStore::load(&site_zoom_path(profile_id), private)
}

fn load_download_history(profile_id: &str) -> HashMap<u64, serde_json::Value> {
    let Ok(bytes) = std::fs::read(download_history_path(profile_id)) else {
        return HashMap::new();
    };
    if bytes.len() > 2 * 1024 * 1024 {
        return HashMap::new();
    }
    let Ok(entries) = serde_json::from_slice::<Vec<serde_json::Value>>(&bytes) else {
        return HashMap::new();
    };
    entries
        .into_iter()
        .filter_map(|mut item| {
            let id = item["download_id"].as_u64()?;
            if !matches!(item["state"].as_str(), Some("Completed" | "Interrupted"))
                || item["filename"]
                    .as_str()
                    .is_none_or(|name| name.len() > 1024)
                || item["path"].as_str().is_none_or(|path| path.len() > 32_768)
            {
                return None;
            }
            item["received_bytes"] =
                serde_json::json!(item["received_bytes"].as_i64().unwrap_or(0).max(0));
            item["total_bytes"] =
                serde_json::json!(item["total_bytes"].as_i64().unwrap_or(0).max(0));
            Some((id, item))
        })
        .collect()
}

fn trim_download_history(downloads: &mut HashMap<u64, serde_json::Value>) {
    let mut completed = downloads
        .iter()
        .filter(|(_, item)| item["state"] != "InProgress")
        .map(|(id, _)| *id)
        .collect::<Vec<_>>();
    completed.sort_unstable();
    for id in completed
        .into_iter()
        .take(downloads.len().saturating_sub(500))
    {
        downloads.remove(&id);
    }
}

fn get_settings_path() -> std::path::PathBuf {
    get_data_directory().join("settings.json")
}

fn bookmark_sync_state_changed(
    before: &evergreen_core::library::Library,
    after: &evergreen_core::library::Library,
    history_enabled: bool,
) -> bool {
    before.bookmarks != after.bookmarks
        || before.bookmark_updates != after.bookmark_updates
        || before.removed_bookmarks != after.removed_bookmarks
        || before.bookmark_tombstones != after.bookmark_tombstones
        || before.bookmark_folders != after.bookmark_folders
        || before.bookmark_folder_assignments != after.bookmark_folder_assignments
        || before.bookmark_folder_tombstones != after.bookmark_folder_tombstones
        || (history_enabled
            && (before.history != after.history
                || before.history_visits != after.history_visits
                || before.removed_history != after.removed_history))
}

fn local_bookmark_updates(
    category: &sync_v2::BookmarkCategory,
) -> Vec<evergreen_core::library::BookmarkUpdate> {
    category
        .items
        .iter()
        .map(|item| evergreen_core::library::BookmarkUpdate {
            url: item.page.url.clone(),
            updated_at_ms: i64::try_from(item.updated_at_ms).unwrap_or(i64::MAX),
        })
        .collect()
}

fn local_bookmark_tombstones(
    category: &sync_v2::BookmarkCategory,
) -> Vec<evergreen_core::library::BookmarkTombstone> {
    category
        .tombstones
        .iter()
        .map(|item| evergreen_core::library::BookmarkTombstone {
            url: item.url.clone(),
            deleted_at_ms: i64::try_from(item.deleted_at_ms).unwrap_or(i64::MAX),
        })
        .collect()
}

fn apply_syncable_settings_to_profile(
    settings: &mut evergreen_core::settings::Settings,
    profile_id: &str,
    synced: &sync_v2::SyncableSettings,
) -> bool {
    let before = settings.clone();
    let mut effective = settings.for_profile(profile_id);
    sync::apply_syncable_settings(&mut effective, synced);
    if let Some(profile) = settings
        .profiles
        .iter_mut()
        .find(|profile| profile.id == profile_id)
    {
        profile.sync_settings_values = Some(
            evergreen_core::settings::ProfileSyncSettings::from_global(&effective),
        );
        profile.sync_settings_fingerprint = Some(sync::syncable_settings_fingerprint(synced));
        profile.sync_settings_updated_at_ms = synced.updated_at_ms;
    } else {
        return false;
    }
    *settings != before
}

fn syncable_tab_url(url: &str) -> bool {
    if url.len() > sync_v2::MAX_URL_BYTES || url.chars().any(char::is_control) {
        return false;
    }
    let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    !authority.is_empty() && !authority.contains('@') && !authority.chars().any(char::is_whitespace)
}

fn dns_proxy_browser_args(enabled: bool, port: Option<u16>) -> String {
    // Wry 0.57 replaces its default flags when custom args are supplied. Keep the two
    // flags Wry uses to suppress Edge's selection mini-menu; recheck on Wry upgrades.
    let mut args = String::from("--disable-features=msWebOOUI,msPdfOOUI");
    if enabled {
        // URL requests use Pip's loopback proxy, while Chromium DNS prefetch can
        // resolve names outside the URL proxy. Fail those speculative lookups
        // locally; the proxy uses numeric loopback and resolves destinations via DoH.
        args.push_str(" --disable-quic");
        if let Some(port) = port {
            args.push_str(&format!(" --proxy-server=http://127.0.0.1:{port}"));
        }
        // If neither the DoH proxy nor its local rejection listener could bind,
        // still deny hostname lookups locally. Numeric IP URLs do not use DNS.
        args.push_str(
            " --host-resolver-rules=\"MAP * ~NOTFOUND , EXCLUDE localhost , EXCLUDE 127.0.0.1\"",
        );
    }
    args
}

fn webview_browser_args(enabled: bool, port: Option<u16>) -> String {
    let mut args = dns_proxy_browser_args(enabled, port);
    if std::env::args().any(|arg| arg == "--verify-native-ui") {
        // Wry creates a separate WebView2 environment per WebView. Keep environment options
        // identical for all views that share WebView2's user data folder.
        args.push_str(" --use-fake-device-for-media-stream");
    }
    args
}

fn startup_profiles_path() -> std::path::PathBuf {
    // This also stores the normal-startup window manifest; retain the legacy filename
    // so users with a pending explicit-restart list from an older build can recover it.
    get_data_directory().join("restart-profiles.json")
}

fn valid_startup_profiles(saved: &[String], known_profiles: &[String]) -> Vec<String> {
    let mut profiles = saved
        .iter()
        .filter(|id| {
            evergreen_core::settings::valid_profile_id(id)
                && known_profiles.iter().any(|known| known == *id)
        })
        .cloned()
        .collect::<Vec<_>>();
    if profiles.is_empty() {
        profiles.push("Default".into());
    }
    profiles
}

fn startup_profiles_for_windows(
    windows: impl IntoIterator<Item = (String, usize, bool)>,
) -> Vec<String> {
    let mut open_windows = windows
        .into_iter()
        .filter(|(_, _, private)| !private)
        .map(|(profile_id, slot, _)| (profile_id, slot))
        .collect::<Vec<_>>();
    open_windows.sort();
    open_windows
        .into_iter()
        .map(|(profile_id, _)| profile_id)
        .collect()
}

fn persist_startup_profiles(path: &std::path::Path, profiles: &[String]) -> std::io::Result<bool> {
    if profiles.is_empty() {
        return Ok(false);
    }
    let bytes = serde_json::to_vec(profiles).map_err(std::io::Error::other)?;
    persist_session_snapshot(path, &bytes)
}

fn load_session_windows(path: &std::path::Path) -> Vec<Vec<TabState>> {
    let Ok(bytes) = std::fs::read(path) else {
        return Vec::new();
    };
    serde_json::from_slice::<Vec<Vec<TabState>>>(&bytes).unwrap_or_else(|_| {
        serde_json::from_slice::<Vec<TabState>>(&bytes)
            .map(|tabs| vec![tabs])
            .unwrap_or_default()
    })
}

fn load_closed_histories(path: &std::path::Path) -> Vec<Vec<String>> {
    let Ok(bytes) = std::fs::read(path) else {
        return Vec::new();
    };
    serde_json::from_slice::<Vec<Vec<String>>>(&bytes).unwrap_or_else(|_| {
        serde_json::from_slice::<Vec<String>>(&bytes)
            .map(|urls| vec![urls])
            .unwrap_or_default()
    })
}

fn is_restorable_session_url(url: &str) -> bool {
    url.starts_with("https://")
        || url.starts_with("http://")
        || matches!(
            url,
            "evergreen://newtab"
                | "evergreen://newtab?history"
                | "evergreen://newtab?bookmarks"
                | "evergreen://settings"
                | "evergreen://settings?clear-data"
        )
}

fn sync_settings_patch_present(value: &serde_json::Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    object.contains_key("search_engine")
        || object.contains_key("default_zoom_level")
        || object
            .get("appearance")
            .and_then(|v| v.as_object())
            .is_some_and(|a| {
                [
                    "theme",
                    "density",
                    "default_zoom_level",
                    "show_zoom_badge",
                    "show_status_preview",
                ]
                .iter()
                .any(|key| a.contains_key(*key))
            })
        || object
            .get("tabs")
            .and_then(|v| v.as_object())
            .is_some_and(|t| {
                [
                    "enable_tab_reordering",
                    "enable_tab_tearoff",
                    "warn_on_close_tabs",
                ]
                .iter()
                .any(|key| t.contains_key(*key))
            })
}

const PROFILE_DOWNLOAD_SETTING_KEYS: [&str; 4] = [
    "ask_where_to_save",
    "askWhereToSave",
    "default_folder",
    "defaultFolder",
];

fn profile_download_patch_field<'a>(
    value: &'a serde_json::Value,
    snake_case: &str,
    camel_case: &str,
) -> Option<&'a serde_json::Value> {
    let object = value.as_object()?;
    object
        .get(camel_case)
        .or_else(|| object.get(snake_case))
        .or_else(|| {
            object
                .get("downloads")
                .and_then(serde_json::Value::as_object)
                .and_then(|downloads| {
                    downloads
                        .get(snake_case)
                        .or_else(|| downloads.get(camel_case))
                })
        })
}

fn profile_download_settings_patch_present(value: &serde_json::Value) -> bool {
    PROFILE_DOWNLOAD_SETTING_KEYS
        .iter()
        .any(|key| value.as_object().is_some_and(|object| object.contains_key(key)))
        || value
            .get("downloads")
            .and_then(serde_json::Value::as_object)
            .is_some_and(|downloads| {
                PROFILE_DOWNLOAD_SETTING_KEYS
                    .iter()
                    .any(|key| downloads.contains_key(key))
            })
}

/// Strip local-only download preferences before updating shared settings.
fn without_profile_download_settings_patch(value: &serde_json::Value) -> serde_json::Value {
    let mut clean = value.clone();
    if let Some(object) = clean.as_object_mut() {
        for key in PROFILE_DOWNLOAD_SETTING_KEYS {
            object.remove(key);
        }
        if let Some(downloads) = object
            .get_mut("downloads")
            .and_then(serde_json::Value::as_object_mut)
        {
            for key in PROFILE_DOWNLOAD_SETTING_KEYS {
                downloads.remove(key);
            }
        }
    }
    clean
}

/// Apply the download fields present in a SaveSettings patch to one profile only.
fn apply_profile_download_settings_patch(
    settings: &mut evergreen_core::settings::Settings,
    profile_id: &str,
    value: &serde_json::Value,
) -> bool {
    let ask_where_to_save =
        profile_download_patch_field(value, "ask_where_to_save", "askWhereToSave")
            .and_then(serde_json::Value::as_bool);
    let default_folder = profile_download_patch_field(value, "default_folder", "defaultFolder")
        .and_then(serde_json::Value::as_str)
        .map(std::path::PathBuf::from);
    if ask_where_to_save.is_none() && default_folder.is_none() {
        return false;
    }

    let Some(mut download_settings) = settings.profile_download_settings(profile_id) else {
        return false;
    };
    if let Some(ask_where_to_save) = ask_where_to_save {
        download_settings.ask_where_to_save = ask_where_to_save;
    }
    if let Some(default_folder) = default_folder {
        download_settings.default_folder = default_folder;
    }
    settings.set_profile_download_settings(profile_id, download_settings)
}

fn update_profile_download_folder(
    settings: &mut evergreen_core::settings::Settings,
    profile_id: &str,
    folder: std::path::PathBuf,
) -> bool {
    let Some(mut download_settings) = settings.profile_download_settings(profile_id) else {
        return false;
    };
    download_settings.default_folder = folder;
    settings.set_profile_download_settings(profile_id, download_settings)
}

/// Strip the nine profile-local Sync preferences before updating global settings.
fn without_sync_settings_patch(value: &serde_json::Value) -> serde_json::Value {
    let mut clean = value.clone();
    if let Some(object) = clean.as_object_mut() {
        object.remove("search_engine");
        object.remove("default_zoom_level");
        if let Some(appearance) = object.get_mut("appearance").and_then(|v| v.as_object_mut()) {
            for key in [
                "theme",
                "density",
                "default_zoom_level",
                "show_zoom_badge",
                "show_status_preview",
            ] {
                appearance.remove(key);
            }
        }
        if let Some(tabs) = object.get_mut("tabs").and_then(|v| v.as_object_mut()) {
            for key in [
                "enable_tab_reordering",
                "enable_tab_tearoff",
                "warn_on_close_tabs",
            ] {
                tabs.remove(key);
            }
        }
    }
    clean
}

fn choose_download_folder(window: &Window) -> windows::core::Result<std::path::PathBuf> {
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::Shell::{
        FileOpenDialog, IFileOpenDialog, FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, SIGDN_FILESYSPATH,
    };
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let owner = window.window_handle().ok().and_then(|h| match h.as_raw() {
        RawWindowHandle::Win32(h) => Some(windows::Win32::Foundation::HWND(h.hwnd.get() as _)),
        _ => None,
    });
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
        dialog.SetOptions(dialog.GetOptions()? | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM)?;
        dialog.SetTitle(windows::core::w!("Chọn thư mục tải xuống của Pip"))?;
        dialog.Show(owner)?;
        let path = dialog.GetResult()?.GetDisplayName(SIGDN_FILESYSPATH)?;
        Ok(webview2_com::take_pwstr(path).into())
    }
}

fn choose_bookmarks_import_file(window: &Window) -> Option<std::path::PathBuf> {
    use windows::core::PCWSTR;
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::Shell::{
        FileOpenDialog, IFileOpenDialog, FOS_FORCEFILESYSTEM, SIGDN_FILESYSPATH,
    };
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let owner = window
        .window_handle()
        .ok()
        .and_then(|handle| match handle.as_raw() {
            RawWindowHandle::Win32(handle) => {
                Some(windows::Win32::Foundation::HWND(handle.hwnd.get() as _))
            }
            _ => None,
        })
        .unwrap_or_default();
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        dialog
            .SetOptions(dialog.GetOptions().ok()? | FOS_FORCEFILESYSTEM)
            .ok()?;
        let title: Vec<u16> = "Nhập dấu trang HTML"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        dialog.SetTitle(PCWSTR(title.as_ptr())).ok()?;
        dialog.Show(Some(owner)).ok()?;
        let path = dialog
            .GetResult()
            .ok()?
            .GetDisplayName(SIGDN_FILESYSPATH)
            .ok()?;
        Some(webview2_com::take_pwstr(path).into())
    }
}

fn choose_browser_extension_folder(window: &Window) -> windows::core::Result<std::path::PathBuf> {
    use windows::core::PCWSTR;
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::Shell::{
        FileOpenDialog, IFileOpenDialog, FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, SIGDN_FILESYSPATH,
    };
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let owner = window
        .window_handle()
        .ok()
        .and_then(|handle| match handle.as_raw() {
            RawWindowHandle::Win32(handle) => {
                Some(windows::Win32::Foundation::HWND(handle.hwnd.get() as _))
            }
            _ => None,
        })
        .unwrap_or_default();
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
        dialog.SetOptions(dialog.GetOptions()? | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM)?;
        let title: Vec<u16> = "Chọn thư mục tiện ích đã giải nén"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        dialog.SetTitle(PCWSTR(title.as_ptr()))?;
        dialog.Show(Some(owner))?;
        let path = dialog.GetResult()?.GetDisplayName(SIGDN_FILESYSPATH)?;
        Ok(webview2_com::take_pwstr(path).into())
    }
}

fn validate_browser_extension_folder(path: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let path = path
        .canonicalize()
        .map_err(|_| "Không tìm thấy thư mục tiện ích đã chọn.".to_string())?;
    if !path.is_dir()
        || path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with('_'))
    {
        return Err("Hãy chọn thư mục tiện ích hợp lệ trên máy này.".into());
    }
    let manifest_path = path.join("manifest.json");
    // Do not follow a manifest symlink outside the user-selected extension folder.
    let metadata = std::fs::symlink_metadata(&manifest_path)
        .map_err(|_| "Thư mục không có manifest.json của tiện ích.".to_string())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 1024 * 1024 {
        return Err("manifest.json không hợp lệ hoặc vượt quá 1 MiB.".into());
    }
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(manifest_path)
            .map_err(|_| "Không đọc được manifest.json của tiện ích.".to_string())?,
    )
    .map_err(|_| "manifest.json không phải JSON hợp lệ.".to_string())?;
    let valid_text_field = |key: &str, max_chars: usize| {
        manifest[key].as_str().is_some_and(|value| {
            let value = value.trim();
            !value.is_empty() && value.chars().count() <= max_chars
        })
    };
    if !manifest.is_object()
        || !matches!(manifest["manifest_version"].as_u64(), Some(2 | 3))
        || !valid_text_field("name", 256)
        || !valid_text_field("version", 128)
    {
        return Err("Tiện ích cần manifest version 2 hoặc 3, tên và phiên bản hợp lệ.".into());
    }
    Ok(path)
}

struct BrowserApp {
    background_check: bool,
    internal_ipc_token: String,
    adblock: std::rc::Rc<std::cell::RefCell<adblock::Blocker>>,
    windows: HashMap<WindowId, WindowContext>,
    proxy: EventLoopProxy<BrowserEvent>,
    cert_cache: cert::CertificateCache,
    runtime_version: String,
    modifiers: ModifiersState,
    settings: evergreen_core::settings::Settings,
    /// Google DNS mode applied at process startup; saved changes require restart.
    google_dns_applied: bool,
    active_google_dns: bool,
    google_dns_start_failed: bool,
    google_dns_health_check_started: bool,
    google_dns_route_ok: Option<bool>,
    google_dns_fallback_used: bool,
    google_dns_resolution_failed: bool,
    doh_proxy: Option<doh_proxy::DohProxy>,
    failed_closed_dns_proxy: Option<doh_proxy::RejectingProxy>,
    restart_requested: bool,
    startup_profiles: Vec<String>,
    restoring_startup_windows: bool,
    next_tab_id: AtomicU64,
    allowed_cert_hosts: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    next_session_autosave: Instant,
    next_background_tab_check: Instant,
    suspending_tabs: HashSet<(WindowId, TabId)>,
    suspending_urls: HashMap<(WindowId, TabId), String>,
    sleep_checking_tabs: HashSet<(WindowId, TabId)>,
    discard_checking_tabs: HashSet<(WindowId, TabId)>,
    syncing_profiles: HashSet<String>,
    pending_sync_profiles: HashSet<String>,
    #[cfg(debug_assertions)]
    force_memory_pressure_for_tests: bool,
}

impl BrowserApp {
    fn dns_proxy_browser_port(&self) -> Option<u16> {
        self.doh_proxy
            .as_ref()
            .map(doh_proxy::DohProxy::port)
            .or_else(|| {
                self.failed_closed_dns_proxy
                    .as_ref()
                    .map(doh_proxy::RejectingProxy::port)
            })
    }

    fn sync_settings(&self) {
        for win in self.windows.values() {
            let effective = self.settings.for_profile(&win.profile_id);
            let json = serde_json::to_string(&effective).unwrap();
            let engine = serde_json::to_string(&effective.search_engine).unwrap();
            let preview = effective.features.enable_link_preview
                && effective.appearance.enable_link_preview
                && effective.appearance.show_status_preview;
            if let Some(chrome) = &win.chrome_webview {
                let _ = chrome.evaluate_script(&format!(
                    "if(window.__applySettings)window.__applySettings({json});if(window.__syncSearchEngine)window.__syncSearchEngine({engine});"
                ));
            }
            for view in win.tabs.values() {
                let _=view.evaluate_script(&format!("window.__pipLinkPreviewEnabled={preview};if(!{preview}){{let tip=document.getElementById('__evergreen_status_bubble');if(tip)tip.style.display='none';}}"));
            }
        }
    }

    fn ensure_tab_view(&mut self, win_id: WindowId, id: TabId) -> bool {
        let Some(win) = self.windows.get(&win_id) else {
            return false;
        };
        if win.tabs.contains_key(&id) {
            return true;
        }
        let Some(tab) = win.tab_manager.get_tab_by_id(id) else {
            return false;
        };
        let result = self.create_tab_webview(
            win_id,
            id,
            &tab.url,
            &win.window,
            win.window_width,
            win.window_height,
            win.is_sidebar_open,
            win.current_chrome_height(),
            None,
        );
        if let Ok(view) = result {
            if let Some(win) = self.windows.get_mut(&win_id) {
                win.tabs.insert(id, view);
                win.tab_persisted.insert(id, !win.private);
                return true;
            }
        }
        false
    }

    fn configure_autofill(&self, view: &WebView, profile_id: &str, private: bool) {
        use webview2_com::Microsoft::Web::WebView2::Win32::*;
        use windows::core::Interface;
        use wry::WebViewExtWindows;
        let profile = self
            .settings
            .profiles
            .iter()
            .find(|profile| profile.id == profile_id);
        let autofill_enabled = !private && profile.is_some_and(|profile| profile.autofill);
        let password_autosave_enabled =
            !private && profile.is_some_and(|profile| profile.password_autosave);
        unsafe {
            if let Ok(profile) = view
                .controller()
                .CoreWebView2()
                .and_then(|c| c.cast::<ICoreWebView2_13>())
                .and_then(|c| c.Profile())
                .and_then(|p| p.cast::<ICoreWebView2Profile6>())
            {
                let _ = profile.SetIsPasswordAutosaveEnabled(password_autosave_enabled);
                let _ = profile.SetIsGeneralAutofillEnabled(autofill_enabled);
            }
        }
    }

    fn browser_extension_profile(
        &self,
        win_id: WindowId,
    ) -> Result<webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Profile7, String> {
        use webview2_com::Microsoft::Web::WebView2::Win32::{
            ICoreWebView2Profile7, ICoreWebView2_13,
        };
        use windows::core::Interface;
        use wry::WebViewExtWindows;

        let win = self
            .windows
            .get(&win_id)
            .filter(|win| !win.private)
            .ok_or_else(|| "Tiện ích bị tắt trong cửa sổ ẩn danh.".to_string())?;
        let view = win
            .tab_manager
            .active_tab_id()
            .and_then(|id| win.tabs.get(&id))
            .ok_or_else(|| "Không tìm thấy tab WebView2 đang hoạt động.".to_string())?;
        unsafe {
            view.controller()
                .CoreWebView2()
                .and_then(|core| core.cast::<ICoreWebView2_13>())
                .and_then(|core| core.Profile())
                .and_then(|profile| profile.cast::<ICoreWebView2Profile7>())
        }
        .map_err(|_| {
            "WebView2 Runtime chưa hỗ trợ tiện ích. Cần Runtime 120.0.2210.55 trở lên.".into()
        })
    }

    fn request_browser_extensions(&self, win_id: WindowId) {
        use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2BrowserExtensionList;

        let profile = match self.browser_extension_profile(win_id) {
            Ok(profile) => profile,
            Err(message) => {
                self.notify_settings(win_id, &message);
                return;
            }
        };
        let proxy = self.proxy.clone();
        let callback = webview2_com::ProfileGetBrowserExtensionsCompletedHandler::create(Box::new(
            move |result, collection: Option<ICoreWebView2BrowserExtensionList>| {
                let extensions = result.map_err(|error| error.to_string()).and_then(|()| {
                    let collection = collection
                        .ok_or_else(|| "WebView2 không trả về danh sách tiện ích.".to_string())?;
                    let mut count = 0u32;
                    unsafe { collection.Count(&mut count) }.map_err(|error| error.to_string())?;
                    let mut items = Vec::with_capacity(count.min(256) as usize);
                    for index in 0..count.min(256) {
                        let extension = unsafe { collection.GetValueAtIndex(index) }
                            .map_err(|error| error.to_string())?;
                        let mut id = windows::core::PWSTR::null();
                        let mut name = windows::core::PWSTR::null();
                        let mut enabled = windows::core::BOOL(0);
                        unsafe {
                            extension.Id(&mut id).map_err(|error| error.to_string())?;
                            extension
                                .Name(&mut name)
                                .map_err(|error| error.to_string())?;
                            extension
                                .IsEnabled(&mut enabled)
                                .map_err(|error| error.to_string())?;
                        }
                        items.push(BrowserExtensionInfo {
                            id: if id.is_null() {
                                String::new()
                            } else {
                                webview2_com::take_pwstr(id)
                            },
                            name: if name.is_null() {
                                "Tiện ích không tên".into()
                            } else {
                                webview2_com::take_pwstr(name)
                            },
                            enabled: enabled.as_bool(),
                        });
                    }
                    items.sort_by(|left, right| {
                        left.name.to_lowercase().cmp(&right.name.to_lowercase())
                    });
                    Ok(items)
                });
                let _ = proxy.send_event(BrowserEvent::BrowserExtensionsLoaded(win_id, extensions));
                Ok(())
            },
        ));
        if unsafe { profile.GetBrowserExtensions(&callback) }.is_err() {
            self.notify_settings(win_id, "Không thể đọc danh sách tiện ích của hồ sơ này.");
        }
    }

    fn install_browser_extension(&self, win_id: WindowId, path: std::path::PathBuf) {
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PCWSTR;

        let path = match validate_browser_extension_folder(&path) {
            Ok(path) => path,
            Err(message) => {
                self.notify_settings(win_id, &message);
                return;
            }
        };
        let profile = match self.browser_extension_profile(win_id) {
            Ok(profile) => profile,
            Err(message) => {
                self.notify_settings(win_id, &message);
                return;
            }
        };
        let path = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let proxy = self.proxy.clone();
        let callback = webview2_com::ProfileAddBrowserExtensionCompletedHandler::create(Box::new(
            move |result, extension| {
                let result = match result {
                    Err(error) => Err(format!("WebView2 không thể cài tiện ích: {error}")),
                    Ok(()) if extension.is_some() => Ok(()),
                    Ok(()) => Err("WebView2 không trả về tiện ích sau khi cài đặt.".into()),
                };
                let _ =
                    proxy.send_event(BrowserEvent::BrowserExtensionOperationDone(win_id, result));
                Ok(())
            },
        ));
        if let Err(error) = unsafe { profile.AddBrowserExtension(PCWSTR(path.as_ptr()), &callback) }
        {
            self.notify_settings(win_id, &format!("Không thể cài tiện ích: {error}"));
        }
    }

    fn change_browser_extension(&self, win_id: WindowId, id: String, enabled: Option<bool>) {
        use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2BrowserExtensionList;

        if id.len() > 128
            || id.is_empty()
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
        {
            self.notify_settings(win_id, "Mã tiện ích không hợp lệ.");
            return;
        }
        let profile = match self.browser_extension_profile(win_id) {
            Ok(profile) => profile,
            Err(message) => {
                self.notify_settings(win_id, &message);
                return;
            }
        };
        let proxy = self.proxy.clone();
        let callback = webview2_com::ProfileGetBrowserExtensionsCompletedHandler::create(Box::new(
            move |result, collection: Option<ICoreWebView2BrowserExtensionList>| {
                let operation = result.and_then(|()| {
                    let collection = collection
                        .ok_or_else(|| windows::core::Error::from(windows::core::HRESULT(-1)))?;
                    let mut count = 0u32;
                    unsafe { collection.Count(&mut count) }?;
                    let mut target = None;
                    for index in 0..count.min(256) {
                        let extension = unsafe { collection.GetValueAtIndex(index) }?;
                        let mut extension_id = windows::core::PWSTR::null();
                        unsafe { extension.Id(&mut extension_id)? };
                        let extension_id = webview2_com::take_pwstr(extension_id);
                        if extension_id == id {
                            target = Some(extension);
                            break;
                        }
                    }
                    target.ok_or_else(|| windows::core::Error::from(windows::core::HRESULT(-1)))
                });
                let Ok(extension) = operation else {
                    let _ = proxy.send_event(BrowserEvent::BrowserExtensionOperationDone(
                        win_id,
                        Err("Không tìm thấy tiện ích trong hồ sơ này.".into()),
                    ));
                    return Ok(());
                };
                let proxy_done = proxy.clone();
                let done = move |result: windows::core::Result<()>| {
                    let action = if enabled.is_some() {
                        "đổi trạng thái"
                    } else {
                        "xóa"
                    };
                    let _ = proxy_done.send_event(BrowserEvent::BrowserExtensionOperationDone(
                        win_id,
                        result.map_err(|error| {
                            format!("Không thể {action} tiện ích qua WebView2: {error}")
                        }),
                    ));
                    Ok(())
                };
                let call = unsafe {
                    match enabled {
                        Some(enabled) => {
                            let callback =
                                webview2_com::BrowserExtensionEnableCompletedHandler::create(
                                    Box::new(done),
                                );
                            extension.Enable(enabled, &callback)
                        }
                        None => {
                            let callback =
                                webview2_com::BrowserExtensionRemoveCompletedHandler::create(
                                    Box::new(done),
                                );
                            extension.Remove(&callback)
                        }
                    }
                };
                if let Err(error) = call {
                    let _ = proxy.send_event(BrowserEvent::BrowserExtensionOperationDone(
                        win_id,
                        Err(format!("Không thể cập nhật tiện ích: {error}")),
                    ));
                }
                Ok(())
            },
        ));
        if unsafe { profile.GetBrowserExtensions(&callback) }.is_err() {
            self.notify_settings(win_id, "Không thể cập nhật tiện ích trong hồ sơ này.");
        }
    }

    fn request_site_permissions(&self, win_id: WindowId) {
        use webview2_com::Microsoft::Web::WebView2::Win32::{
            ICoreWebView2Profile4, ICoreWebView2_13,
        };
        use windows::core::Interface;
        use wry::WebViewExtWindows;

        let Some(win) = self.windows.get(&win_id) else {
            return;
        };
        if win.private {
            self.notify_settings(
                win_id,
                "Không thể quản lý quyền đã lưu trong cửa sổ ẩn danh.",
            );
            return;
        }
        let Some(view) = win
            .tab_manager
            .active_tab_id()
            .and_then(|id| win.tabs.get(&id))
        else {
            self.notify_settings(win_id, "Không tìm thấy hồ sơ WebView2 đang hoạt động.");
            return;
        };

        let profile = unsafe {
            view.controller()
                .CoreWebView2()
                .and_then(|core| core.cast::<ICoreWebView2_13>())
                .and_then(|core| core.Profile())
                .and_then(|profile| profile.cast::<ICoreWebView2Profile4>())
        };
        let Ok(profile) = profile else {
            self.notify_settings(
                win_id,
                "WebView2 Runtime hiện tại chưa hỗ trợ quản lý quyền theo website.",
            );
            return;
        };

        let proxy = self.proxy.clone();
        let callback = webview2_com::GetNonDefaultPermissionSettingsCompletedHandler::create(
            Box::new(move |result, collection| {
                let permissions = result.map_err(|error| error.to_string()).and_then(|()| {
                    let Some(collection) = collection else {
                        return Err("WebView2 không trả về danh sách quyền.".to_string());
                    };
                    let mut count = 0u32;
                    unsafe { collection.Count(&mut count) }.map_err(|error| error.to_string())?;
                    let mut permissions = Vec::with_capacity(count.min(256) as usize);
                    for index in 0..count.min(256) {
                        let setting = unsafe { collection.GetValueAtIndex(index) }
                            .map_err(|error| error.to_string())?;
                        let mut kind = webview2_com::Microsoft::Web::WebView2::Win32::
                                COREWEBVIEW2_PERMISSION_KIND_UNKNOWN_PERMISSION;
                        let mut state = webview2_com::Microsoft::Web::WebView2::Win32::
                                COREWEBVIEW2_PERMISSION_STATE_DEFAULT;
                        let mut origin = windows::core::PWSTR::null();
                        unsafe {
                            setting
                                .PermissionKind(&mut kind)
                                .map_err(|error| error.to_string())?;
                            setting
                                .PermissionState(&mut state)
                                .map_err(|error| error.to_string())?;
                            setting
                                .PermissionOrigin(&mut origin)
                                .map_err(|error| error.to_string())?;
                        }
                        permissions.push(SitePermission {
                            origin: if origin.is_null() {
                                String::new()
                            } else {
                                webview2_com::take_pwstr(origin)
                            },
                            kind_id: kind.0,
                            kind_name: site_permission_kind_name(kind.0),
                            state_name: site_permission_state_name(state.0),
                        });
                    }
                    Ok(permissions)
                });
                let _ = proxy.send_event(BrowserEvent::SitePermissionsLoaded(win_id, permissions));
                Ok(())
            }),
        );
        if unsafe { profile.GetNonDefaultPermissionSettings(&callback) }.is_err() {
            self.notify_settings(win_id, "Không thể đọc quyền website đã lưu.");
        }
    }

    fn reset_site_permission(&self, win_id: WindowId, origin: String, kind: i32) {
        use webview2_com::Microsoft::Web::WebView2::Win32::{
            ICoreWebView2Profile4, ICoreWebView2_13, COREWEBVIEW2_PERMISSION_KIND,
            COREWEBVIEW2_PERMISSION_STATE_DEFAULT,
        };
        use windows::core::{Interface, PCWSTR};
        use wry::WebViewExtWindows;

        let Some(win) = self.windows.get(&win_id) else {
            return;
        };
        if win.private || !valid_permission_origin(&origin) || !(1..=12).contains(&kind) {
            self.notify_settings(win_id, "Quyền hoặc website không hợp lệ.");
            return;
        }
        let Some(view) = win
            .tab_manager
            .active_tab_id()
            .and_then(|id| win.tabs.get(&id))
        else {
            self.notify_settings(win_id, "Không tìm thấy hồ sơ WebView2 đang hoạt động.");
            return;
        };
        let profile = unsafe {
            view.controller()
                .CoreWebView2()
                .and_then(|core| core.cast::<ICoreWebView2_13>())
                .and_then(|core| core.Profile())
                .and_then(|profile| profile.cast::<ICoreWebView2Profile4>())
        };
        let Ok(profile) = profile else {
            self.notify_settings(
                win_id,
                "WebView2 Runtime hiện tại chưa hỗ trợ quản lý quyền theo website.",
            );
            return;
        };

        let origin_wide = origin
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let proxy = self.proxy.clone();
        let callback =
            webview2_com::SetPermissionStateCompletedHandler::create(Box::new(move |result| {
                let _ = proxy.send_event(BrowserEvent::SitePermissionReset(win_id, result.is_ok()));
                Ok(())
            }));
        let result = unsafe {
            profile.SetPermissionState(
                COREWEBVIEW2_PERMISSION_KIND(kind),
                PCWSTR(origin_wide.as_ptr()),
                COREWEBVIEW2_PERMISSION_STATE_DEFAULT,
                &callback,
            )
        };
        if result.is_err() {
            self.notify_settings(win_id, "Không thể đặt lại quyền website.");
        }
    }

    fn save_session(&self, win_id: WindowId) {
        let Some(win) = self.windows.get(&win_id) else {
            return;
        };
        if win.private {
            return;
        }
        let session_directory = get_data_directory().join("sessions");
        let tabs = win.tab_manager.snapshot();
        let path = session_directory.join(format!("{}.json", win.profile_id));
        let mut windows = load_session_windows(&path);
        update_session_slot(&mut windows, win.session_slot, tabs);
        if let Ok(bytes) = serde_json::to_vec(&windows) {
            if let Err(error) = persist_session_snapshot(&path, &bytes) {
                eprintln!("Save session: {error}");
            }
        }
        let closed_path = session_directory.join(format!("{}.closed.json", win.profile_id));
        let mut histories = load_closed_histories(&closed_path);
        update_session_slot(
            &mut histories,
            win.session_slot,
            win.tab_manager.closed_history_snapshot(),
        );
        if let Ok(bytes) = serde_json::to_vec(&histories) {
            if let Err(error) = persist_session_snapshot(&closed_path, &bytes) {
                eprintln!("Save recently closed tabs: {error}");
            }
        }
    }

    fn persist_open_window_profiles(&self) {
        if self.restoring_startup_windows {
            return;
        }
        #[cfg(debug_assertions)]
        if std::env::args().any(|arg| arg == "--verify-native-ui") {
            return;
        }
        let profiles = startup_profiles_for_windows(self.windows.values().map(|window| {
            (
                window.profile_id.clone(),
                window.session_slot,
                window.private,
            )
        }));
        // Keep the last normal session manifest when the final window is closed.
        // That close is also how users normally quit the browser.
        if let Err(error) = persist_startup_profiles(&startup_profiles_path(), &profiles) {
            eprintln!("Could not persist Pip startup windows: {error}");
        }
    }

    fn compact_and_save_open_sessions(&mut self) {
        self.compact_open_session_slots(None);
        let window_ids = self.windows.keys().copied().collect::<Vec<_>>();
        for window_id in window_ids {
            self.save_session(window_id);
        }
        self.persist_open_window_profiles();
    }

    fn restore_session(&mut self, win_id: WindowId) {
        let Some((profile_id, private, session_slot)) = self
            .windows
            .get(&win_id)
            .map(|win| (win.profile_id.clone(), win.private, win.session_slot))
        else {
            return;
        };
        if private {
            return;
        }
        let session_directory = get_data_directory().join("sessions");
        let closed_path = session_directory.join(format!("{profile_id}.closed.json"));
        let urls = load_closed_histories(&closed_path)
            .get(session_slot)
            .cloned()
            .unwrap_or_default();
        if let Some(win) = self.windows.get_mut(&win_id) {
            win.tab_manager.restore_closed_history(urls);
        }
        let path = session_directory.join(format!("{profile_id}.json"));
        let saved_windows = load_session_windows(&path);
        let Some(tabs) = saved_windows.get(session_slot).cloned() else {
            return;
        };
        let mut restored = Vec::new();
        for mut tab in tabs.into_iter().take(100) {
            if !is_restorable_session_url(&tab.url) {
                continue;
            }
            tab.id = self.next_tab_id();
            tab.is_audio_playing = false;
            tab.is_loading = false;
            tab.is_muted = false;
            restored.push(tab);
        }
        if restored.is_empty() {
            return;
        }
        if let Some(win) = self.windows.get_mut(&win_id) {
            let blank = win.tab_manager.tabs().len() == 1
                && win.tab_manager.tabs()[0].url == "evergreen://newtab";
            if blank {
                win.tabs.clear();
                win.tab_persisted.clear();
                win.tab_manager.restore_from_snapshot(restored);
            } else {
                for tab in restored {
                    win.tab_manager.insert_tab(tab, None);
                }
            }
            if let Some(id) = win.tab_manager.active_tab_id() {
                let _ = self
                    .proxy
                    .send_event(BrowserEvent::Ipc(win_id, UiToHostMessage::SwitchTab { id }));
            }
            win.sync_ui_state();
        }
    }

    fn sync_google_dns(&self) {
        let enabled = self.settings.google_dns;
        let active = self.active_google_dns;
        let failed = self.google_dns_start_failed;
        let route_ok = self
            .google_dns_route_ok
            .map(|ok| ok.to_string())
            .unwrap_or_else(|| "null".into());
        let fallback_used = self.google_dns_fallback_used;
        let resolution_failed = self.google_dns_resolution_failed;
        for win in self.windows.values() {
            if let Some(chrome) = &win.chrome_webview {
                let _ = chrome.evaluate_script(&format!(
                    "if(window.__renderGoogleDns) window.__renderGoogleDns({enabled},{active},{failed},{route_ok},{fallback_used},{resolution_failed});"
                ));
            }
            for (id, view) in &win.tabs {
                if win
                    .tab_manager
                    .get_tab_by_id(*id)
                    .is_some_and(|tab| tab.url == "evergreen://settings")
                {
                    let _ = view.evaluate_script(&format!(
                        "if(window.__renderGoogleDns) window.__renderGoogleDns({enabled},{active},{failed},{route_ok},{fallback_used},{resolution_failed});"
                    ));
                }
            }
        }
    }

    fn start_google_dns_health_check(&mut self) {
        if !self.active_google_dns || self.google_dns_health_check_started {
            return;
        }
        let Some(port) = self.doh_proxy.as_ref().map(doh_proxy::DohProxy::port) else {
            return;
        };
        self.google_dns_health_check_started = true;
        let proxy = self.proxy.clone();
        if let Err(error) = std::thread::Builder::new()
            .name("pip-google-dns-startup-check".into())
            .spawn(move || {
                let result = doh_proxy::test_google_doh_through_proxy(port);
                let _ = proxy.send_event(BrowserEvent::GoogleDnsHealthCheckDone(result));
            })
        {
            self.google_dns_route_ok = Some(false);
            eprintln!("Could not start Pip Google DNS route check: {error}");
            self.sync_google_dns();
        }
    }

    fn record_google_dns_health(&mut self, result: &Result<(), String>) {
        if self
            .doh_proxy
            .as_ref()
            .is_some_and(doh_proxy::DohProxy::system_dns_fallback_used)
        {
            self.google_dns_fallback_used = true;
        }
        if self
            .doh_proxy
            .as_ref()
            .is_some_and(doh_proxy::DohProxy::both_resolvers_failed)
        {
            self.google_dns_resolution_failed = true;
        }
        self.google_dns_route_ok = Some(
            result.is_ok() && !self.google_dns_fallback_used && !self.google_dns_resolution_failed,
        );
        self.sync_google_dns();
    }

    fn google_dns_check_message(
        result: &Result<(), String>,
        fallback_used: bool,
        resolution_failed: bool,
    ) -> String {
        match result {
            Ok(()) if fallback_used => {
                let failed_detail = if resolution_failed {
                    " Một hostname khác cũng đã thất bại với cả Google DoH và DNS Windows."
                } else {
                    ""
                };
                format!("Yêu cầu kiểm tra tải Google được qua proxy Pip. Trong phiên này, ít nhất một hostname đã dùng DNS Windows dự phòng; mạng Windows có thể nhìn thấy hostname đó, nên Google DNS chưa được xác minh đầy đủ.{failed_detail}")
            }
            Ok(()) if resolution_failed => "Yêu cầu kiểm tra tải Google được qua proxy Pip, nhưng trong phiên này có hostname thất bại với cả Google DoH và DNS Windows.".into(),
            Ok(()) => "Đã thử tải Google thành công qua proxy DNS của Pip. DNS Windows không bị thay đổi.".into(),
            Err(error) => {
                let reason: String = error.chars().take(120).collect();
                let detail = if fallback_used {
                    " Pip đã phải dùng DNS Windows dự phòng cho ít nhất một hostname; hostname đó có thể được mạng Windows nhìn thấy."
                } else if resolution_failed {
                    " Google DoH và DNS Windows đều không phân giải được ít nhất một hostname."
                } else {
                    ""
                };
                format!("Tuyến DNS của Pip chưa tải được Google. Kiểm tra mạng hoặc tường lửa.{detail} Chi tiết: {reason}")
            }
        }
    }

    fn notify_settings(&self, win_id: WindowId, message: &str) {
        if let Some(win) = self.windows.get(&win_id) {
            if let Some(view) = win.tab_manager.active_tab_id().and_then(|id| {
                win.tab_manager
                    .get_tab_by_id(id)
                    .filter(|tab| tab.url == "evergreen://settings")
                    .and_then(|_| win.tabs.get(&id))
            }) {
                let encoded = serde_json::to_string(message).unwrap();
                let _ = view.evaluate_script(&format!("showStatus({encoded})"));
            }
        }
    }

    fn render_remote_sync_tabs(
        &self,
        win_id: WindowId,
        profile_id: &str,
        payload: &sync_v2::SyncPayloadV2,
    ) {
        let Some(devices) = payload.open_tabs.as_ref().map(|category| &category.devices) else {
            return;
        };
        let local_device_id = self
            .settings
            .profiles
            .iter()
            .find(|profile| profile.id == profile_id)
            .and_then(|profile| profile.sync_device_id.as_deref())
            .unwrap_or_default();
        let Some(view) = self.windows.get(&win_id).and_then(|window| {
            window
                .tab_manager
                .tabs()
                .iter()
                .find(|tab| tab.url == "evergreen://settings")
                .and_then(|tab| window.tabs.get(&tab.id))
        }) else {
            return;
        };
        let devices = serde_json::to_string(devices).unwrap_or_else(|_| "[]".into());
        let local_device_id =
            serde_json::to_string(local_device_id).unwrap_or_else(|_| "\"\"".into());
        let _ = view.evaluate_script(&format!(
            "if(window.__renderSyncedTabs)window.__renderSyncedTabs({devices},{local_device_id});"
        ));
    }

    fn notify_user(&self, win_id: WindowId, message: &str) {
        let Some(chrome) = self
            .windows
            .get(&win_id)
            .and_then(|win| win.chrome_webview.as_ref())
        else {
            return;
        };
        let encoded = serde_json::to_string(message).unwrap_or_else(|_| "\"Pip\"".into());
        let _ = chrome.evaluate_script(&format!(
            "if(window.__notifyUser)window.__notifyUser({encoded})"
        ));
    }

    fn sync_profiles(&self) {
        let public_profiles = self
            .settings
            .profiles
            .iter()
            .map(|profile| {
                serde_json::json!({
                    "id": profile.id,
                    "name": profile.name,
                    "autofill": profile.autofill,
                    "passwordAutosave": profile.password_autosave,
                    "syncEnabled": profile.sync_credentials_protected.is_some(),
                    "syncBusy": self.syncing_profiles.contains(&profile.id),
                    "syncHistory": profile.sync_history,
                    "syncOpenTabs": profile.sync_open_tabs,
                    "syncSettings": profile.sync_settings,
                    "syncPasswords": profile.sync_passwords,
                    "isOpen": self.windows.values().any(|window| !window.private && window.profile_id == profile.id),
                    "isPrivateOpen": self.windows.values().any(|window| window.private && window.profile_id == profile.id),
                })
            })
            .collect::<Vec<_>>();
        let json = serde_json::to_string(&public_profiles).unwrap();
        for win in self.windows.values() {
            if let Some(profile) = self
                .settings
                .profiles
                .iter()
                .find(|p| p.id == win.profile_id)
            {
                let label = format!(
                    "{}{}",
                    profile.name,
                    if win.private { " · Ẩn danh" } else { "" }
                );
                win.window.set_title(&format!("Pip — {label}"));
                if let Some(chrome) = &win.chrome_webview {
                    let profile_id = serde_json::to_string(&win.profile_id).unwrap();
                    let private = if win.private { "true" } else { "false" };
                    let _ = chrome.evaluate_script(&format!(
                        "if(window.__renderProfileMenu)window.__renderProfileMenu({json},{profile_id},{private})"
                    ));
                }
            }
            for (id, view) in &win.tabs {
                if win
                    .tab_manager
                    .get_tab_by_id(*id)
                    .is_some_and(|tab| tab.url == "evergreen://settings")
                {
                    let _ = view.evaluate_script(&format!(
                        "if(window.__renderProfiles) window.__renderProfiles({json});"
                    ));
                }
            }
        }
    }
    fn new(proxy: EventLoopProxy<BrowserEvent>, runtime_version: String) -> Self {
        let settings_path = get_settings_path();
        let mut settings =
            evergreen_core::settings::Settings::load_from_path(&settings_path).unwrap_or_default();
        let mut profile_ids = std::collections::HashSet::new();
        settings.profiles.retain_mut(|profile| {
            if !evergreen_core::settings::valid_profile_id(&profile.id)
                || !profile_ids.insert(profile.id.clone())
            {
                return false;
            }
            if !evergreen_core::settings::valid_profile_name(&profile.name) {
                profile.name = if profile.id == "Default" {
                    "Cá nhân"
                } else {
                    "Hồ sơ"
                }
                .into();
            }
            true
        });
        settings.profiles.truncate(20);
        if !settings.profiles.iter().any(|p| p.id == "Default") {
            settings
                .profiles
                .extend(evergreen_core::settings::default_profiles());
        }
        let google_dns_applied = settings.google_dns;
        let mut failed_closed_dns_proxy = None;
        let doh_proxy = if google_dns_applied {
            let dns_event_proxy = proxy.clone();
            match doh_proxy::DohProxy::start_with_fallback_reporter(move |report| {
                let _ = dns_event_proxy.send_event(BrowserEvent::GoogleDnsResolverReport(report));
            }) {
                Ok(proxy) => Some(proxy),
                Err(error) => {
                    eprintln!("Pip Google DNS proxy failed to start: {error}");
                    match doh_proxy::RejectingProxy::start() {
                        Ok(proxy) => failed_closed_dns_proxy = Some(proxy),
                        Err(error) => eprintln!(
                            "Pip Google DNS rejection proxy failed to start; WebView2 will block hostname resolution locally, but numeric IP URLs can still connect directly: {error}"
                        ),
                    }
                    None
                }
            }
        } else {
            None
        };
        let google_dns_start_failed = google_dns_applied && doh_proxy.is_none();
        let startup_profiles = std::fs::read(startup_profiles_path())
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Vec<String>>(&bytes).ok())
            .unwrap_or_default();
        Self {
            windows: HashMap::new(),
            internal_ipc_token: format!("{:?}", unsafe {
                windows::Win32::System::Com::CoCreateGuid().expect("Create IPC capability")
            }),
            adblock: std::rc::Rc::new(std::cell::RefCell::new(adblock::Blocker::load(
                get_data_directory(),
                settings.adblock_mode,
            ))),
            proxy,
            cert_cache: cert::CertificateCache::new(),
            runtime_version,
            modifiers: ModifiersState::default(),
            background_check: std::env::args().any(|a| a == "--background-check")
                && std::env::args().any(|a| a == "--portable"),
            google_dns_applied,
            active_google_dns: doh_proxy.is_some(),
            google_dns_start_failed,
            google_dns_health_check_started: false,
            google_dns_route_ok: None,
            google_dns_fallback_used: false,
            google_dns_resolution_failed: false,
            doh_proxy,
            failed_closed_dns_proxy,
            restart_requested: false,
            startup_profiles,
            restoring_startup_windows: false,
            settings,
            next_tab_id: AtomicU64::new(1),
            allowed_cert_hosts: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
            next_session_autosave: Instant::now() + SESSION_AUTOSAVE_INTERVAL,
            next_background_tab_check: Instant::now() + Duration::from_secs(15),
            suspending_tabs: HashSet::new(),
            suspending_urls: HashMap::new(),
            sleep_checking_tabs: HashSet::new(),
            discard_checking_tabs: HashSet::new(),
            syncing_profiles: HashSet::new(),
            pending_sync_profiles: HashSet::new(),
            #[cfg(debug_assertions)]
            force_memory_pressure_for_tests: false,
        }
    }

    pub fn now_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    fn now_millis() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(u64::MAX as u128) as u64
    }

    fn memory_pressure_detected(&self) -> bool {
        #[cfg(debug_assertions)]
        if self.force_memory_pressure_for_tests {
            return true;
        }
        available_physical_memory_bytes()
            .is_some_and(|available| available < LOW_MEMORY_AVAILABLE_BYTES)
    }

    fn next_tab_id(&self) -> TabId {
        TabId(self.next_tab_id.fetch_add(1, Ordering::SeqCst))
    }

    fn suspend_idle_tabs(&mut self, now_secs: u64) {
        if !self.settings.performance.background_throttling
            || self.settings.performance.sleep_after_secs == 0
        {
            return;
        }
        let idle_after = self.settings.performance.sleep_after_secs;
        let low_memory = self.memory_pressure_detected();
        let mut suspend_candidates = Vec::new();
        let mut discard_candidates = Vec::new();
        for (window_id, win) in &self.windows {
            // Keep active downloads alive; WebView2 does not identify their source tab here.
            if win.permission_bar_open
                || win
                    .downloads
                    .values()
                    .any(|download| download["state"] == "InProgress")
            {
                continue;
            }
            let active = win.tab_manager.active_tab_id();
            for tab in win.tab_manager.tabs() {
                if tab.status == evergreen_core::tabs::TabStatus::Suspended {
                    if low_memory
                        && eligible_for_memory_discard(tab, now_secs)
                        && self.discard_checking_tabs.is_empty()
                    {
                        discard_candidates.push((
                            tab.last_active_timestamp_secs,
                            *window_id,
                            tab.id,
                        ));
                    }
                    continue;
                }
                if Some(tab.id) == active
                    || tab.status == evergreen_core::tabs::TabStatus::Crashed
                    || tab.status == evergreen_core::tabs::TabStatus::Discarded
                    || !eligible_for_memory_suspend(tab, now_secs, idle_after)
                    || !win.tabs.contains_key(&tab.id)
                    || self.suspending_tabs.contains(&(*window_id, tab.id))
                    || self.sleep_checking_tabs.contains(&(*window_id, tab.id))
                {
                    continue;
                }
                suspend_candidates.push((*window_id, tab.id));
            }
        }
        for (window_id, tab_id) in suspend_candidates {
            let Some(view) = self
                .windows
                .get(&window_id)
                .and_then(|win| win.tabs.get(&tab_id))
            else {
                continue;
            };
            let proxy = self.proxy.clone();
            self.sleep_checking_tabs.insert((window_id, tab_id));
            if view
                .evaluate_script_with_callback(
                    "JSON.stringify({safe:window.__pipCanDiscard?.() === true,href:location.href})",
                    move |result| {
                        let (safe, url) =
                            parse_memory_safety_result(&result).unwrap_or((false, String::new()));
                        let _ = proxy.send_event(BrowserEvent::TabSuspendSafetyResult(
                            window_id, tab_id, safe, url,
                        ));
                    },
                )
                .is_err()
            {
                self.sleep_checking_tabs.remove(&(window_id, tab_id));
            }
        }
        discard_candidates.sort_by_key(|(last_active, _, _)| *last_active);
        if let Some((_, window_id, tab_id)) = discard_candidates.into_iter().next() {
            let Some(view) = self
                .windows
                .get(&window_id)
                .and_then(|win| win.tabs.get(&tab_id))
            else {
                return;
            };
            let proxy = self.proxy.clone();
            self.discard_checking_tabs.insert((window_id, tab_id));
            if view
                .evaluate_script_with_callback(
                    "JSON.stringify({safe:window.__pipCanDiscard?.() === true,href:location.href})",
                    move |result| {
                        let (safe, url) =
                            parse_memory_safety_result(&result).unwrap_or((false, String::new()));
                        let _ = proxy.send_event(BrowserEvent::TabDiscardSafetyResult(
                            window_id, tab_id, safe, url,
                        ));
                    },
                )
                .is_err()
            {
                self.discard_checking_tabs.remove(&(window_id, tab_id));
            }
        }
    }

    fn create_browser_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        initial_url: Option<String>,
    ) -> Option<WindowId> {
        let restore_slot = (initial_url.is_none()
            && !self
                .windows
                .values()
                .any(|window| !window.private && window.profile_id == "Default"))
        .then_some(0);
        self.create_profile_window(
            event_loop,
            initial_url,
            "Default".into(),
            false,
            restore_slot,
        )
    }

    fn open_related_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        source: WindowId,
        private: bool,
    ) {
        if let Some(win) = self.windows.get(&source) {
            let profile = win.profile_id.clone();
            self.create_profile_window(event_loop, None, profile, private, None);
        }
    }

    fn next_profile_session_slot(&self, profile_id: &str) -> usize {
        self.windows
            .values()
            .filter(|window| !window.private && window.profile_id == profile_id)
            .map(|window| window.session_slot)
            .max()
            .map_or(0, |slot| slot + 1)
    }

    fn compact_open_session_slots(&mut self, excluded_profile: Option<&str>) {
        let window_ids = self.windows.keys().copied().collect::<Vec<_>>();
        let mut next_slot = HashMap::<String, usize>::new();
        for window_id in window_ids {
            let Some(window) = self.windows.get_mut(&window_id) else {
                continue;
            };
            if window.private || excluded_profile == Some(window.profile_id.as_str()) {
                continue;
            }
            let slot = next_slot.entry(window.profile_id.clone()).or_default();
            window.session_slot = *slot;
            *slot += 1;
        }
    }

    fn create_profile_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        initial_url: Option<String>,
        profile_id: String,
        private: bool,
        restore_session_slot: Option<usize>,
    ) -> Option<WindowId> {
        let session_slot = if private {
            0
        } else {
            restore_session_slot.unwrap_or_else(|| self.next_profile_session_slot(&profile_id))
        };
        let mut window_attributes = window::default_window_attributes();
        let profile_name = self
            .settings
            .profiles
            .iter()
            .find(|p| p.id == profile_id)
            .map(|p| p.name.as_str())
            .unwrap_or("Cá nhân");
        let label = format!(
            "Pip — {}{}",
            profile_name,
            if private { " — Ẩn danh" } else { "" }
        );
        window_attributes = window_attributes.with_visible(false).with_title(&label);

        let window = match event_loop.create_window(window_attributes) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                eprintln!("Failed to create window: {:?}", e);
                return None;
            }
        };
        let win_id = window.id();

        let scale = window.scale_factor();
        let physical_size = window.inner_size();
        let logical = physical_size.to_logical::<f64>(scale);
        let win_width = if logical.width > 0.0 {
            logical.width
        } else {
            1280.0
        };
        let win_height = if logical.height > 0.0 {
            logical.height
        } else {
            800.0
        };

        #[cfg(target_os = "windows")]
        {
            use windows::Win32::Foundation::HWND;
            use windows::Win32::Graphics::Gdi::{GetSysColorBrush, COLOR_WINDOW};
            use windows::Win32::UI::WindowsAndMessaging::{SetClassLongPtrW, GCLP_HBRBACKGROUND};
            use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

            #[link(name = "dwmapi")]
            extern "system" {
                fn DwmSetWindowAttribute(
                    hwnd: isize,
                    dwAttribute: u32,
                    pvAttribute: *const std::ffi::c_void,
                    cbAttribute: u32,
                ) -> i32;
            }

            if let Ok(handle) = window.window_handle() {
                if let RawWindowHandle::Win32(win32_handle) = handle.as_raw() {
                    let hwnd = HWND(win32_handle.hwnd.get() as _);
                    let background_brush = unsafe { GetSysColorBrush(COLOR_WINDOW) };
                    let _ = unsafe {
                        SetClassLongPtrW(hwnd, GCLP_HBRBACKGROUND, background_brush.0 as isize)
                    };

                    let dark_mode: i32 = 0;
                    let _ = unsafe {
                        DwmSetWindowAttribute(hwnd.0 as _, 20, &dark_mode as *const _ as _, 4)
                    };
                    let caption_color: u32 = 0x00e5f0ff; // Pip warm cream, RGB(255,240,229).
                    let _ = unsafe {
                        DwmSetWindowAttribute(hwnd.0 as _, 35, &caption_color as *const _ as _, 4)
                    };
                    let text_color: u32 = 0x002e2017;
                    let _ = unsafe {
                        DwmSetWindowAttribute(hwnd.0 as _, 36, &text_color as *const _ as _, 4)
                    };
                }
            }
        }

        let mut win_ctx = WindowContext {
            downloads: if private {
                HashMap::new()
            } else {
                load_download_history(&profile_id)
            },
            proxy: self.proxy.clone(),
            profile_id: profile_id.clone(),
            session_slot,
            dns_proxy_port: self.dns_proxy_browser_port(),
            dns_proxy_required: self.google_dns_applied,
            private,
            id: win_id,
            window: window.clone(),
            tab_manager: TabManager::new(),
            chrome_webview: None,
            sidebar_webview: None,
            is_sidebar_open: false,
            sidebar_mode: "menu".to_string(),
            tabs: HashMap::new(),
            tab_persisted: HashMap::new(),
            site_zoom: load_site_zoom(&profile_id, private),
            window_width: win_width,
            window_height: win_height,
            is_fullscreen: false,
            find_bar_open: false,
            permission_bar_open: false,
            profile_menu_open: false,
        };

        // Create Chrome Strip WebView
        let chrome_bounds = create_bounds(0.0, 0.0, win_width, CHROME_HEIGHT);
        let proxy_ipc = self.proxy.clone();

        let profile_settings = self.settings.for_profile(&profile_id);
        let chrome_html = chrome::get_chrome_html_with_appearance(
            &profile_settings.search_engine,
            &profile_settings.appearance.theme,
            &profile_settings.appearance.density,
        )
        .replace(
            "{{PROFILE_LABEL}}",
            &profile_name
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('"', "&quot;"),
        )
        .replace("{{PRIVATE_LABEL}}", if private { " · Ẩn danh" } else { "" });

        let chrome_builder = WebViewBuilder::new()
            .with_additional_browser_args(webview_browser_args(
                win_ctx.dns_proxy_required,
                win_ctx.dns_proxy_port,
            ))
            .with_bounds(chrome_bounds)
            .with_browser_extensions_enabled(true);
        match chrome_builder
            .with_html(&chrome_html)
            .with_navigation_handler(|url| {
                url == "about:blank" || url.starts_with("data:text/html")
            })
            .with_transparent(true)
            .with_background_color((24, 24, 32, 255))
            .with_ipc_handler(move |req: wry::http::Request<String>| {
                if let Ok(msg) = serde_json::from_str::<UiToHostMessage>(req.body()) {
                    let _ = proxy_ipc.send_event(BrowserEvent::Ipc(win_id, msg));
                }
            })
            .build_as_child(window.as_ref())
        {
            Ok(chrome_wv) => {
                accelerator::attach_accelerator_keys(&chrome_wv, win_id, self.proxy.clone());
                let version_script = format!(
                    "if (window.__syncEngineInfo) {{ window.__syncEngineInfo('{}'); }}",
                    self.runtime_version
                );
                let _ = chrome_wv.evaluate_script(&version_script);
                let engine_script = format!(
                    "if (window.__syncSearchEngine) {{ window.__syncSearchEngine('{}'); }}",
                    profile_settings.search_engine
                );
                let _ = chrome_wv.evaluate_script(&engine_script);
                win_ctx.chrome_webview = Some(chrome_wv);
            }
            Err(e) => eprintln!("Failed to create chrome webview: {:?}", e),
        }

        // Sidebar webview is lazily instantiated on demand in open_sidebar()
        self.windows.insert(win_id, win_ctx);

        let target_url = initial_url.or_else(|| {
            std::env::args()
                .nth(1)
                .filter(|a| !a.is_empty() && !a.starts_with('-'))
        });
        self.handle_create_tab(win_id, target_url);
        if restore_session_slot.is_some() {
            self.restore_session(win_id);
        }
        if !private {
            self.save_session(win_id);
        }
        if !self.restoring_startup_windows && !private {
            self.persist_open_window_profiles();
        }

        // Isolated automation can exercise the real binary without stealing focus.
        // Both explicit flags are required; normal launches always show Pip.
        if !self.background_check {
            window.set_visible(true);
        }
        Some(win_id)
    }

    #[allow(clippy::too_many_arguments)]
    fn create_tab_webview(
        &self,
        win_id: WindowId,
        tab_id: TabId,
        url: &str,
        window: &Window,
        win_width: f64,
        win_height: f64,
        is_sidebar_open: bool,
        chrome_h: f64,
        popup_opener_url: Option<String>,
    ) -> Result<WebView, wry::Error> {
        let profile_id = self
            .windows
            .get(&win_id)
            .map(|context| context.profile_id.clone())
            .unwrap_or_else(|| "Default".to_string());
        let profile_settings = self.settings.for_profile(&profile_id);
        let content_width = if is_sidebar_open {
            (win_width - 320.0).max(1.0)
        } else {
            win_width
        };
        let bounds = create_bounds(
            0.0,
            chrome_h,
            content_width,
            (win_height - chrome_h).max(1.0),
        );

        let proxy_title = self.proxy.clone();
        let proxy_nav = self.proxy.clone();
        let proxy_ipc = self.proxy.clone();

        let runtime_ver = self.runtime_version.clone();
        let is_newtab = url.starts_with("evergreen://newtab") || url == "about:blank";
        let is_settings = url.starts_with("evergreen://settings");
        let is_not_found = url.starts_with("evergreen://404");
        let is_loading_page = url.starts_with("evergreen://loading");

        let init_script = format!(
            "window.__pipLinkPreviewEnabled={};\n{NAV_WATCHER_SCRIPT}\n{DISCARD_SAFETY_SCRIPT}",
            profile_settings.features.enable_link_preview
                && profile_settings.appearance.enable_link_preview
                && profile_settings.appearance.show_status_preview
        );
        let context = self.windows.get(&win_id).expect("tab window exists");
        let is_persistent = !context.private;
        let profile_id = context.profile_id.clone();
        let google_dns_active = self.active_google_dns;
        let google_dns_failed = self.google_dns_start_failed;
        let google_dns_route_ok = self.google_dns_route_ok;
        let google_dns_fallback_used = self.google_dns_fallback_used;
        let google_dns_resolution_failed = self.google_dns_resolution_failed;

        let browser_args = webview_browser_args(context.dns_proxy_required, context.dns_proxy_port);
        let mut builder = WebViewBuilder::new()
            .with_additional_browser_args(browser_args)
            .with_bounds(bounds)
            .with_browser_extensions_enabled(true)
            .with_incognito(!is_persistent)
            .with_profile_name(&profile_id)
            .with_transparent(true)
            .with_background_color((24, 24, 32, 255))
            .with_initialization_script(&init_script)
            .with_devtools(true);
        let profile_settings = self.settings.for_profile(&profile_id);
        let search_name = profile_settings.search_engine_display_name().to_string();
        let ipc_token = self.internal_ipc_token.clone();
        let settings_clone = profile_settings.clone();
        let protocol_token = ipc_token.clone();
        let protocol_profile = profile_id.clone();
        let protocol_search_name = search_name.clone();
        let protocol_private = !is_persistent;
        let fallback_runtime_ver = runtime_ver.clone();

        // Generate only the requested internal page. Keeping rendered home/error/loading
        // HTML (and duplicate base64 images) inside every tab wasted memory on remote tabs.
        builder = builder.with_custom_protocol("evergreen".into(), move |_webview_id, request| {
            let host = request.uri().host().unwrap_or("");
            let path = request.uri().path();
            if host == "newtab"
                || host == "home"
                || path == "/newtab"
                || path == "/home"
                || path == "newtab"
                || path == "home"
            {
                let html = chrome::protect_internal_html(
                    home_ui::get_home_html(
                        &protocol_search_name,
                        request.uri().query().unwrap_or("home"),
                        protocol_private,
                    ),
                    &protocol_token,
                );
                wry::http::Response::builder()
                    .header("Content-Type", "text/html; charset=utf-8")
                    .body(std::borrow::Cow::Owned(html.into_bytes()))
                    .unwrap()
            } else if host == "settings" || path == "/settings" || path == "settings" {
                let active_ver = evergreen_core::env::detect_webview2_runtime()
                    .unwrap_or_else(|| fallback_runtime_ver.clone());
                let dynamic_settings_html = chrome::protect_internal_html(
                    settings_ui::get_settings_html(
                        &active_ver,
                        &settings_clone,
                        google_dns_active,
                        google_dns_failed,
                        google_dns_route_ok,
                        google_dns_fallback_used,
                        google_dns_resolution_failed,
                    )
                    .replace("{{CURRENT_PROFILE}}", &protocol_profile)
                    .replace(
                        "{{CURRENT_PRIVATE}}",
                        if is_persistent { "false" } else { "true" },
                    ),
                    &protocol_token,
                );
                wry::http::Response::builder()
                    .header("Content-Type", "text/html; charset=utf-8")
                    .body(std::borrow::Cow::Owned(dynamic_settings_html.into_bytes()))
                    .unwrap()
            } else if host == "404" || path == "/404" || path == "404" {
                wry::http::Response::builder()
                    .header("Content-Type", "text/html; charset=utf-8")
                    .body(std::borrow::Cow::Owned(
                        chrome::protect_internal_html(
                            error_ui::get_not_found_html(),
                            &protocol_token,
                        )
                        .into_bytes(),
                    ))
                    .unwrap()
            } else if host == "loading" || path == "/loading" || path == "loading" {
                wry::http::Response::builder()
                    .header("Content-Type", "text/html; charset=utf-8")
                    .body(std::borrow::Cow::Owned(
                        chrome::protect_internal_html(
                            loading_ui::get_loading_html(),
                            &protocol_token,
                        )
                        .into_bytes(),
                    ))
                    .unwrap()
            } else {
                wry::http::Response::builder()
                    .status(404)
                    .body(std::borrow::Cow::Borrowed(&b"Not Found"[..]))
                    .unwrap()
            }
        });

        if is_newtab {
            builder = builder.with_html(chrome::protect_internal_html(
                home_ui::get_home_html(
                    &search_name,
                    url.split_once('?').map_or("home", |(_, view)| view),
                    !is_persistent,
                ),
                &ipc_token,
            ));
        } else if is_settings {
            let active_ver = evergreen_core::env::detect_webview2_runtime()
                .unwrap_or_else(|| runtime_ver.clone());
            builder = builder.with_html(chrome::protect_internal_html(
                settings_ui::get_settings_html(
                    &active_ver,
                    &profile_settings,
                    google_dns_active,
                    google_dns_failed,
                    google_dns_route_ok,
                    google_dns_fallback_used,
                    google_dns_resolution_failed,
                )
                .replace("{{CURRENT_PROFILE}}", &profile_id)
                .replace(
                    "{{CURRENT_PRIVATE}}",
                    if is_persistent { "false" } else { "true" },
                ),
                &ipc_token,
            ));
        } else if is_not_found {
            builder = builder.with_html(chrome::protect_internal_html(
                error_ui::get_not_found_html(),
                &ipc_token,
            ));
        } else if is_loading_page {
            builder = builder.with_html(chrome::protect_internal_html(
                loading_ui::get_loading_html(),
                &ipc_token,
            ));
        }

        let proxy_nav_interceptor = self.proxy.clone();
        let webview = builder
            .with_document_title_changed_handler(move |title: String| {
                let _ =
                    proxy_title.send_event(BrowserEvent::TabTitleChanged(win_id, tab_id, title));
            })
            .with_navigation_handler(move |nav_url: String| {
                if nav_url.starts_with("data:text/html") {
                    return true;
                }
                if nav_url.starts_with("evergreen://settings") {
                    let _ = proxy_nav_interceptor
                        .send_event(BrowserEvent::Ipc(win_id, UiToHostMessage::OpenSettings));
                    return false;
                }
                let _ = proxy_nav.send_event(BrowserEvent::TabNavigated(win_id, tab_id, nav_url));
                true
            })
            .with_ipc_handler(move |req: wry::http::Request<String>| {
                if let Some(msg) = chrome::internal_message(req.body(), &ipc_token) {
                    let _ = proxy_ipc.send_event(BrowserEvent::Ipc(win_id, msg));
                }
            })
            .build_as_child(window)?;

        self.configure_autofill(&webview, &profile_id, !is_persistent);
        adblock::attach(&webview, self.adblock.clone())?;
        downloads::attach(&webview, win_id, self.proxy.clone())?;
        accelerator::attach_accelerator_keys(&webview, win_id, self.proxy.clone());
        accelerator::attach_navigation_events(
            &webview,
            win_id,
            tab_id,
            self.proxy.clone(),
            self.allowed_cert_hosts.clone(),
            self.adblock.clone(),
            popup_opener_url,
        );

        // Register interception before the first remote navigation, including restored tabs.
        if !is_newtab && !is_settings && !is_not_found && !is_loading_page {
            webview.load_url(url)?;
        }

        Ok(webview)
    }

    fn handle_create_tab(&mut self, win_id: WindowId, url: Option<String>) {
        self.handle_create_tab_with_popup_context(win_id, url, None);
    }

    fn handle_create_popup_tab(&mut self, win_id: WindowId, url: String, opener_url: String) {
        self.handle_create_tab_with_popup_context(win_id, Some(url), Some(opener_url));
    }

    fn handle_create_tab_with_popup_context(
        &mut self,
        win_id: WindowId,
        url: Option<String>,
        popup_opener_url: Option<String>,
    ) {
        let tab_id = self.next_tab_id();
        let target_url = url.unwrap_or_else(|| "evergreen://newtab".to_string());

        if let Some(win_ctx) = self.windows.get_mut(&win_id) {
            let tab_state = TabState {
                id: tab_id,
                url: target_url.clone(),
                title: if target_url.starts_with("evergreen://settings") {
                    "Cài đặt — Pip".to_string()
                } else if target_url.starts_with("evergreen://404") {
                    "404 — Pip".to_string()
                } else if target_url.starts_with("evergreen://loading") {
                    "Đang tải — Pip".to_string()
                } else {
                    "Tab mới — Pip".to_string()
                },
                favicon_uri: None,
                is_loading: false,
                can_go_back: false,
                can_go_forward: false,
                is_audio_playing: false,
                is_muted: false,
                pinned: false,
                keep_awake: false,
                memory_discard_protected: false,
                group: None,
                status: evergreen_core::tabs::TabStatus::Active,
                last_active_timestamp_secs: Self::now_secs(),
            };
            let previous_id = win_ctx.tab_manager.active_tab_id();
            let actual_id = win_ctx.tab_manager.insert_tab(tab_state, None);

            if let Some(wv) = previous_id.and_then(|id| win_ctx.tabs.get(&id)) {
                accelerator::hide_webview(wv);
            }

            let window = win_ctx.window.clone();
            let win_width = win_ctx.window_width;
            let win_height = win_ctx.window_height;
            let is_sidebar_open = win_ctx.is_sidebar_open;
            let chrome_h = win_ctx.current_chrome_height();

            match self.create_tab_webview(
                win_id,
                actual_id,
                &target_url,
                window.as_ref(),
                win_width,
                win_height,
                is_sidebar_open,
                chrome_h,
                popup_opener_url,
            ) {
                Ok(wv) => {
                    let _ = wv.set_visible(true);
                    if let Some(win_ctx) = self.windows.get_mut(&win_id) {
                        let is_persistent = !win_ctx.private;
                        win_ctx.tabs.insert(actual_id, wv);
                        win_ctx.tab_persisted.insert(actual_id, is_persistent);
                        win_ctx.update_layout();
                        win_ctx.sync_ui_state();
                        win_ctx.update_sidebar_sync(&self.cert_cache);
                        let default_zoom = self
                            .settings
                            .for_profile(&win_ctx.profile_id)
                            .appearance
                            .default_zoom_level;
                        let initial_zoom = win_ctx.zoom_for_url(&target_url, default_zoom);
                        win_ctx.apply_tab_zoom(actual_id, initial_zoom);
                    }
                }
                Err(e) => {
                    eprintln!("Failed to create tab webview: {:?}", e);
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        win.tab_manager.extract_tab(actual_id);
                        if let Some(previous) = previous_id {
                            win.handle_switch_tab(previous, &self.cert_cache);
                        }
                        win.window
                            .set_title("Pip — Không thể tạo tab. Hãy thử lại.");
                    }
                }
            }
        }
        self.sync_profile_open_tabs(win_id);
    }

    fn library_path(&self, win_id: WindowId) -> Option<std::path::PathBuf> {
        let profile = &self.windows.get(&win_id)?.profile_id;
        Some(
            get_data_directory()
                .join("profiles")
                .join(profile)
                .join("library.json"),
        )
    }

    fn save_download_history(&self, profile_id: &str) {
        let mut downloads = self
            .windows
            .values()
            .filter(|win| !win.private && win.profile_id == profile_id)
            .flat_map(|win| win.downloads.iter())
            .filter(|(_, item)| item["state"] == "Completed" || item["state"] == "Interrupted")
            .map(|(id, item)| (*id, item.clone()))
            .collect::<HashMap<_, _>>();
        if downloads.len() > 500 {
            let mut ids = downloads.keys().copied().collect::<Vec<_>>();
            ids.sort_unstable();
            for id in ids.into_iter().take(downloads.len() - 500) {
                downloads.remove(&id);
            }
        }
        let mut entries = downloads.into_values().collect::<Vec<_>>();
        entries.sort_by_key(|item| item["download_id"].as_u64().unwrap_or_default());
        let path = download_history_path(profile_id);
        let staging = path.with_file_name(format!("downloads-{}.json.tmp", std::process::id()));
        if let Some(parent) = path.parent() {
            if std::fs::create_dir_all(parent).is_err() {
                return;
            }
        }
        if let Ok(bytes) = serde_json::to_vec(&entries) {
            if let Err(error) =
                std::fs::write(&staging, bytes).and_then(|_| std::fs::rename(staging, path))
            {
                eprintln!("Save download history: {error}");
            }
        }
    }

    fn clear_download_history(&mut self, win_id: WindowId) {
        let Some((profile_id, private)) = self
            .windows
            .get(&win_id)
            .map(|win| (win.profile_id.clone(), win.private))
        else {
            return;
        };
        if private {
            downloads::clear_finished(win_id);
            if let Some(win) = self.windows.get_mut(&win_id) {
                win.downloads
                    .retain(|_, item| item["state"] == "InProgress");
                win.update_sidebar_sync(&self.cert_cache);
            }
            return;
        }
        let profile_windows = self
            .windows
            .values()
            .filter(|win| !win.private && win.profile_id == profile_id)
            .map(|win| win.id)
            .collect::<Vec<_>>();
        for profile_window in profile_windows {
            downloads::clear_finished(profile_window);
        }
        for win in self
            .windows
            .values_mut()
            .filter(|win| !win.private && win.profile_id == profile_id)
        {
            win.downloads
                .retain(|_, item| item["state"] == "InProgress");
            win.update_sidebar_sync(&self.cert_cache);
        }
        self.save_download_history(&profile_id);
    }

    fn passwords_path(&self, win_id: WindowId) -> Option<std::path::PathBuf> {
        let profile = &self.windows.get(&win_id)?.profile_id;
        Some(
            get_data_directory()
                .join("profiles")
                .join(profile)
                .join("passwords.bin"),
        )
    }

    fn sync_passwords(&self, win_id: WindowId) {
        let Some(win) = self.windows.get(&win_id) else {
            return;
        };
        if win.private {
            return;
        }
        let profile_id = win.profile_id.clone();
        let result = self
            .passwords_path(win_id)
            .ok_or_else(|| "Không tìm thấy hồ sơ Pip.".to_string())
            .and_then(|path| passwords::load(&path));
        let entries = match result {
            Ok(entries) => passwords::summaries(&entries),
            Err(message) => {
                self.notify_settings(win_id, &message);
                return;
            }
        };
        let json = serde_json::to_string(&entries)
            .unwrap_or_else(|_| "[]".into())
            .replace('<', "\\u003c");
        for window in self
            .windows
            .values()
            .filter(|window| !window.private && window.profile_id == profile_id)
        {
            let targets = window
                .tab_manager
                .tabs()
                .iter()
                .filter_map(|tab| {
                    let origin = passwords::normalize_origin(&tab.url)?;
                    Some(serde_json::json!({
                        "id": tab.id.0,
                        "origin": origin,
                        "title": tab.title,
                    }))
                })
                .collect::<Vec<_>>();
            let targets = serde_json::to_string(&targets)
                .unwrap_or_else(|_| "[]".into())
                .replace('<', "\\u003c");
            for (tab_id, view) in &window.tabs {
                if window
                    .tab_manager
                    .get_tab_by_id(*tab_id)
                    .is_some_and(|tab| tab.url.starts_with("evergreen://settings"))
                {
                    let _ = view.evaluate_script(&format!(
                        "if(window.__renderPasswords)window.__renderPasswords({json},{targets})"
                    ));
                }
            }
        }
    }

    fn sync_password_vault_if_enabled(&mut self, win_id: WindowId) {
        let enabled = self
            .windows
            .get(&win_id)
            .filter(|win| !win.private)
            .and_then(|win| {
                self.settings
                    .profiles
                    .iter()
                    .find(|profile| profile.id == win.profile_id)
            })
            .is_some_and(|profile| profile.sync_passwords);
        if enabled {
            self.start_bookmark_sync(win_id, false);
        }
    }

    fn sync_library(&self, win_id: WindowId) {
        if let Some(win) = self.windows.get(&win_id) {
            if let Some(chrome) = &win.chrome_webview {
                let enabled = win
                    .tab_manager
                    .active_tab()
                    .and_then(|tab| self.adblock.borrow().enabled(&tab.url));
                let _ = chrome.evaluate_script(&format!(
                    "if(window.__renderAdblock)window.__renderAdblock({});",
                    serde_json::to_string(&enabled).unwrap()
                ));
            }
        }
        let Some(path) = self.library_path(win_id) else {
            return;
        };
        let Ok(library) = evergreen_core::library::Library::load(&path) else {
            return;
        };
        let Some(win) = self.windows.get(&win_id) else {
            return;
        };
        let active = win.tab_manager.active_tab();
        let bookmarked = active.is_some_and(|t| library.bookmarks.iter().any(|p| p.url == t.url));
        if let Some(chrome) = &win.chrome_webview {
            let _ = chrome.evaluate_script(&format!(
                "if(window.__renderBookmark)window.__renderBookmark({bookmarked});"
            ));
        }
        let shortcuts = library.home_shortcuts();
        let bookmarks = library
            .bookmarks
            .iter()
            .map(|page| {
                serde_json::json!({
                    "url": page.url,
                    "title": page.title,
                    "folder_id": library.bookmark_folder_for(&page.url),
                })
            })
            .collect::<Vec<_>>();
        let history = if win.private {
            Vec::<serde_json::Value>::new()
        } else {
            library
                .history_visits
                .iter()
                .map(|visit| {
                    serde_json::json!({
                        "url": visit.url,
                        "title": visit.title,
                        "visited_at_ms": visit.visited_at_ms,
                    })
                })
                .collect()
        };
        let data = serde_json::json!({"bookmarks":bookmarks,"bookmark_folders":library.bookmark_folders,"history":history,"shortcuts":shortcuts,"private":win.private});
        let script = format!("if(window.__renderLibrary)window.__renderLibrary({data});");
        for (id, view) in &win.tabs {
            if win
                .tab_manager
                .get_tab_by_id(*id)
                .is_some_and(|t| t.url.starts_with("evergreen://newtab"))
            {
                let _ = view.evaluate_script(&script);
            }
        }
    }

    fn suggest_omnibox(&self, win_id: WindowId, query: String) {
        let Some(win) = self.windows.get(&win_id) else {
            return;
        };
        let private = win.private;
        let Some(path) = self.library_path(win_id) else {
            return;
        };
        let library = evergreen_core::library::Library::load(&path).unwrap_or_default();
        let bookmarks = library
            .bookmarks
            .iter()
            .map(|page| page.url.as_str())
            .collect::<HashSet<_>>();
        let suggestions = if query.chars().count() <= 128 {
            library.suggestions(&query, !private, 8)
        } else {
            Vec::new()
        };
        let items = suggestions
            .into_iter()
            .map(|page| {
                serde_json::json!({
                    "url": page.url,
                    "title": page.title,
                    "bookmarked": bookmarks.contains(page.url.as_str()),
                })
            })
            .collect::<Vec<_>>();
        if let Some(chrome) = self
            .windows
            .get(&win_id)
            .and_then(|win| win.chrome_webview.as_ref())
        {
            let query = serde_json::to_string(&query).unwrap_or_else(|_| "\"\"".into());
            let items = serde_json::to_string(&items).unwrap_or_else(|_| "[]".into());
            let _ = chrome.evaluate_script(&format!(
                "if(window.__renderOmniboxSuggestions)window.__renderOmniboxSuggestions({query},{items});"
            ));
        }
    }
    fn edit_library(
        &mut self,
        win_id: WindowId,
        edit: impl FnOnce(&mut evergreen_core::library::Library),
    ) {
        let Some(path) = self.library_path(win_id) else {
            return;
        };
        let (history_sync_enabled, open_tabs_sync_enabled) = self
            .windows
            .get(&win_id)
            .filter(|window| !window.private)
            .and_then(|window| {
                self.settings
                    .profiles
                    .iter()
                    .find(|profile| profile.id == window.profile_id)
            })
            .map(|profile| (profile.sync_history, profile.sync_open_tabs))
            .unwrap_or_default();
        let result = (|| -> std::io::Result<bool> {
            let mut library = evergreen_core::library::Library::load(&path)?;
            let before = library.clone();
            edit(&mut library);
            let sync_needed = bookmark_sync_state_changed(&before, &library, history_sync_enabled);
            let sync_needed = sync_needed || open_tabs_sync_enabled;
            if before == library {
                return Ok(false);
            }
            library.save(&path).map(|()| sync_needed)
        })();
        let bookmarks_changed = match result {
            Ok(changed) => changed,
            Err(error) => {
                if let Some(win) = self.windows.get(&win_id) {
                    win.window
                        .set_title(&format!("Pip — Không lưu được thư viện: {error}"));
                }
                return;
            }
        };
        let Some(profile) = self.windows.get(&win_id).map(|win| win.profile_id.clone()) else {
            return;
        };
        for win in self
            .windows
            .values()
            .filter(|win| win.profile_id == profile)
        {
            self.sync_library(win.id);
        }
        if bookmarks_changed {
            self.start_bookmark_sync(win_id, false);
        }
    }

    fn ensure_sync_device_id(&mut self, profile_id: &str) -> Option<String> {
        if let Some(id) = self
            .settings
            .profiles
            .iter()
            .find(|profile| profile.id == profile_id)
            .and_then(|profile| profile.sync_device_id.as_deref())
            .filter(|id| id.len() == 32 && id.bytes().all(|byte| byte.is_ascii_hexdigit()))
        {
            return Some(id.to_string());
        }
        let id = sync::generate_device_id();
        let mut settings = self.settings.clone();
        let profile = settings.profiles.iter_mut().find(|p| p.id == profile_id)?;
        profile.sync_device_id = Some(id.clone());
        if settings.save_to_path(&get_settings_path()).is_err() {
            return None;
        }
        self.settings = settings;
        Some(id)
    }

    fn sync_settings_snapshot(
        &mut self,
        profile_id: &str,
        now_ms: u64,
    ) -> Option<sync_v2::SyncableSettings> {
        let effective = self.settings.for_profile(profile_id);
        let mut snapshot = sync::syncable_settings_from(&effective, 0);
        let fingerprint = sync::syncable_settings_fingerprint(&snapshot);
        let profile = self
            .settings
            .profiles
            .iter()
            .find(|profile| profile.id == profile_id)?;
        let previous_fingerprint = profile.sync_settings_fingerprint.clone();
        let updated_at = if previous_fingerprint.as_deref() == Some(&fingerprint) {
            profile.sync_settings_updated_at_ms
        } else if previous_fingerprint.is_some() {
            now_ms
        } else {
            // On the first device join, allow an existing remote preference snapshot to win.
            0
        };
        snapshot.updated_at_ms = updated_at;
        if previous_fingerprint.as_deref() != Some(&fingerprint)
            || profile.sync_settings_updated_at_ms != updated_at
        {
            let mut settings = self.settings.clone();
            let profile = settings
                .profiles
                .iter_mut()
                .find(|profile| profile.id == profile_id)?;
            profile.sync_settings_fingerprint = Some(fingerprint);
            profile.sync_settings_updated_at_ms = updated_at;
            if settings.save_to_path(&get_settings_path()).is_err() {
                return None;
            }
            self.settings = settings;
        }
        Some(snapshot)
    }

    fn start_bookmark_sync(&mut self, win_id: WindowId, manual: bool) {
        let Some(profile_id) = self
            .windows
            .get(&win_id)
            .filter(|win| !win.private)
            .map(|win| win.profile_id.clone())
        else {
            return;
        };
        if self.syncing_profiles.contains(&profile_id) {
            self.pending_sync_profiles.insert(profile_id);
            if manual {
                self.notify_settings(win_id, "Pip Sync của hồ sơ này đang chạy.");
            }
            return;
        }
        let Some((protected, sync_history, sync_open_tabs, sync_settings, sync_passwords)) = self
            .settings
            .profiles
            .iter()
            .find(|profile| profile.id == profile_id)
            .and_then(|profile| {
                profile
                    .sync_credentials_protected
                    .as_deref()
                    .map(|protected| {
                        (
                            protected.to_string(),
                            profile.sync_history,
                            profile.sync_open_tabs,
                            profile.sync_settings,
                            profile.sync_passwords,
                        )
                    })
            })
        else {
            if manual {
                self.notify_settings(win_id, "Tạo hoặc nhập mã Pip Sync trước.");
            }
            return;
        };
        let credentials = match sync::unprotect_credentials(&protected) {
            Ok(credentials) => credentials,
            Err(error) => {
                if manual {
                    self.notify_settings(win_id, &error.to_string());
                }
                return;
            }
        };
        let Some(path) = self.library_path(win_id) else {
            return;
        };
        let library = match evergreen_core::library::Library::load(&path) {
            Ok(library) => library,
            Err(_) => {
                if manual {
                    self.notify_settings(win_id, "Không đọc được dấu trang hồ sơ này.");
                }
                return;
            }
        };
        let mut snapshot = sync::sync_payload_from_library(&library, sync_history);
        if sync_passwords {
            let Some(password_path) = self.passwords_path(win_id) else {
                if manual {
                    self.notify_settings(win_id, "Không tìm thấy kho mật khẩu hồ sơ này.");
                }
                return;
            };
            let password_snapshot = match passwords::sync_projection(&password_path) {
                Ok(snapshot) => snapshot,
                Err(message) => {
                    if manual {
                        self.notify_settings(
                            win_id,
                            &format!("Không thể chuẩn bị mật khẩu Sync: {message}"),
                        );
                    }
                    return;
                }
            };
            snapshot.passwords = Some(sync_v2::PasswordsCategory {
                version: password_snapshot.version,
                entries: password_snapshot
                    .entries
                    .into_iter()
                    .map(|entry| sync_v2::SyncPasswordEntry {
                        origin: entry.origin,
                        username: entry.username,
                        password: entry.password,
                        updated_at_ms: entry.updated_at_ms,
                    })
                    .collect(),
                tombstones: password_snapshot
                    .tombstones
                    .into_iter()
                    .map(|item| sync_v2::SyncPasswordTombstone {
                        origin: item.origin,
                        username: item.username,
                        deleted_at_ms: item.deleted_at_ms,
                    })
                    .collect(),
            });
        }
        let now_ms = BrowserApp::now_millis();
        if sync_open_tabs {
            let Some(device_id) = self.ensure_sync_device_id(&profile_id) else {
                if manual {
                    self.notify_settings(win_id, "Không lưu được mã thiết bị Pip Sync.");
                }
                return;
            };
            let mut tabs = self
                .windows
                .values()
                .filter(|window| !window.private && window.profile_id == profile_id)
                .flat_map(|window| {
                    let active_id = window.tab_manager.active_tab_id();
                    window
                        .tab_manager
                        .tabs()
                        .iter()
                        .filter(|tab| syncable_tab_url(&tab.url))
                        .map(move |tab| {
                            (
                                active_id == Some(tab.id),
                                tab.last_active_timestamp_secs,
                                sync_v2::OpenTab {
                                    url: tab.url.clone(),
                                    title: tab
                                        .title
                                        .chars()
                                        .take(sync_v2::MAX_TITLE_CHARS)
                                        .collect(),
                                    pinned: tab.pinned,
                                    group: tab.group.as_ref().map(|group| group.name.clone()),
                                },
                            )
                        })
                })
                .collect::<Vec<_>>();
            tabs.sort_by(|a, b| {
                b.0.cmp(&a.0)
                    .then_with(|| b.1.cmp(&a.1))
                    .then_with(|| a.2.url.cmp(&b.2.url))
            });
            tabs.truncate(sync_v2::MAX_OPEN_TABS_PER_DEVICE);
            let tabs = tabs.into_iter().map(|(_, _, tab)| tab).collect();
            snapshot.open_tabs = Some(sync_v2::OpenTabsCategory {
                devices: vec![sync_v2::DeviceTabs {
                    device_id,
                    updated_at_ms: now_ms,
                    tabs,
                }],
            });
        }
        if sync_settings {
            snapshot.settings = self.sync_settings_snapshot(&profile_id, now_ms);
            if snapshot.settings.is_none() && manual {
                self.notify_settings(win_id, "Không lưu được dấu thời gian cài đặt Sync.");
            }
        }
        let dns_proxy_port = self.doh_proxy.as_ref().map(doh_proxy::DohProxy::port);
        let google_dns_enabled = self.google_dns_applied;
        let proxy = self.proxy.clone();
        self.syncing_profiles.insert(profile_id.clone());
        let event_profile_id = profile_id.clone();
        if std::thread::Builder::new()
            .name("pip-bookmark-sync".into())
            .spawn(move || {
                let result = sync::sync_payload_with_google_dns(
                    &credentials,
                    snapshot,
                    google_dns_enabled,
                    dns_proxy_port,
                );
                let _ = proxy.send_event(BrowserEvent::SyncBookmarksDone(
                    win_id,
                    event_profile_id,
                    manual,
                    Box::new(result),
                ));
            })
            .is_ok()
        {
            self.sync_profiles();
            if manual {
                self.notify_settings(win_id, "Đang mã hóa và đồng bộ dữ liệu đã chọn…");
            }
        } else {
            self.syncing_profiles.remove(&profile_id);
            if manual {
                self.notify_settings(win_id, "Không khởi chạy được đồng bộ.");
            }
        }
    }

    fn sync_profile_setting_changes(&mut self) {
        let windows = self
            .windows
            .iter()
            .filter(|(_, window)| {
                !window.private
                    && self
                        .settings
                        .profiles
                        .iter()
                        .find(|profile| profile.id == window.profile_id)
                        .is_some_and(|profile| profile.sync_settings)
            })
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        for window in windows {
            self.start_bookmark_sync(window, false);
        }
    }

    fn sync_profile_open_tabs(&mut self, win_id: WindowId) {
        let enabled = self
            .windows
            .get(&win_id)
            .filter(|window| !window.private)
            .and_then(|window| {
                self.settings
                    .profiles
                    .iter()
                    .find(|profile| profile.id == window.profile_id)
            })
            .is_some_and(|profile| profile.sync_open_tabs);
        if enabled {
            self.start_bookmark_sync(win_id, false);
        }
    }

    fn sync_library_profile(&self, profile_id: &str) {
        let windows = self
            .windows
            .values()
            .filter(|win| win.profile_id == profile_id)
            .map(|win| win.id)
            .collect::<Vec<_>>();
        for win_id in windows {
            self.sync_library(win_id);
        }
    }

    fn notify_library(&self, win_id: WindowId, message: &str) {
        if let Some(view) = self.windows.get(&win_id).and_then(|win| {
            win.tab_manager
                .active_tab_id()
                .and_then(|id| win.tab_manager.get_tab_by_id(id))
                .filter(|tab| tab.url.starts_with("evergreen://newtab"))
                .and_then(|tab| win.tabs.get(&tab.id))
        }) {
            let message = serde_json::to_string(message).unwrap();
            let _ = view.evaluate_script(&format!(
                "if(window.__showLibraryStatus)window.__showLibraryStatus({message});"
            ));
        }
    }

    fn import_bookmarks(&mut self, win_id: WindowId) {
        let Some(file) = self
            .windows
            .get(&win_id)
            .and_then(|win| choose_bookmarks_import_file(&win.window))
        else {
            return;
        };
        let Some(path) = self.library_path(win_id) else {
            return;
        };
        let Some(profile_id) = self.windows.get(&win_id).map(|win| win.profile_id.clone()) else {
            return;
        };
        let result = (|| -> Result<evergreen_core::library::BookmarkImportResult, String> {
            let file_size = std::fs::metadata(&file)
                .map_err(|error| format!("Không đọc được thông tin tệp dấu trang: {error}"))?
                .len();
            if file_size > 8 * 1024 * 1024 {
                return Err("Tệp dấu trang vượt quá giới hạn 8 MiB.".to_string());
            }
            let html = std::fs::read_to_string(file)
                .map_err(|error| format!("Không đọc được tệp dấu trang: {error}"))?;
            let mut library = evergreen_core::library::Library::load(&path)
                .map_err(|error| format!("Không đọc được hồ sơ dấu trang: {error}"))?;
            let imported = library
                .import_netscape_bookmarks(&html)
                .map_err(|error| error.to_string())?;
            library
                .save(&path)
                .map_err(|error| format!("Không lưu được dấu trang: {error}"))?;
            Ok(imported)
        })();
        match result {
            Ok(imported) => {
                self.sync_library_profile(&profile_id);
                if imported.bookmarks > 0 || imported.folders > 0 {
                    self.start_bookmark_sync(win_id, false);
                }
                let message = if imported.bookmarks == 0 && imported.folders == 0 {
                    "Đã nhập 0 dấu trang và 0 thư mục. Mục trùng được bỏ qua; hồ sơ đã đủ 200 dấu trang cũng không nhận thêm.".to_string()
                } else {
                    format!(
                        "Đã nhập {} dấu trang và {} thư mục vào hồ sơ hiện tại.",
                        imported.bookmarks, imported.folders
                    )
                };
                self.notify_library(win_id, &message);
            }
            Err(error) => self.notify_library(win_id, &error),
        }
    }

    fn export_bookmarks(&mut self, win_id: WindowId) {
        let Some(path) = self.library_path(win_id) else {
            return;
        };
        let Some(library) = evergreen_core::library::Library::load(&path).ok() else {
            self.notify_library(win_id, "Không đọc được dấu trang của hồ sơ hiện tại.");
            return;
        };
        let Some(destination) =
            accelerator::show_save_file_dialog("Pip Bookmarks.html", "Xuất dấu trang Pip")
        else {
            return;
        };
        match std::fs::write(destination, library.export_netscape_bookmarks()) {
            Ok(()) => self.notify_library(
                win_id,
                "Đã xuất dấu trang theo định dạng HTML của trình duyệt.",
            ),
            Err(error) => {
                self.notify_library(win_id, &format!("Không lưu được tệp dấu trang: {error}"))
            }
        }
    }

    fn handle_open_settings(&mut self, win_id: WindowId) {
        if let Some(win_ctx) = self.windows.get_mut(&win_id) {
            win_ctx.close_sidebar();
            if let Some(existing) = win_ctx
                .tab_manager
                .tabs()
                .iter()
                .find(|t| t.url.starts_with("evergreen://settings"))
            {
                let id = existing.id;
                win_ctx.handle_switch_tab(id, &self.cert_cache);
                return;
            }
        }
        self.handle_create_tab(win_id, Some("evergreen://settings".to_string()));
    }

    fn handle_open_clear_data_settings(&mut self, win_id: WindowId) {
        if let Some(win) = self.windows.get_mut(&win_id) {
            win.close_sidebar();
            if let Some(tab) = win
                .tab_manager
                .tabs()
                .iter()
                .find(|tab| tab.url.starts_with("evergreen://settings"))
            {
                let id = tab.id;
                win.handle_switch_tab(id, &self.cert_cache);
                if let Some(view) = win.tabs.get(&id) {
                    let _ = view
                        .evaluate_script("if(window.__focusClearData)window.__focusClearData()");
                }
                return;
            }
        }
        self.handle_create_tab(win_id, Some("evergreen://settings?clear-data".into()));
    }

    fn close_window(&mut self, win_id: WindowId, event_loop: &ActiveEventLoop) {
        if let Some(win) = self.windows.get(&win_id) {
            let warn = self
                .settings
                .for_profile(&win.profile_id)
                .tabs
                .warn_on_close_tabs;
            if !confirm_close_window_tabs(warn, win.tab_manager.tabs().len()) {
                return;
            }
        }
        if self.windows.contains_key(&win_id) {
            self.save_session(win_id);
            self.windows.remove(&win_id);
            if self.windows.is_empty() {
                event_loop.exit();
            } else {
                self.compact_and_save_open_sessions();
            }
        }
    }

    fn handle_close_tab(
        &mut self,
        win_id: WindowId,
        target_id: TabId,
        event_loop: &ActiveEventLoop,
    ) {
        accelerator::cancel_permissions(win_id, Some(target_id), None);
        let should_close_window = if let Some(win_ctx) = self.windows.get_mut(&win_id) {
            if let Some(wv) = win_ctx.tabs.remove(&target_id) {
                accelerator::hide_webview(&wv);
                drop(wv);
            }
            win_ctx.tab_persisted.remove(&target_id);

            match close_tab_for_window(&mut win_ctx.tab_manager, target_id, win_ctx.private) {
                Some(next_active_id) => {
                    win_ctx.handle_switch_tab(next_active_id, &self.cert_cache);
                    false
                }
                None => true,
            }
        } else {
            false
        };

        self.save_session(win_id);

        if let Some(win) = self.windows.get(&win_id) {
            win.update_sidebar_sync(&self.cert_cache);
        }

        if !should_close_window {
            self.sync_passwords(win_id);
            self.sync_profile_open_tabs(win_id);
        }

        if should_close_window {
            self.windows.remove(&win_id);
            if self.windows.is_empty() {
                event_loop.exit();
            } else {
                self.compact_and_save_open_sessions();
            }
        }
    }

    fn handle_reopen_closed_tab(&mut self, win_id: WindowId) {
        let target_url = self
            .windows
            .get_mut(&win_id)
            .and_then(|win| pop_closed_tab_for_window(&mut win.tab_manager, win.private));
        if let Some(url) = target_url {
            self.handle_create_tab(win_id, Some(url));
            self.save_session(win_id);
        }
    }

    fn handle_open_closed_tab(&mut self, win_id: WindowId, index: usize) {
        let target_url = self
            .windows
            .get_mut(&win_id)
            .filter(|win| !win.private)
            .and_then(|win| win.tab_manager.take_closed_at_newest_index(index));
        if let Some(url) = target_url {
            self.handle_create_tab(win_id, Some(url));
            self.save_session(win_id);
        }
    }

    fn handle_detach_tab(
        &mut self,
        source_win_id: WindowId,
        tab_id: TabId,
        screen_x: Option<f64>,
        screen_y: Option<f64>,
        event_loop: &ActiveEventLoop,
    ) {
        let Some(source) = self.windows.get(&source_win_id) else {
            return;
        };
        let profile_id = source.profile_id.clone();
        let private = source.private;
        let tab_state_opt = if let Some(win_ctx) = self.windows.get_mut(&source_win_id) {
            let extracted = win_ctx.tab_manager.extract_tab(tab_id);
            if let Some(wv) = win_ctx.tabs.remove(&tab_id) {
                accelerator::hide_webview(&wv);
                drop(wv);
            }
            win_ctx.tab_persisted.remove(&tab_id);
            extracted
        } else {
            None
        };

        if let Some(tab_state) = tab_state_opt {
            let mut target_win_id = None;
            if let (Some(sx), Some(sy)) = (screen_x, screen_y) {
                for (w_id, w_ctx) in &self.windows {
                    if *w_id != source_win_id
                        && w_ctx.profile_id == profile_id
                        && w_ctx.private == private
                    {
                        if let (Ok(pos), size) =
                            (w_ctx.window.outer_position(), w_ctx.window.outer_size())
                        {
                            let scale = w_ctx.window.scale_factor();
                            let l_pos = pos.to_logical::<f64>(scale);
                            let l_size = size.to_logical::<f64>(scale);
                            if sx >= l_pos.x
                                && sx <= l_pos.x + l_size.width
                                && sy >= l_pos.y
                                && sy <= l_pos.y + l_size.height
                            {
                                target_win_id = Some(*w_id);
                                break;
                            }
                        }
                    }
                }
            }

            if let Some(target_id) = target_win_id {
                self.handle_create_tab(target_id, Some(tab_state.url));
            } else if let Some(new_win_id) = self.create_profile_window(
                event_loop,
                Some(tab_state.url),
                profile_id,
                private,
                None,
            ) {
                if let (Some(sx), Some(sy)) = (screen_x, screen_y) {
                    if let Some(new_win) = self.windows.get(&new_win_id) {
                        new_win
                            .window
                            .set_outer_position(winit::dpi::LogicalPosition::new(
                                (sx - 200.0).max(0.0),
                                (sy - 20.0).max(0.0),
                            ));
                    }
                }
            }

            let should_close_source = if let Some(win_ctx) = self.windows.get_mut(&source_win_id) {
                if win_ctx.tab_manager.tabs().is_empty() {
                    true
                } else {
                    if let Some(next_active) = win_ctx.tab_manager.active_tab_id() {
                        win_ctx.handle_switch_tab(next_active, &self.cert_cache);
                    }
                    win_ctx.sync_ui_state();
                    false
                }
            } else {
                false
            };

            if should_close_source {
                self.windows.remove(&source_win_id);
                if self.windows.is_empty() {
                    event_loop.exit();
                } else {
                    self.compact_and_save_open_sessions();
                }
            }
        }
    }

    fn handle_navigate(&mut self, win_id: WindowId, raw_url: &str) {
        let search_template = self
            .windows
            .get(&win_id)
            .map(|win| {
                self.settings
                    .for_profile(&win.profile_id)
                    .search_url_template()
            })
            .unwrap_or_else(|| self.settings.search_url_template());
        match normalize_url(raw_url, search_template) {
            Ok(target) if target.starts_with("evergreen://settings") => {
                self.handle_open_settings(win_id)
            }
            Ok(target) if target.starts_with("evergreen://newtab") => {
                self.handle_navigate_to_newtab(win_id, &target);
            }
            Ok(target) => {
                let navigation = self.windows.get(&win_id).and_then(|win| {
                    win.tab_manager
                        .active_tab()
                        .map(|tab| (tab.id, tab.url.clone()))
                });
                if let Some((id, source)) = navigation {
                    self.adblock
                        .borrow_mut()
                        .issue_explicit_navigation_ticket(id.0, &target, &source);
                    if let Some(view) = self.windows.get(&win_id).and_then(|win| win.tabs.get(&id))
                    {
                        // SourceChanged is the authoritative committed URL.
                        if let Err(error) = view.load_url(&target) {
                            eprintln!("Pip navigation failed for {target}: {error}");
                        }
                    }
                }
            }
            Err(error) => eprintln!("Navigation error: {error}"),
        }
    }

    fn handle_navigate_to_newtab(&mut self, win_id: WindowId, target: &str) {
        let Some((tab_id, window, width, height, sidebar_open, chrome_height)) =
            self.windows.get(&win_id).and_then(|win| {
                Some((
                    win.tab_manager.active_tab_id()?,
                    win.window.clone(),
                    win.window_width,
                    win.window_height,
                    win.is_sidebar_open,
                    win.current_chrome_height(),
                ))
            })
        else {
            return;
        };

        let replacement = match self.create_tab_webview(
            win_id,
            tab_id,
            target,
            window.as_ref(),
            width,
            height,
            sidebar_open,
            chrome_height,
            None,
        ) {
            Ok(replacement) => replacement,
            Err(error) => {
                eprintln!("Pip could not create its start page: {error}");
                self.notify_user(win_id, "Không mở được trang chủ Pip.");
                return;
            }
        };
        let _ = replacement.set_visible(false);
        accelerator::cancel_permissions(win_id, Some(tab_id), None);

        if let Some(win) = self.windows.get_mut(&win_id) {
            if let Some(previous) = win.tabs.remove(&tab_id) {
                accelerator::hide_webview(&previous);
                drop(previous);
            }
            win.tabs.insert(tab_id, replacement);
            win.tab_manager.update_url(tab_id, target.to_string());
            win.tab_manager
                .update_title(tab_id, "Tab mới — Pip".to_string());
            win.tab_manager.update_favicon(tab_id, None);
            win.tab_manager.update_history_state(tab_id, false, false);
            if let Some(tab) = win
                .tab_manager
                .tabs_mut()
                .iter_mut()
                .find(|tab| tab.id == tab_id)
            {
                tab.is_loading = false;
                tab.status = evergreen_core::tabs::TabStatus::Active;
                tab.is_audio_playing = false;
                tab.is_muted = false;
            }
            win.close_sidebar();
            if let Some(view) = win.tabs.get(&tab_id) {
                let _ = view.set_visible(true);
            }
            win.update_layout();
            win.sync_ui_state();
            win.update_sidebar_sync(&self.cert_cache);
            win.sync_zoom(win.get_active_zoom());
        }
    }

    #[cfg(target_os = "windows")]
    fn show_native_menu(
        &mut self,
        win_id: WindowId,
        logical_x: f64,
        logical_y: f64,
        event_loop: &ActiveEventLoop,
    ) {
        use windows::core::w;
        use windows::Win32::Foundation::{HWND, POINT};
        use windows::Win32::UI::WindowsAndMessaging::{
            AppendMenuW, CreatePopupMenu, DestroyMenu, TrackPopupMenuEx, MF_SEPARATOR, MF_STRING,
            TPM_RETURNCMD, TPM_RIGHTALIGN, TPM_TOPALIGN,
        };
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

        if let Some(win_ctx) = self.windows.get(&win_id) {
            if let Ok(handle) = win_ctx.window.window_handle() {
                if let RawWindowHandle::Win32(win32_handle) = handle.as_raw() {
                    let hwnd = HWND(win32_handle.hwnd.get() as _);
                    let scale = win_ctx.window.scale_factor();
                    let mut pt = POINT {
                        x: (logical_x * scale) as i32,
                        y: (logical_y * scale) as i32,
                    };
                    unsafe {
                        use windows::Win32::Graphics::Gdi::ClientToScreen;
                        let _ = ClientToScreen(hwnd, &mut pt);

                        if let Ok(hmenu) = CreatePopupMenu() {
                            let _ = AppendMenuW(hmenu, MF_STRING, 1001, w!("New Tab"));
                            let _ = AppendMenuW(hmenu, MF_STRING, 1002, w!("New Window"));
                            let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, None);
                            let _ = AppendMenuW(hmenu, MF_STRING, 1003, w!("DevTools"));
                            let _ = AppendMenuW(hmenu, MF_STRING, 1004, w!("Settings"));

                            let cmd = TrackPopupMenuEx(
                                hmenu,
                                (TPM_RIGHTALIGN | TPM_TOPALIGN | TPM_RETURNCMD).0,
                                pt.x,
                                pt.y,
                                hwnd,
                                None,
                            );
                            let _ = DestroyMenu(hmenu);

                            match cmd.0 {
                                1001 => self.handle_create_tab(win_id, None),
                                1002 => {
                                    self.create_browser_window(event_loop, None);
                                }
                                1003 => {
                                    if let Some(win) = self.windows.get(&win_id) {
                                        if let Some(active) = win.tab_manager.active_tab() {
                                            if let Some(wv) = win.tabs.get(&active.id) {
                                                wv.open_devtools();
                                            }
                                        }
                                    }
                                }
                                1004 => self.handle_open_settings(win_id),
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    fn show_native_menu(
        &mut self,
        _win_id: WindowId,
        _x: f64,
        _y: f64,
        _event_loop: &ActiveEventLoop,
    ) {
    }
}

impl ApplicationHandler<BrowserEvent> for BrowserApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.start_google_dns_health_check();
        if self.windows.is_empty() {
            let saved_profiles = std::mem::take(&mut self.startup_profiles);
            let known_profiles = self
                .settings
                .profiles
                .iter()
                .map(|profile| profile.id.clone())
                .collect::<Vec<_>>();
            let profiles = valid_startup_profiles(&saved_profiles, &known_profiles);
            let mut windows_per_profile = HashMap::<String, usize>::new();
            for profile_id in profiles {
                *windows_per_profile.entry(profile_id).or_default() += 1;
            }
            let profiles = self
                .settings
                .profiles
                .iter()
                .filter_map(|profile| {
                    windows_per_profile
                        .get(&profile.id)
                        .copied()
                        .map(|count| (profile.id.clone(), count))
                })
                .collect::<Vec<_>>();
            self.restoring_startup_windows = true;
            for (profile_id, count) in profiles {
                for slot in 0..count {
                    self.create_profile_window(
                        event_loop,
                        None,
                        profile_id.clone(),
                        false,
                        Some(slot),
                    );
                }
            }
            self.restoring_startup_windows = false;
            self.persist_open_window_profiles();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if now >= self.next_background_tab_check {
            self.suspend_idle_tabs(BrowserApp::now_secs());
            self.next_background_tab_check = now + Duration::from_secs(15);
        }
        if now >= self.next_session_autosave {
            let open_windows = self.windows.keys().copied().collect::<Vec<_>>();
            for win_id in open_windows {
                self.save_session(win_id);
            }
            self.next_session_autosave = now + SESSION_AUTOSAVE_INTERVAL;
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            self.next_session_autosave
                .min(self.next_background_tab_check),
        ));
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: BrowserEvent) {
        match event {
            BrowserEvent::PopupTabRequested {
                window_id,
                url,
                opener_url,
            } => self.handle_create_popup_tab(window_id, url, opener_url),
            BrowserEvent::Ipc(win_id, action) => match action {
                UiToHostMessage::SidebarReady => {
                    if let Some(win) = self.windows.get(&win_id) {
                        win.update_sidebar_sync(&self.cert_cache);
                    }
                }
                UiToHostMessage::ChromeReady => {
                    if let Some(win) = self.windows.get(&win_id) {
                        win.sync_ui_state();
                    }
                    self.sync_google_dns();
                    self.sync_library(win_id);
                    self.sync_settings();
                    self.sync_profiles();
                    self.start_bookmark_sync(win_id, false);
                }
                UiToHostMessage::GetLibrary => self.sync_library(win_id),
                UiToHostMessage::SuggestOmnibox { query } => self.suggest_omnibox(win_id, query),
                UiToHostMessage::ToggleBookmark => {
                    let page = self
                        .windows
                        .get(&win_id)
                        .and_then(|w| w.tab_manager.active_tab())
                        .map(|t| (t.url.clone(), t.title.clone()));
                    if let Some((url, title)) = page {
                        let mut changed = false;
                        self.edit_library(win_id, |library| {
                            changed = library.toggle_bookmark(&url, &title);
                        });
                        if !changed {
                            self.notify_user(
                                win_id,
                                "Không thể thêm dấu trang: địa chỉ không hợp lệ hoặc đã đạt giới hạn 200 mục.",
                            );
                        }
                    }
                }
                UiToHostMessage::RemoveBookmark { url } => {
                    self.edit_library(win_id, |library| library.remove_bookmark(&url))
                }
                UiToHostMessage::CreateBookmarkFolder { name, parent_id } => {
                    let mut created = false;
                    self.edit_library(win_id, |library| {
                        created = library
                            .create_bookmark_folder(&name, parent_id.as_deref())
                            .is_some();
                    });
                    self.notify_library(
                        win_id,
                        if created {
                            "Đã tạo thư mục dấu trang."
                        } else {
                            "Không tạo được thư mục. Hãy kiểm tra tên, thư mục cha và giới hạn 200 thư mục."
                        },
                    );
                }
                UiToHostMessage::DeleteBookmarkFolder { folder_id } => {
                    let mut deleted = false;
                    self.edit_library(win_id, |library| {
                        deleted = library.delete_bookmark_folder(&folder_id);
                    });
                    if !deleted {
                        self.notify_library(win_id, "Không tìm thấy thư mục dấu trang cần xóa.");
                    }
                }
                UiToHostMessage::MoveBookmarkToFolder { url, folder_id } => {
                    let mut moved = false;
                    self.edit_library(win_id, |library| {
                        moved = library.move_bookmark_to_folder(&url, folder_id.as_deref());
                    });
                    if !moved {
                        self.notify_library(
                            win_id,
                            "Không chuyển được dấu trang vào thư mục đã chọn.",
                        );
                    }
                }
                UiToHostMessage::AddHomeShortcut { url, title } => {
                    let mut added = false;
                    self.edit_library(win_id, |library| {
                        added = library.add_home_shortcut(&url, &title);
                    });
                    self.notify_library(
                        win_id,
                        if added {
                            "Đã thêm lối tắt vào trang chủ Pip."
                        } else {
                            "Không thêm được lối tắt. Địa chỉ có thể đã tồn tại hoặc trang chủ đã đủ 12 mục."
                        },
                    );
                }
                UiToHostMessage::RemoveHomeShortcut { url } => {
                    let mut removed = false;
                    self.edit_library(win_id, |library| {
                        removed = library.remove_home_shortcut(&url);
                    });
                    if !removed {
                        self.notify_library(win_id, "Không tìm thấy lối tắt cần xóa.");
                    }
                }
                UiToHostMessage::MoveHomeShortcut { url, direction } => {
                    let mut moved = false;
                    self.edit_library(win_id, |library| {
                        moved = library.move_home_shortcut(&url, direction);
                    });
                    if !moved {
                        self.notify_library(win_id, "Lối tắt không thể di chuyển theo hướng đó.");
                    }
                }
                UiToHostMessage::RemoveHistory { url } => {
                    if self.windows.get(&win_id).is_some_and(|win| !win.private) {
                        self.edit_library(win_id, |library| library.remove_history(&url));
                    }
                }
                UiToHostMessage::RemoveHistoryVisit { url, visited_at_ms } => {
                    if self.windows.get(&win_id).is_some_and(|win| !win.private) {
                        self.edit_library(win_id, |library| {
                            library.remove_history_visit(&url, visited_at_ms)
                        });
                    }
                }
                UiToHostMessage::ImportBookmarks => self.import_bookmarks(win_id),
                UiToHostMessage::ExportBookmarks => self.export_bookmarks(win_id),
                UiToHostMessage::ClearHistory => {
                    if self.windows.get(&win_id).is_some_and(|win| !win.private) {
                        self.edit_library(win_id, |library| library.clear_history());
                    }
                }
                UiToHostMessage::OpenLibrary { view } => {
                    if matches!(view.as_str(), "bookmarks" | "history") {
                        if let Some(win) = self.windows.get_mut(&win_id) {
                            win.close_sidebar();
                        }
                        self.handle_create_tab(win_id, Some(format!("evergreen://newtab?{view}")));
                    }
                }
                UiToHostMessage::ToggleAdblock => {
                    self.user_event(
                        event_loop,
                        BrowserEvent::Shortcut(win_id, "Ctrl+Shift+A".into()),
                    );
                }
                UiToHostMessage::RepairPlayback => {
                    self.user_event(
                        event_loop,
                        BrowserEvent::Shortcut(win_id, "RepairPlayback".into()),
                    );
                }
                UiToHostMessage::CreateTab { url } => {
                    self.handle_create_tab(win_id, url);
                }
                UiToHostMessage::OpenNewWindow => {
                    let private = self.windows.get(&win_id).is_some_and(|w| w.private);
                    self.open_related_window(event_loop, win_id, private);
                }
                UiToHostMessage::WindowMinimize => {
                    if let Some(win) = self.windows.get(&win_id) {
                        win.window.set_minimized(true);
                    }
                }
                UiToHostMessage::WindowToggleMaximize => {
                    if let Some(win) = self.windows.get(&win_id) {
                        win.window.set_maximized(!win.window.is_maximized());
                    }
                }
                UiToHostMessage::WindowClose => {
                    self.close_window(win_id, event_loop);
                }
                UiToHostMessage::WindowDrag => {
                    if let Some(win) = self.windows.get(&win_id) {
                        let _ = win.window.drag_window();
                    }
                }
                UiToHostMessage::OpenPrivateWindow => {
                    self.open_related_window(event_loop, win_id, true)
                }
                UiToHostMessage::OpenProfile { id } => {
                    if self.settings.profiles.iter().any(|p| p.id == id) {
                        let restore_slot = (!self
                            .windows
                            .values()
                            .any(|window| !window.private && window.profile_id == id))
                        .then_some(0);
                        self.create_profile_window(event_loop, None, id, false, restore_slot);
                    }
                }
                UiToHostMessage::CreateProfile { name } => {
                    if self.settings.profiles.len() < 20
                        && evergreen_core::settings::valid_profile_name(&name)
                    {
                        if let Ok(guid) = unsafe { windows::Win32::System::Com::CoCreateGuid() } {
                            let id = format!("Pip{guid:?}").replace('-', "");
                            let mut settings = self.settings.clone();
                            settings
                                .profiles
                                .push(evergreen_core::settings::BrowserProfile {
                                    id: id.clone(),
                                    name: name.trim().into(),
                                    autofill: true,
                                    password_autosave: true,
                                    sync_credentials_protected: None,
                                    sync_history: false,
                                    sync_open_tabs: false,
                                    sync_settings: false,
                                    sync_passwords: false,
                                    sync_device_id: None,
                                    sync_settings_updated_at_ms: 0,
                                    sync_settings_fingerprint: None,
                                    sync_settings_values: None,
                                    download_settings: None,
                                });
                            if settings.save_to_path(&get_settings_path()).is_ok() {
                                self.settings = settings;
                                self.sync_profiles();
                                self.create_profile_window(event_loop, None, id, false, Some(0));
                            }
                        }
                    }
                }
                UiToHostMessage::RenameProfile { id, name } => {
                    if evergreen_core::settings::valid_profile_name(&name) {
                        let mut settings = self.settings.clone();
                        if let Some(profile) = settings.profiles.iter_mut().find(|p| p.id == id) {
                            profile.name = name.trim().into();
                            if settings.save_to_path(&get_settings_path()).is_ok() {
                                self.settings = settings;
                                self.sync_profiles();
                            }
                        }
                    }
                }
                UiToHostMessage::DeleteProfile { id } => {
                    if self
                        .windows
                        .get(&win_id)
                        .is_some_and(|window| window.private)
                    {
                        self.notify_settings(win_id, "Không thể xóa hồ sơ trong cửa sổ ẩn danh.");
                        return;
                    }
                    if !evergreen_core::settings::valid_profile_id(&id) || id == "Default" {
                        self.notify_settings(win_id, "Không thể xóa hồ sơ mặc định.");
                        return;
                    }
                    let Some(profile) = self
                        .settings
                        .profiles
                        .iter()
                        .find(|profile| profile.id == id)
                    else {
                        self.notify_settings(win_id, "Không tìm thấy hồ sơ cần xóa.");
                        return;
                    };
                    if profile.sync_credentials_protected.is_some() {
                        self.notify_settings(
                            win_id,
                            "Hãy tắt và xóa Pip Sync của hồ sơ trước khi xóa hồ sơ.",
                        );
                        return;
                    }
                    self.compact_open_session_slots(Some(&id));
                    let profile_view = self
                        .windows
                        .values()
                        .find(|window| !window.private && window.profile_id == id)
                        .and_then(|window| window.tabs.values().next());
                    let Some(profile_view) = profile_view else {
                        self.notify_settings(
                            win_id,
                            "Hãy mở hồ sơ này trong cửa sổ Pip bình thường trước khi xóa. Cửa sổ ẩn danh không thể dùng để xóa dữ liệu hồ sơ.",
                        );
                        return;
                    };

                    // Save the restart list before destructive work so other open profiles
                    // return after the WebView2 profile has been deleted and Pip restarts.
                    let restart_profiles: Vec<String> = self
                        .windows
                        .values()
                        .filter(|window| !window.private && window.profile_id != id)
                        .map(|window| window.profile_id.clone())
                        .collect();
                    for window_id in self.windows.keys().copied().collect::<Vec<_>>() {
                        if self
                            .windows
                            .get(&window_id)
                            .is_some_and(|window| window.profile_id != id)
                        {
                            self.save_session(window_id);
                        }
                    }
                    let restart_list_saved = serde_json::to_vec(&restart_profiles)
                        .map_err(std::io::Error::other)
                        .and_then(|data| std::fs::write(startup_profiles_path(), data));
                    if restart_list_saved.is_err() {
                        self.notify_settings(
                            win_id,
                            "Không thể chuẩn bị khởi động lại Pip; hồ sơ chưa bị xóa.",
                        );
                        return;
                    }

                    let previous_settings = self.settings.clone();
                    let mut settings = previous_settings.clone();
                    settings.profiles.retain(|profile| profile.id != id);
                    if settings.save_to_path(&get_settings_path()).is_err() {
                        let _ = std::fs::remove_file(startup_profiles_path());
                        self.notify_settings(win_id, "Không thể lưu thay đổi; hồ sơ chưa bị xóa.");
                        return;
                    }

                    use webview2_com::Microsoft::Web::WebView2::Win32::{
                        ICoreWebView2Profile8, ICoreWebView2_13,
                    };
                    use windows::core::Interface;
                    use wry::WebViewExtWindows;
                    let deletion = unsafe {
                        profile_view
                            .controller()
                            .CoreWebView2()
                            .and_then(|core| core.cast::<ICoreWebView2_13>())
                            .and_then(|core| core.Profile())
                            .and_then(|profile| profile.cast::<ICoreWebView2Profile8>())
                            .and_then(|profile| profile.Delete())
                    };
                    if deletion.is_err() {
                        let _ = previous_settings.save_to_path(&get_settings_path());
                        let _ = std::fs::remove_file(startup_profiles_path());
                        self.notify_settings(win_id, "WebView2 không thể xóa hồ sơ này. Hãy cập nhật WebView2 Runtime rồi thử lại.");
                        return;
                    }

                    self.settings = settings;
                    let data_dir = get_data_directory();
                    let profiles_directory = data_dir.join("profiles");
                    let vault_cleanup = passwords::remove_profile_vault(&profiles_directory, &id);
                    let site_zoom_cleanup = match std::fs::remove_file(
                        profiles_directory.join(&id).join("site-zoom.json"),
                    ) {
                        Ok(()) => Ok(()),
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                        Err(_) => Err("Không thể xóa mức thu phóng website của hồ sơ.".to_owned()),
                    };
                    if let Err(error) = vault_cleanup.and(site_zoom_cleanup) {
                        eprintln!("Pip profile sidecar cleanup failed for {id}: {error}");
                        self.notify_settings(
                            win_id,
                            &format!("WebView2 đã xóa hồ sơ, nhưng {error} Pip sẽ khởi động lại."),
                        );
                    }
                    let _ =
                        std::fs::remove_file(data_dir.join("sessions").join(format!("{id}.json")));
                    let _ = std::fs::remove_file(
                        data_dir.join("sessions").join(format!("{id}.closed.json")),
                    );
                    let _ = std::fs::remove_file(
                        data_dir.join("profiles").join(&id).join("library.json"),
                    );
                    let _ = std::fs::remove_file(
                        data_dir.join("profiles").join(&id).join("downloads.json"),
                    );
                    self.restart_requested = true;
                    self.windows.clear();
                    event_loop.exit();
                }
                UiToHostMessage::TogglePinTab => {
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        if let Some(id) = win.tab_manager.active_tab_id() {
                            win.tab_manager.toggle_pin(id);
                            win.sync_ui_state();
                        }
                    }
                }
                UiToHostMessage::ToggleKeepAwakeTab => {
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        if let Some(id) = win.tab_manager.active_tab_id() {
                            if let Some(enabled) = win.tab_manager.toggle_keep_awake(id) {
                                win.sync_ui_state();
                                win.update_sidebar_sync(&self.cert_cache);
                                let message = if enabled {
                                    "Tab này sẽ không tự ngủ khi Pip chạy nền."
                                } else {
                                    "Tab này đã bật lại chế độ tự ngủ."
                                };
                                self.notify_settings(win_id, message);
                            }
                        }
                    }
                }
                UiToHostMessage::SetTabGroup { tab_id, name } => {
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        if !win.tab_manager.set_group(tab_id, &name) {
                            eprintln!("Pip ignored an invalid tab group name");
                        }
                        win.sync_ui_state();
                    }
                    self.save_session(win_id);
                    self.sync_profile_open_tabs(win_id);
                }
                UiToHostMessage::RestoreSession => self.restore_session(win_id),
                UiToHostMessage::ReopenClosedTab => self.handle_reopen_closed_tab(win_id),
                UiToHostMessage::OpenClosedTab { index } => {
                    self.handle_open_closed_tab(win_id, index)
                }
                UiToHostMessage::TranslatePage => {
                    if let Some(win) = self.windows.get(&win_id) {
                        let Some(tab_id) = win.tab_manager.active_tab_id() else {
                            return;
                        };
                        let Some(tab) = win.tab_manager.get_tab_by_id(tab_id) else {
                            return;
                        };
                        if !(tab.url.starts_with("https://") || tab.url.starts_with("http://")) {
                            return;
                        }
                        if let Some(view) = win.tabs.get(&tab_id) {
                            let _ = view.evaluate_script(translate::PAGE_TRANSLATION_SCRIPT);
                        }
                    }
                }
                UiToHostMessage::EnableBookmarkSync => {
                    if self.windows.get(&win_id).is_none_or(|win| win.private) {
                        return;
                    }
                    let profile_id = self.windows.get(&win_id).unwrap().profile_id.clone();
                    if self.syncing_profiles.contains(&profile_id)
                        || self.settings.profiles.iter().any(|profile| {
                            profile.id == profile_id && profile.sync_credentials_protected.is_some()
                        })
                    {
                        self.notify_settings(
                            win_id,
                            "Pip Sync đã được bật hoặc đang thiết lập cho hồ sơ này.",
                        );
                        return;
                    }
                    let credentials = sync::SyncCredentials::generate();
                    let protected = match sync::protect_credentials(&credentials) {
                        Ok(value) => value,
                        Err(error) => {
                            self.notify_settings(win_id, &error.to_string());
                            return;
                        }
                    };
                    let mut settings = self.settings.clone();
                    if let Some(profile) = settings
                        .profiles
                        .iter_mut()
                        .find(|profile| profile.id == profile_id)
                    {
                        profile.sync_credentials_protected = Some(protected);
                    }
                    if settings.save_to_path(&get_settings_path()).is_err() {
                        self.notify_settings(win_id, "Không thể lưu cài đặt đồng bộ.");
                        return;
                    }
                    self.settings = settings;
                    self.sync_profiles();
                    let code = serde_json::to_string(&credentials.recovery_code()).unwrap();
                    if let Some(view) = self.windows.get(&win_id).and_then(|win| {
                        win.tab_manager
                            .active_tab_id()
                            .and_then(|id| {
                                win.tab_manager
                                    .get_tab_by_id(id)
                                    .filter(|tab| tab.url == "evergreen://settings")
                            })
                            .and_then(|tab| win.tabs.get(&tab.id))
                    }) {
                        let _ = view.evaluate_script(&format!("window.showStatus('Đã tạo mã Pip Sync. Hãy cất mã khôi phục ở nơi riêng; người có mã có thể truy cập dấu trang đã đồng bộ.');prompt('Lưu mã khôi phục ở nơi an toàn. Mã chỉ hiện lần này:',{code});"));
                    }
                    self.start_bookmark_sync(win_id, false);
                }
                UiToHostMessage::ShowBookmarkSyncCode => {
                    let Some(win) = self.windows.get(&win_id).filter(|win| !win.private) else {
                        return;
                    };
                    let Some(protected) = self
                        .settings
                        .profiles
                        .iter()
                        .find(|profile| profile.id == win.profile_id)
                        .and_then(|profile| profile.sync_credentials_protected.as_deref())
                    else {
                        self.notify_settings(win_id, "Hồ sơ này chưa thiết lập Pip Sync.");
                        return;
                    };
                    let credentials = match sync::unprotect_credentials(protected) {
                        Ok(value) => value,
                        Err(error) => {
                            self.notify_settings(win_id, &error.to_string());
                            return;
                        }
                    };
                    let code = serde_json::to_string(&credentials.recovery_code()).unwrap();
                    if let Some(view) = self.windows.get(&win_id).and_then(|win| {
                        win.tab_manager
                            .active_tab_id()
                            .and_then(|id| {
                                win.tab_manager
                                    .get_tab_by_id(id)
                                    .filter(|tab| tab.url == "evergreen://settings")
                            })
                            .and_then(|tab| win.tabs.get(&tab.id))
                    }) {
                        let _ = view.evaluate_script(&format!("prompt('Mã khôi phục Pip Sync. Bất kỳ ai có mã này đều có thể truy cập dấu trang đã đồng bộ:',{code});"));
                    }
                }
                UiToHostMessage::DisableBookmarkSync => {
                    let Some(win) = self.windows.get(&win_id).filter(|win| !win.private) else {
                        return;
                    };
                    let profile_id = win.profile_id.clone();
                    if self.syncing_profiles.contains(&profile_id) {
                        self.notify_settings(
                            win_id,
                            "Pip Sync đang chạy. Hãy thử tắt và xóa lại sau khi đồng bộ xong.",
                        );
                        return;
                    }
                    let Some(protected) = self
                        .settings
                        .profiles
                        .iter()
                        .find(|profile| profile.id == profile_id)
                        .and_then(|profile| profile.sync_credentials_protected.as_deref())
                    else {
                        self.notify_settings(win_id, "Hồ sơ này chưa bật Pip Sync.");
                        return;
                    };
                    let credentials = match sync::unprotect_credentials(protected) {
                        Ok(value) => value,
                        Err(error) => {
                            self.notify_settings(win_id, &error.to_string());
                            return;
                        }
                    };
                    let vault_id = credentials.vault_id.clone();
                    let dns_proxy_port = self.doh_proxy.as_ref().map(doh_proxy::DohProxy::port);
                    let google_dns_enabled = self.google_dns_applied;
                    let proxy = self.proxy.clone();
                    let event_profile_id = profile_id.clone();
                    self.syncing_profiles.insert(profile_id.clone());
                    if std::thread::Builder::new()
                        .name("pip-bookmark-sync-delete".into())
                        .spawn(move || {
                            let result = sync::delete_vault_with_google_dns(
                                &credentials,
                                google_dns_enabled,
                                dns_proxy_port,
                            );
                            let _ = proxy.send_event(BrowserEvent::SyncDisabled(
                                win_id,
                                event_profile_id,
                                vault_id,
                                result,
                            ));
                        })
                        .is_ok()
                    {
                        self.notify_settings(win_id, "Đang xóa bản đồng bộ trên máy chủ…");
                    } else {
                        self.syncing_profiles.remove(&profile_id);
                        self.notify_settings(win_id, "Không khởi chạy được yêu cầu xóa.");
                    }
                }
                UiToHostMessage::ImportBookmarkSync { recovery_code } => {
                    if self.windows.get(&win_id).is_none_or(|win| win.private) {
                        return;
                    }
                    let profile_id = self.windows.get(&win_id).unwrap().profile_id.clone();
                    if self.syncing_profiles.contains(&profile_id) {
                        self.notify_settings(
                            win_id,
                            "Hãy đợi đồng bộ đang chạy xong trước khi thay mã khôi phục.",
                        );
                        return;
                    }
                    let credentials =
                        match sync::SyncCredentials::from_recovery_code(&recovery_code) {
                            Ok(value) => value,
                            Err(error) => {
                                self.notify_settings(win_id, &error.to_string());
                                return;
                            }
                        };
                    let protected = match sync::protect_credentials(&credentials) {
                        Ok(value) => value,
                        Err(error) => {
                            self.notify_settings(win_id, &error.to_string());
                            return;
                        }
                    };
                    let mut settings = self.settings.clone();
                    if let Some(profile) = settings
                        .profiles
                        .iter_mut()
                        .find(|profile| profile.id == profile_id)
                    {
                        profile.sync_credentials_protected = Some(protected);
                    }
                    if settings.save_to_path(&get_settings_path()).is_ok() {
                        self.settings = settings;
                        self.sync_profiles();
                        self.notify_settings(
                            win_id,
                            "Đã lưu mã khôi phục. Pip đang đồng bộ dấu trang và sẽ tự cập nhật khi có thay đổi.",
                        );
                        self.start_bookmark_sync(win_id, false);
                    } else {
                        self.notify_settings(win_id, "Không thể lưu mã đồng bộ trên thiết bị.");
                    }
                }
                UiToHostMessage::SyncBookmarks => {
                    self.start_bookmark_sync(win_id, true);
                }
                UiToHostMessage::SetSyncCategories {
                    history,
                    open_tabs,
                    settings: sync_settings,
                    passwords: sync_passwords,
                } => {
                    let profile_id = self
                        .windows
                        .get(&win_id)
                        .and_then(|window| (!window.private).then(|| window.profile_id.clone()));
                    let Some(profile_id) = profile_id else {
                        return;
                    };
                    let mut settings = self.settings.clone();
                    let Some(profile) = settings
                        .profiles
                        .iter_mut()
                        .find(|profile| profile.id == profile_id)
                    else {
                        return;
                    };
                    if profile.sync_credentials_protected.is_none() {
                        self.notify_settings(win_id, "Tạo mã Pip Sync trước khi chọn dữ liệu.");
                        return;
                    }
                    let changed = profile.sync_history != history
                        || profile.sync_open_tabs != open_tabs
                        || profile.sync_settings != sync_settings
                        || profile.sync_passwords != sync_passwords;
                    profile.sync_history = history;
                    profile.sync_open_tabs = open_tabs;
                    profile.sync_settings = sync_settings;
                    profile.sync_passwords = sync_passwords;
                    if settings.save_to_path(&get_settings_path()).is_ok() {
                        self.settings = settings;
                        if changed {
                            self.start_bookmark_sync(win_id, false);
                        }
                    } else {
                        self.notify_settings(win_id, "Không lưu được lựa chọn dữ liệu Sync.");
                    }
                }
                UiToHostMessage::GetSitePermissions => self.request_site_permissions(win_id),
                UiToHostMessage::ResetSitePermission { origin, kind } => {
                    self.reset_site_permission(win_id, origin, kind)
                }
                UiToHostMessage::SetAutofill { enabled } => {
                    if let Some(win) = self.windows.get(&win_id) {
                        if !win.private {
                            let id = win.profile_id.clone();
                            let mut settings = self.settings.clone();
                            if let Some(profile) = settings.profiles.iter_mut().find(|p| p.id == id)
                            {
                                profile.autofill = enabled;
                                if settings.save_to_path(&get_settings_path()).is_ok() {
                                    self.settings = settings;
                                    for context in
                                        self.windows.values().filter(|w| w.profile_id == id)
                                    {
                                        for view in context.tabs.values() {
                                            self.configure_autofill(view, &id, context.private);
                                        }
                                    }
                                    self.sync_profiles();
                                }
                            }
                        }
                    }
                }
                UiToHostMessage::SetPasswordAutosave { enabled } => {
                    if let Some(win) = self.windows.get(&win_id) {
                        if !win.private {
                            let id = win.profile_id.clone();
                            let mut settings = self.settings.clone();
                            if let Some(profile) = settings.profiles.iter_mut().find(|p| p.id == id)
                            {
                                profile.password_autosave = enabled;
                                if settings.save_to_path(&get_settings_path()).is_ok() {
                                    self.settings = settings;
                                    for context in
                                        self.windows.values().filter(|w| w.profile_id == id)
                                    {
                                        for view in context.tabs.values() {
                                            self.configure_autofill(view, &id, context.private);
                                        }
                                    }
                                    self.sync_profiles();
                                }
                            }
                        }
                    }
                }
                UiToHostMessage::GetBrowserExtensions => self.request_browser_extensions(win_id),
                UiToHostMessage::SelectBrowserExtensionFolder => {
                    let window = self
                        .windows
                        .get(&win_id)
                        .filter(|window| !window.private)
                        .map(|window| window.window.clone());
                    if let Some(window) = window {
                        match choose_browser_extension_folder(&window) {
                            Ok(path) => self.install_browser_extension(win_id, path),
                            Err(error) if error.code().0 as u32 == 1223 => {}
                            Err(error) => self.notify_settings(
                                win_id,
                                &format!("Không mở được hộp chọn thư mục tiện ích: {error}"),
                            ),
                        }
                    }
                }
                UiToHostMessage::SetBrowserExtensionEnabled { id, enabled } => {
                    self.change_browser_extension(win_id, id, Some(enabled));
                }
                UiToHostMessage::RemoveBrowserExtension { id } => {
                    self.change_browser_extension(win_id, id, None);
                }
                UiToHostMessage::GetSavedPasswords => self.sync_passwords(win_id),
                UiToHostMessage::SavePasswordForTab {
                    tab_id,
                    username,
                    password,
                } => {
                    let Some(win) = self.windows.get(&win_id).filter(|win| !win.private) else {
                        return;
                    };
                    let Some(url) = win
                        .tab_manager
                        .get_tab_by_id(TabId(tab_id))
                        .map(|tab| tab.url.clone())
                    else {
                        self.notify_settings(win_id, "Tab đã chọn không còn mở.");
                        return;
                    };
                    let Some(origin) = passwords::normalize_origin(&url) else {
                        self.notify_settings(
                            win_id,
                            "Chỉ có thể lưu mật khẩu cho tab HTTP hoặc HTTPS đang mở.",
                        );
                        return;
                    };
                    let Some(path) = self.passwords_path(win_id) else {
                        return;
                    };
                    let mut entries = match passwords::load(&path) {
                        Ok(entries) => entries,
                        Err(message) => {
                            self.notify_settings(win_id, &message);
                            return;
                        }
                    };
                    if let Err(message) =
                        passwords::upsert(&mut entries, &origin, &username, &password)
                            .and_then(|_| passwords::save(&path, &entries))
                    {
                        self.notify_settings(win_id, &message);
                        return;
                    }
                    self.sync_passwords(win_id);
                    self.sync_password_vault_if_enabled(win_id);
                    self.notify_settings(
                        win_id,
                        &format!(
                            "Đã lưu mật khẩu cho {origin}, được mã hóa bằng tài khoản Windows này."
                        ),
                    );
                }
                UiToHostMessage::DeletePassword { id } => {
                    if self.windows.get(&win_id).is_none_or(|win| win.private) {
                        return;
                    }
                    let Some(path) = self.passwords_path(win_id) else {
                        return;
                    };
                    let mut entries = match passwords::load(&path) {
                        Ok(entries) => entries,
                        Err(message) => {
                            self.notify_settings(win_id, &message);
                            return;
                        }
                    };
                    let old_len = entries.len();
                    entries.retain(|entry| entry.id != id);
                    if old_len == entries.len() {
                        self.notify_settings(win_id, "Mật khẩu này không còn trong kho.");
                        self.sync_passwords(win_id);
                        return;
                    }
                    if let Err(message) = passwords::save(&path, &entries) {
                        self.notify_settings(win_id, &message);
                        return;
                    }
                    self.sync_passwords(win_id);
                    self.sync_password_vault_if_enabled(win_id);
                    self.notify_settings(win_id, "Đã xóa mật khẩu khỏi hồ sơ.");
                }
                UiToHostMessage::RevealPassword { id } => {
                    let Some(win) = self.windows.get(&win_id).filter(|win| !win.private) else {
                        return;
                    };
                    let Some(path) = self.passwords_path(win_id) else {
                        return;
                    };
                    let entries = match passwords::load(&path) {
                        Ok(entries) => entries,
                        Err(message) => {
                            self.notify_settings(win_id, &message);
                            return;
                        }
                    };
                    let Some(entry) = passwords::find(&entries, id) else {
                        self.notify_settings(win_id, "Mật khẩu này không còn trong kho.");
                        return;
                    };
                    let Some(view) = win.tab_manager.active_tab_id().and_then(|tab_id| {
                        win.tab_manager
                            .get_tab_by_id(tab_id)
                            .filter(|tab| tab.url.starts_with("evergreen://settings"))
                            .and_then(|_| win.tabs.get(&tab_id))
                    }) else {
                        return;
                    };
                    let password = serde_json::to_string(&entry.password)
                        .unwrap_or_else(|_| "\"\"".into())
                        .replace('<', "\\u003c");
                    let _ = view.evaluate_script(&format!(
                        "if(window.__showSavedPassword)window.__showSavedPassword({id},{password})"
                    ));
                }
                UiToHostMessage::FillPassword { id, tab_id } => {
                    let Some(win) = self.windows.get(&win_id).filter(|win| !win.private) else {
                        return;
                    };
                    let tab_id = TabId(tab_id);
                    let Some(url) = win
                        .tab_manager
                        .get_tab_by_id(tab_id)
                        .map(|tab| tab.url.clone())
                    else {
                        self.notify_settings(win_id, "Tab đích không còn mở.");
                        return;
                    };
                    let Some(origin) = passwords::normalize_origin(&url) else {
                        self.notify_settings(
                            win_id,
                            "Hãy mở đúng website đã lưu trước khi điền mật khẩu.",
                        );
                        return;
                    };
                    let Some(path) = self.passwords_path(win_id) else {
                        return;
                    };
                    let entries = match passwords::load(&path) {
                        Ok(entries) => entries,
                        Err(message) => {
                            self.notify_settings(win_id, &message);
                            return;
                        }
                    };
                    let Some(entry) = passwords::find(&entries, id) else {
                        self.notify_settings(win_id, "Mật khẩu này không còn trong kho.");
                        return;
                    };
                    if entry.origin != origin {
                        self.notify_settings(
                            win_id,
                            "Pip chỉ điền mật khẩu khi hostname, giao thức và cổng khớp chính xác.",
                        );
                        return;
                    }
                    let Some(view) = win.tabs.get(&tab_id) else {
                        return;
                    };
                    let credentials = serde_json::to_string(&serde_json::json!({
                        "origin": entry.origin,
                        "username": entry.username,
                        "password": entry.password,
                    }))
                    .unwrap_or_else(|_| "{}".into())
                    .replace('<', "\\u003c");
                    let script = format!(
                        r#"(()=>{{const c={credentials};if(location.origin!==c.origin)return false;const visible=e=>!!(e.offsetWidth||e.offsetHeight||e.getClientRects().length);const p=[...document.querySelectorAll('input[type=password]')].find(visible);if(!p)return false;const root=p.form||document;const fields=[...root.querySelectorAll('input')].filter(e=>visible(e)&&!['password','hidden','checkbox','radio','submit','button','file'].includes((e.type||'').toLowerCase()));const user=fields.find(e=>e.type==='email')||fields.find(e=>/user|email|login|account/i.test([e.name,e.id,e.autocomplete].join(' ')))||fields.find(e=>e.type==='text');const set=(e,v)=>{{const d=Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value');d.set.call(e,v);e.dispatchEvent(new Event('input',{{bubbles:true}}));e.dispatchEvent(new Event('change',{{bubbles:true}}))}};if(user)set(user,c.username);set(p,c.password);p.focus();return true}})()"#
                    );
                    let proxy = self.proxy.clone();
                    if let Err(error) = view.evaluate_script_with_callback(&script, move |result| {
                        let _ = proxy.send_event(BrowserEvent::PasswordFillDone(
                            win_id,
                            result.trim_matches('"') == "true",
                        ));
                    }) {
                        eprintln!("Pip password fill: {error}");
                        self.notify_settings(win_id, "Không thể điền mật khẩu vào trang này.");
                    }
                }
                action @ (UiToHostMessage::ClearSavedPasswords
                | UiToHostMessage::ClearCache
                | UiToHostMessage::ClearSiteData) => {
                    let kind = match action {
                        UiToHostMessage::ClearCache => BrowsingDataClearKind::Cache,
                        UiToHostMessage::ClearSiteData => BrowsingDataClearKind::SiteData,
                        _ => BrowsingDataClearKind::Passwords,
                    };
                    use webview2_com::Microsoft::Web::WebView2::Win32::*;
                    use windows::core::Interface;
                    use wry::WebViewExtWindows;
                    if let Some(win) = self.windows.get(&win_id) {
                        if win.private {
                            self.notify_settings(
                                win_id,
                                "Trong ẩn danh, cookie và dữ liệu website tự được xóa khi đóng toàn bộ cửa sổ ẩn danh.",
                            );
                            return;
                        }
                        if let Some(view) = win
                            .tab_manager
                            .active_tab_id()
                            .and_then(|id| win.tabs.get(&id))
                        {
                            unsafe {
                                if let Ok(profile) = view
                                    .controller()
                                    .CoreWebView2()
                                    .and_then(|c| c.cast::<ICoreWebView2_13>())
                                    .and_then(|c| c.Profile())
                                    .and_then(|p| p.cast::<ICoreWebView2Profile2>())
                                {
                                    let proxy = self.proxy.clone();
                                    let callback =
                                        webview2_com::ClearBrowsingDataCompletedHandler::create(
                                            Box::new(move |result| {
                                                let _ = proxy.send_event(
                                                    BrowserEvent::BrowsingDataClearDone(
                                                        win_id,
                                                        kind,
                                                        result.is_ok(),
                                                    ),
                                                );
                                                Ok(())
                                            }),
                                        );
                                    let data_kind = match kind {
                                        BrowsingDataClearKind::Cache => {
                                            COREWEBVIEW2_BROWSING_DATA_KINDS_DISK_CACHE
                                        }
                                        BrowsingDataClearKind::Passwords => {
                                            COREWEBVIEW2_BROWSING_DATA_KINDS_PASSWORD_AUTOSAVE
                                        }
                                        BrowsingDataClearKind::SiteData => {
                                            COREWEBVIEW2_BROWSING_DATA_KINDS_ALL_SITE
                                        }
                                        BrowsingDataClearKind::Selected { .. } => {
                                            COREWEBVIEW2_BROWSING_DATA_KINDS(0)
                                        }
                                    };
                                    if profile.ClearBrowsingData(data_kind, &callback).is_err() {
                                        let _ = self.proxy.send_event(
                                            BrowserEvent::BrowsingDataClearDone(
                                                win_id, kind, false,
                                            ),
                                        );
                                    }
                                } else {
                                    self.notify_settings(
                                        win_id,
                                        "WebView2 hiện tại chưa hỗ trợ thao tác xóa dữ liệu này.",
                                    );
                                }
                            }
                        }
                    }
                }
                UiToHostMessage::ClearSelectedBrowsingData {
                    history,
                    site_data,
                    cache,
                    passwords,
                    autofill,
                    download_history,
                } => {
                    if !(history || site_data || cache || passwords || autofill || download_history)
                    {
                        self.notify_settings(win_id, "Hãy chọn ít nhất một loại dữ liệu cần xóa.");
                        return;
                    }
                    use webview2_com::Microsoft::Web::WebView2::Win32::*;
                    use windows::core::Interface;
                    use wry::WebViewExtWindows;
                    let Some(win) = self.windows.get(&win_id) else {
                        return;
                    };
                    if win.private {
                        self.notify_settings(
                            win_id,
                            "Dữ liệu ẩn danh được xóa khi đóng toàn bộ cửa sổ ẩn danh.",
                        );
                        return;
                    }
                    let Some(view) = win
                        .tab_manager
                        .active_tab_id()
                        .and_then(|id| win.tabs.get(&id))
                    else {
                        self.notify_settings(win_id, "Không tìm thấy hồ sơ đang hoạt động.");
                        return;
                    };
                    let profile = unsafe {
                        view.controller()
                            .CoreWebView2()
                            .and_then(|core| core.cast::<ICoreWebView2_13>())
                            .and_then(|core| core.Profile())
                            .and_then(|profile| profile.cast::<ICoreWebView2Profile2>())
                    };
                    let Ok(profile) = profile else {
                        self.notify_settings(
                            win_id,
                            "WebView2 hiện tại chưa hỗ trợ xóa dữ liệu duyệt web.",
                        );
                        return;
                    };

                    let mut data_kinds = COREWEBVIEW2_BROWSING_DATA_KINDS(0);
                    if history {
                        data_kinds |= COREWEBVIEW2_BROWSING_DATA_KINDS_BROWSING_HISTORY;
                    }
                    if site_data {
                        data_kinds |= COREWEBVIEW2_BROWSING_DATA_KINDS_ALL_SITE;
                    }
                    if cache {
                        data_kinds |= COREWEBVIEW2_BROWSING_DATA_KINDS_DISK_CACHE;
                    }
                    if passwords {
                        data_kinds |= COREWEBVIEW2_BROWSING_DATA_KINDS_PASSWORD_AUTOSAVE;
                    }
                    if autofill {
                        data_kinds |= COREWEBVIEW2_BROWSING_DATA_KINDS_GENERAL_AUTOFILL;
                    }
                    if download_history {
                        data_kinds |= COREWEBVIEW2_BROWSING_DATA_KINDS_DOWNLOAD_HISTORY;
                    }

                    let clear_kind = BrowsingDataClearKind::Selected {
                        clear_pip_history: history,
                        clear_pip_download_history: download_history,
                        clear_pip_passwords: passwords,
                    };
                    let proxy = self.proxy.clone();
                    let callback = webview2_com::ClearBrowsingDataCompletedHandler::create(
                        Box::new(move |result| {
                            let _ = proxy.send_event(BrowserEvent::BrowsingDataClearDone(
                                win_id,
                                clear_kind,
                                result.is_ok(),
                            ));
                            Ok(())
                        }),
                    );
                    if unsafe { profile.ClearBrowsingData(data_kinds, &callback) }.is_err() {
                        let _ = self.proxy.send_event(BrowserEvent::BrowsingDataClearDone(
                            win_id, clear_kind, false,
                        ));
                    } else {
                        self.notify_settings(win_id, "Đang xóa dữ liệu đã chọn…");
                    }
                }
                UiToHostMessage::SetGoogleDns { enabled } => {
                    let mut settings = self.settings.clone();
                    settings.google_dns = enabled;
                    if settings.save_to_path(&get_settings_path()).is_ok() {
                        self.settings = settings;
                        self.sync_google_dns();
                        self.notify_settings(
                            win_id,
                            if enabled {
                                "Google DNS đã bật trong cài đặt. Khởi động lại Pip để áp dụng."
                            } else {
                                "Google DNS đã tắt trong cài đặt. Khởi động lại Pip để áp dụng."
                            },
                        );
                    } else {
                        self.notify_settings(win_id, "Không thể lưu cài đặt Google DNS.");
                        self.sync_google_dns();
                    }
                }
                UiToHostMessage::TestGoogleDns => {
                    let Some(proxy_port) = self.doh_proxy.as_ref().map(doh_proxy::DohProxy::port)
                    else {
                        self.notify_settings(
                            win_id,
                            "Chưa kiểm tra được tuyến DNS của Pip vì Google DNS chưa chạy. Bật G DNS rồi đóng và mở lại Pip để áp dụng, sau đó thử lại.",
                        );
                        return;
                    };
                    self.notify_settings(
                        win_id,
                        "Đang thử tải Google qua proxy DNS đang cấu hình cho các tab Pip...",
                    );
                    let proxy = self.proxy.clone();
                    let _ = std::thread::Builder::new()
                        .name("pip-google-dns-check".to_string())
                        .spawn(move || {
                            let result = doh_proxy::test_google_doh_through_proxy(proxy_port);
                            let _ =
                                proxy.send_event(BrowserEvent::GoogleDnsCheckDone(win_id, result));
                        });
                }
                UiToHostMessage::RestartBrowser => {
                    self.compact_open_session_slots(None);
                    let profiles: Vec<String> = self
                        .windows
                        .values()
                        .filter(|win| !win.private)
                        .map(|win| win.profile_id.clone())
                        .collect();
                    for open_win_id in self.windows.keys().copied().collect::<Vec<_>>() {
                        self.save_session(open_win_id);
                    }
                    let saved = serde_json::to_vec(&profiles)
                        .map_err(std::io::Error::other)
                        .and_then(|data| std::fs::write(startup_profiles_path(), data));
                    match saved {
                        Ok(()) => {
                            self.restart_requested = true;
                            self.windows.clear();
                            event_loop.exit();
                        }
                        Err(error) => {
                            eprintln!("Could not save Pip windows before restart: {error}");
                            self.notify_settings(
                                win_id,
                                "Không thể lưu các tab trước khi khởi động lại Pip.",
                            );
                        }
                    }
                }
                UiToHostMessage::DuplicateTab => {
                    let url = self
                        .windows
                        .get(&win_id)
                        .and_then(|w| w.tab_manager.active_tab())
                        .map(|t| t.url.clone());
                    if let Some(url) = url {
                        self.handle_create_tab(win_id, Some(url));
                    }
                }
                UiToHostMessage::ToggleMuteTab => {
                    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_8;
                    use windows::core::Interface;
                    use wry::WebViewExtWindows;
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        if let Some(view) = win
                            .tab_manager
                            .active_tab_id()
                            .and_then(|id| win.tabs.get(&id))
                        {
                            if let Ok(core) = unsafe { view.controller().CoreWebView2() }
                                .and_then(|c| c.cast::<ICoreWebView2_8>())
                            {
                                let mut muted = windows::core::BOOL(0);
                                if unsafe { core.IsMuted(&mut muted) }.is_ok() {
                                    let muted = !muted.as_bool();
                                    if unsafe { core.SetIsMuted(muted) }.is_ok() {
                                        if let Some(id) = win.tab_manager.active_tab_id() {
                                            win.tab_manager.set_muted(id, muted);
                                            win.sync_ui_state();
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                UiToHostMessage::SwitchTab { id } => {
                    if !self.ensure_tab_view(win_id, id) {
                        return;
                    }
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        win.handle_switch_tab(id, &self.cert_cache);
                    }
                }
                UiToHostMessage::CloseTab { id } => {
                    self.handle_close_tab(win_id, id, event_loop);
                }
                UiToHostMessage::Navigate { url } => {
                    self.handle_navigate(win_id, &url);
                }
                UiToHostMessage::GoBack => {
                    if let Some(win) = self.windows.get(&win_id) {
                        if let Some(active) = win.tab_manager.active_tab() {
                            if let Some(wv) = win.tabs.get(&active.id) {
                                let _ = wv.go_back();
                            }
                        }
                    }
                }
                UiToHostMessage::GoForward => {
                    if let Some(win) = self.windows.get(&win_id) {
                        if let Some(active) = win.tab_manager.active_tab() {
                            if let Some(wv) = win.tabs.get(&active.id) {
                                let _ = wv.go_forward();
                            }
                        }
                    }
                }
                UiToHostMessage::Reload => {
                    let reload_nav = if let Some(win) = self.windows.get(&win_id) {
                        if let Some(active) = win.tab_manager.active_tab() {
                            let desired = !win.private;
                            let current =
                                win.tab_persisted.get(&active.id).copied().unwrap_or(false);
                            if current != desired {
                                Some(active.url.clone())
                            } else {
                                if let Some(wv) = win.tabs.get(&active.id) {
                                    let _ = wv.reload();
                                }
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    if let Some(url) = reload_nav {
                        self.handle_navigate(win_id, &url);
                    }
                }
                UiToHostMessage::Stop => {
                    if let Some(win) = self.windows.get(&win_id) {
                        if let Some(active) = win.tab_manager.active_tab() {
                            if let Some(wv) = win.tabs.get(&active.id) {
                                let _ = wv.evaluate_script("window.stop()");
                            }
                        }
                    }
                }
                UiToHostMessage::MenuToggled { open } => {
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        win.profile_menu_open = open;
                        win.update_layout();
                    }
                }
                UiToHostMessage::ToggleMenuPanel => {
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        win.toggle_sidebar("menu", &self.cert_cache, self.proxy.clone());
                    }
                }
                UiToHostMessage::ToggleSecurityPanel => {
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        win.toggle_sidebar("security", &self.cert_cache, self.proxy.clone());
                    }
                }
                UiToHostMessage::ToggleDownloadsSidebar => {
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        win.toggle_sidebar("downloads", &self.cert_cache, self.proxy.clone());
                    }
                }
                UiToHostMessage::CloseSidebar => {
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        win.close_sidebar();
                    }
                }
                UiToHostMessage::OpenCertificateDialog { host } => {
                    cert::open_native_certificate_dialog(&host);
                }
                UiToHostMessage::PageNavigated { url, title } => {
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        if let Some(active) = win.tab_manager.active_tab() {
                            let active_id = active.id;
                            if !url.starts_with("data:text/html") {
                                win.tab_manager.update_url(active_id, url.clone());
                                win.tab_manager.update_favicon(active_id, favicon_url(&url));
                            }
                            if !title.is_empty() {
                                win.tab_manager.update_title(active_id, title);
                            }
                            win.sync_ui_state();
                            win.update_sidebar_sync(&self.cert_cache);
                        }
                    }
                }
                UiToHostMessage::OpenDevTools => {
                    if let Some(win) = self.windows.get(&win_id) {
                        if let Some(active) = win.tab_manager.active_tab() {
                            if let Some(wv) = win.tabs.get(&active.id) {
                                wv.open_devtools();
                            }
                        }
                    }
                }
                UiToHostMessage::OpenMenu { x, y } => {
                    self.show_native_menu(win_id, x, y, event_loop);
                }
                UiToHostMessage::OpenSettings => {
                    self.handle_open_settings(win_id);
                }
                UiToHostMessage::SetSearchEngine { engine } => {
                    if let Some(profile_id) =
                        self.windows.get(&win_id).map(|win| win.profile_id.clone())
                    {
                        let mut settings = self.settings.clone();
                        let mut values = settings
                            .profile_sync_settings(&profile_id)
                            .unwrap_or_default();
                        values.search_engine = engine;
                        if let Some(profile) =
                            settings.profiles.iter_mut().find(|p| p.id == profile_id)
                        {
                            profile.sync_settings_values = Some(values);
                        }
                        if settings.save_to_path(&get_settings_path()).is_ok() {
                            self.settings = settings;
                            self.sync_settings();
                        }
                    }
                    self.sync_profile_setting_changes();
                }
                UiToHostMessage::ReorderTab {
                    from_index,
                    to_index,
                } => {
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        win.tab_manager.reorder_tab(from_index, to_index);
                        win.sync_ui_state();
                    }
                    self.sync_profile_open_tabs(win_id);
                }
                UiToHostMessage::DetachTabToNewWindow {
                    tab_id,
                    screen_x,
                    screen_y,
                } => {
                    self.handle_detach_tab(win_id, tab_id, screen_x, screen_y, event_loop);
                }
                UiToHostMessage::SetZoom { factor } => {
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        win.set_active_zoom(factor);
                    }
                }
                UiToHostMessage::ZoomIn => {
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        let next = win.get_active_zoom() + 0.1;
                        win.set_active_zoom(next);
                    }
                }
                UiToHostMessage::ZoomOut => {
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        let next = win.get_active_zoom() - 0.1;
                        win.set_active_zoom(next);
                    }
                }
                UiToHostMessage::ZoomReset => {
                    let default_zoom = self
                        .windows
                        .get(&win_id)
                        .map(|win| {
                            self.settings
                                .for_profile(&win.profile_id)
                                .appearance
                                .default_zoom_level
                        })
                        .unwrap_or(self.settings.appearance.default_zoom_level);
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        win.reset_active_zoom(default_zoom);
                    }
                }
                UiToHostMessage::OpenFindInPage => {
                    if !self.settings.features.enable_find_in_page {
                        return;
                    }
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        if let Some(chrome) = &win.chrome_webview {
                            let _ = chrome
                                .evaluate_script("if(window.__showFindBar)window.__showFindBar();");
                        }
                        win.find_bar_open = true;
                        win.update_layout();
                    }
                }
                UiToHostMessage::PrintPage => {
                    if let Some(win) = self.windows.get(&win_id) {
                        if let Some(active) = win.tab_manager.active_tab() {
                            if let Some(wv) = win.tabs.get(&active.id) {
                                if let Err(error) = accelerator::print_page(wv) {
                                    self.notify_user(
                                        win_id,
                                        &format!("Không mở được hộp thoại in: {error}"),
                                    );
                                }
                            }
                        }
                    }
                }
                UiToHostMessage::SavePage => {
                    if let Some(win) = self.windows.get(&win_id) {
                        if let Some(active) = win.tab_manager.active_tab() {
                            if let Some(wv) = win.tabs.get(&active.id) {
                                if let Err(error) = accelerator::save_page(wv) {
                                    self.notify_user(
                                        win_id,
                                        &format!("Không lưu được trang: {error}"),
                                    );
                                }
                            }
                        }
                    }
                }
                UiToHostMessage::FindInPage { query, forward } => {
                    if !self.settings.features.enable_find_in_page {
                        return;
                    }
                    if let Some(win) = self.windows.get(&win_id) {
                        win.handle_find_in_page(&query, forward);
                    }
                }
                UiToHostMessage::CloseFindInPage => {
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        win.find_bar_open = false;
                        win.handle_close_find_in_page();
                        win.update_layout();
                    }
                }
                UiToHostMessage::FindResult { current, total } => {
                    if let Some(win) = self.windows.get(&win_id) {
                        if let Some(chrome) = &win.chrome_webview {
                            let _ = chrome.evaluate_script(&format!(
                                "if (window.__syncFindResult) {{ window.__syncFindResult({{ current: {}, total: {} }}); }}",
                                current, total
                            ));
                        }
                    }
                }
                UiToHostMessage::OpenDownloads => {
                    if !self.settings.features.enable_downloads_manager {
                        return;
                    }
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        win.toggle_sidebar("downloads", &self.cert_cache, self.proxy.clone());
                    }
                }
                UiToHostMessage::DownloadConfirm { .. } => {}
                UiToHostMessage::CancelDownload { download_id } => {
                    downloads::cancel(win_id, download_id)
                }
                UiToHostMessage::OpenDownload {
                    download_id,
                    folder,
                } => {
                    let path = self
                        .windows
                        .get(&win_id)
                        .filter(|win| !win.private)
                        .and_then(|win| win.downloads.get(&download_id))
                        .filter(|item| {
                            item["state"] == "Completed" || item["state"] == "Interrupted"
                        })
                        .and_then(|item| item["path"].as_str())
                        .map(std::path::PathBuf::from);
                    downloads::open_with_fallback(win_id, download_id, folder, path);
                }
                UiToHostMessage::ClearDownloads => {
                    self.clear_download_history(win_id);
                }
                UiToHostMessage::OpenPermissionPrompt => {
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        win.permission_bar_open = true;
                        win.update_layout();
                    }
                }
                UiToHostMessage::ClosePermissionPrompt => {
                    accelerator::cancel_permissions(win_id, None, None);
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        win.permission_bar_open = false;
                        win.update_layout();
                    }
                }
                UiToHostMessage::PermissionResponse {
                    permission_id,
                    allow,
                } => {
                    accelerator::resolve_permission(permission_id, allow);
                }
                UiToHostMessage::TriggerLinkPreview { url, .. } => {
                    if let Some(win) = self.windows.get(&win_id) {
                        if let Some(chrome) = &win.chrome_webview {
                            let escaped = url.replace('\\', "\\\\").replace('\'', "\\'");
                            let _ = chrome.evaluate_script(&format!(
                                "if (window.__showStatusPreview) {{ window.__showStatusPreview('{}'); }}",
                                escaped
                            ));
                        }
                    }
                }
                UiToHostMessage::ChangeDownloadFolder => {
                    let profile_id = self.windows.get(&win_id).map(|win| win.profile_id.clone());
                    let result = self
                        .windows
                        .get(&win_id)
                        .map(|win| choose_download_folder(&win.window));
                    match result {
                        Some(Ok(folder)) => {
                            let Some(profile_id) = profile_id else {
                                return;
                            };
                            let mut settings = self.settings.clone();
                            if !update_profile_download_folder(&mut settings, &profile_id, folder) {
                                self.notify_settings(win_id, "Không tìm thấy hồ sơ tải xuống.");
                                return;
                            }
                            if settings.save_to_path(&get_settings_path()).is_ok() {
                                self.settings = settings;
                                let folder = serde_json::to_string(
                                    &self
                                        .settings
                                        .for_profile(&profile_id)
                                        .downloads
                                        .default_folder
                                        .to_string_lossy(),
                                )
                                .unwrap();
                                for win in self
                                    .windows
                                    .values()
                                    .filter(|win| win.profile_id == profile_id)
                                {
                                    for (id, view) in &win.tabs {
                                        if win
                                            .tab_manager
                                            .get_tab_by_id(*id)
                                            .is_some_and(|tab| tab.url == "evergreen://settings")
                                        {
                                            let _ = view.evaluate_script(&format!(
                                                "document.getElementById('downloadFolderDesc').textContent={folder}"
                                            ));
                                        }
                                    }
                                }
                                self.notify_settings(win_id, "Đã đổi thư mục tải xuống.");
                            } else {
                                self.notify_settings(win_id, "Không thể lưu thư mục tải xuống.");
                            }
                        }
                        Some(Err(error)) if error.code().0 as u32 != 0x800704c7 => {
                            self.notify_settings(win_id, "Không mở được hộp thoại chọn thư mục.");
                        }
                        _ => {}
                    }
                }
                UiToHostMessage::SaveSettings { settings_json } => {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&settings_json) {
                        let profile_id =
                            self.windows.get(&win_id).map(|win| win.profile_id.clone());
                        let has_profile_sync_patch = sync_settings_patch_present(&val);
                        let has_profile_download_patch =
                            profile_download_settings_patch_present(&val);
                        let mut settings = self.settings.clone();
                        settings.update_from_json(&without_sync_settings_patch(
                            &without_profile_download_settings_patch(&val),
                        ));
                        if has_profile_sync_patch {
                            if let Some(profile_id) = profile_id.as_deref() {
                                let mut effective = self.settings.for_profile(profile_id);
                                effective.update_from_json(&val);
                                if let Some(profile) = settings
                                    .profiles
                                    .iter_mut()
                                    .find(|profile| profile.id == profile_id)
                                {
                                    profile.sync_settings_values = Some(
                                        evergreen_core::settings::ProfileSyncSettings::from_global(
                                            &effective,
                                        ),
                                    );
                                }
                            }
                        }
                        if has_profile_download_patch {
                            if let Some(profile_id) = profile_id.as_deref() {
                                apply_profile_download_settings_patch(
                                    &mut settings,
                                    profile_id,
                                    &val,
                                );
                            }
                        }
                        let path = get_settings_path();
                        if settings.save_to_path(&path).is_err() {
                            self.notify_settings(win_id, "Không thể lưu cài đặt. Hãy thử lại.");
                            return;
                        }
                        self.adblock.borrow_mut().set_mode(settings.adblock_mode);
                        self.settings = settings;
                        self.notify_settings(win_id, "Đã lưu lựa chọn của bạn.");
                        self.sync_settings();
                        self.sync_profile_setting_changes();
                    }
                }
                UiToHostMessage::CheckAppUpdates => {
                    let dns_proxy_port = self.doh_proxy.as_ref().map(doh_proxy::DohProxy::port);
                    let google_dns_enabled = self.google_dns_applied;
                    let proxy = self.proxy.clone();
                    std::thread::spawn(move || {
                        let _ = proxy.send_event(BrowserEvent::AppUpdateCheckDone(
                            win_id,
                            app_updates::check_with_google_dns(google_dns_enabled, dns_proxy_port),
                        ));
                    });
                }
                UiToHostMessage::UpdateAdblockFilters => {
                    let dns_proxy_port = self.doh_proxy.as_ref().map(doh_proxy::DohProxy::port);
                    let google_dns_enabled = self.google_dns_applied;
                    let proxy = self.proxy.clone();
                    let directory = get_data_directory();
                    std::thread::spawn(move || {
                        let _ = proxy.send_event(BrowserEvent::AdblockFilterUpdateDone(
                            win_id,
                            adblock_updates::fetch_and_store_with_google_dns(
                                directory,
                                google_dns_enabled,
                                dns_proxy_port,
                            ),
                        ));
                    });
                }
                UiToHostMessage::RunEngineUpdate => {
                    let proxy = self.proxy.clone();
                    std::thread::spawn(move || {
                        let cmd = "powershell -NoProfile -Command \"$u='https://go.microsoft.com/fwlink/p/?LinkId=2124703'; $o=\\\"$env:TEMP\\MicrosoftEdgeWebview2Setup.exe\\\"; Invoke-WebRequest -Uri $u -OutFile $o; Start-Process -FilePath $o -ArgumentList '/silent','/install' -Wait\"";
                        match run_local_command(cmd) {
                            Ok(res) => {
                                let _ = proxy.send_event(BrowserEvent::CommandDone(
                                    "Engine Update".to_string(),
                                    res.success,
                                    res.stdout,
                                ));
                            }
                            Err(e) => {
                                let _ = proxy.send_event(BrowserEvent::CommandDone(
                                    "Engine Update".to_string(),
                                    false,
                                    e.to_string(),
                                ));
                            }
                        }
                    });
                }
                UiToHostMessage::RunForkUpdate => {
                    let proxy = self.proxy.clone();
                    std::thread::spawn(move || {
                        let cmd = "git pull && cargo build --release";
                        match run_local_command(cmd) {
                            Ok(res) => {
                                let _ = proxy.send_event(BrowserEvent::CommandDone(
                                    "Fork Update".to_string(),
                                    res.success,
                                    res.stdout,
                                ));
                            }
                            Err(e) => {
                                let _ = proxy.send_event(BrowserEvent::CommandDone(
                                    "Fork Update".to_string(),
                                    false,
                                    e.to_string(),
                                ));
                            }
                        }
                    });
                }
                UiToHostMessage::ReloadTab { tab_id } => {
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        if let Some(tab) = win
                            .tab_manager
                            .tabs_mut()
                            .iter_mut()
                            .find(|t| t.id == tab_id)
                        {
                            if tab.status == evergreen_core::tabs::TabStatus::Crashed {
                                tab.status = evergreen_core::tabs::TabStatus::Active;
                            }
                        }
                        if let Some(wv) = win.tabs.get(&tab_id) {
                            let _ = wv.reload();
                        }
                        win.sync_ui_state();
                    }
                }
                UiToHostMessage::BypassCertificateError { tab_id, host, url } => {
                    eprintln!(
                        "[TLS BYPASS INITIATED] Bypassing certificate error for host: {} on Tab {:?}",
                        host, tab_id
                    );
                    if let Ok(mut set) = self.allowed_cert_hosts.lock() {
                        set.insert(host.clone());
                    }
                    if let Some(win) = self.windows.get(&win_id) {
                        if let Some(wv) = win.tabs.get(&tab_id) {
                            let _ = wv.load_url(&url);
                        }
                    }
                }
            },
            BrowserEvent::TabTitleChanged(win_id, tab_id, title) => {
                if self
                    .windows
                    .get(&win_id)
                    .and_then(|w| w.tab_manager.get_tab_by_id(tab_id))
                    .is_none_or(|tab| tab.title == title)
                {
                    return;
                }
                let visit = self
                    .windows
                    .get(&win_id)
                    .filter(|w| !w.private)
                    .and_then(|w| w.tab_manager.get_tab_by_id(tab_id))
                    .filter(|t| {
                        t.title != title
                            && (t.url.starts_with("https://") || t.url.starts_with("http://"))
                    })
                    .map(|t| t.url.clone());
                if let Some(url) = visit {
                    self.edit_library(win_id, |library| library.visit(&url, &title, false));
                }
                if let Some(win) = self.windows.get_mut(&win_id) {
                    win.tab_manager.update_title(tab_id, title);
                    win.sync_ui_state();
                    win.update_sidebar_sync(&self.cert_cache);
                }
            }
            BrowserEvent::TabNavigated(win_id, tab_id, url) => {
                if url.starts_with("data:text/html") || url == "about:blank" || url == "aboutblank"
                {
                    return;
                }
                let default_zoom = self
                    .windows
                    .get(&win_id)
                    .map(|win| {
                        self.settings
                            .for_profile(&win.profile_id)
                            .appearance
                            .default_zoom_level
                    })
                    .unwrap_or(self.settings.appearance.default_zoom_level);
                if let Some(win) = self.windows.get_mut(&win_id) {
                    if win.tab_manager.get_tab_by_id(tab_id).is_none() {
                        return;
                    }
                    let desired_zoom = win.zoom_for_url(&url, default_zoom);
                    win.apply_tab_zoom(tab_id, desired_zoom);
                    let url_changed = win
                        .tab_manager
                        .get_tab_by_id(tab_id)
                        .is_some_and(|tab| tab.url != url || tab.favicon_uri != favicon_url(&url));
                    if url_changed {
                        win.tab_manager.update_url(tab_id, url.clone());
                        win.tab_manager.update_favicon(tab_id, favicon_url(&url));
                        win.sync_ui_state();
                        win.update_sidebar_sync(&self.cert_cache);
                    }
                }
                let visit = self
                    .windows
                    .get(&win_id)
                    .filter(|w| !w.private)
                    .and_then(|w| w.tab_manager.get_tab_by_id(tab_id))
                    .map(|tab| (tab.url.clone(), tab.title.clone()));
                if let Some((url, title)) = visit {
                    self.edit_library(win_id, |library| library.visit(&url, &title, false));
                }
                self.sync_passwords(win_id);
            }
            BrowserEvent::TabAudioChanged(win_id, tab_id, playing) => {
                if let Some(win) = self.windows.get_mut(&win_id) {
                    win.tab_manager.set_audio_playing(tab_id, playing);
                    win.sync_ui_state();
                }
            }
            BrowserEvent::HistoryChanged(win_id, tab_id, can_back, can_forward) => {
                if let Some(win) = self.windows.get_mut(&win_id) {
                    if win.tab_manager.get_tab_by_id(tab_id).is_none_or(|tab| {
                        tab.can_go_back == can_back && tab.can_go_forward == can_forward
                    }) {
                        return;
                    }
                    win.tab_manager
                        .update_history_state(tab_id, can_back, can_forward);
                    win.sync_ui_state();
                }
            }
            BrowserEvent::CommandDone(name, success, output) => {
                println!(
                    "[COMMAND RESULT] {}: success={} (output: {})",
                    name,
                    success,
                    output.trim()
                );
            }
            BrowserEvent::BrowsingDataClearDone(win_id, kind, success) => {
                let clear_pip_passwords = match kind {
                    BrowsingDataClearKind::Passwords => true,
                    BrowsingDataClearKind::Selected {
                        clear_pip_passwords,
                        ..
                    } => clear_pip_passwords,
                    _ => false,
                };
                let pip_passwords_removed = if success && clear_pip_passwords {
                    let removed = self
                        .passwords_path(win_id)
                        .is_some_and(|path| passwords::save(&path, &[]).is_ok());
                    if removed {
                        self.sync_passwords(win_id);
                        self.sync_password_vault_if_enabled(win_id);
                    }
                    removed
                } else {
                    true
                };
                if success
                    && matches!(
                        kind,
                        BrowsingDataClearKind::Selected {
                            clear_pip_history: true,
                            ..
                        }
                    )
                {
                    self.edit_library(win_id, |library| library.clear_history());
                }
                if success
                    && matches!(
                        kind,
                        BrowsingDataClearKind::Selected {
                            clear_pip_download_history: true,
                            ..
                        }
                    )
                {
                    downloads::clear_finished(win_id);
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        win.downloads
                            .retain(|_, download| download["state"] == "InProgress");
                        win.update_sidebar_sync(&self.cert_cache);
                    }
                }
                let message = if success {
                    match kind {
                        BrowsingDataClearKind::Cache => {
                            "Đã xóa bộ nhớ đệm; đăng nhập và lịch sử được giữ lại."
                        }
                        BrowsingDataClearKind::Passwords if pip_passwords_removed => {
                            "Đã xóa mật khẩu đã lưu của hồ sơ."
                        }
                        BrowsingDataClearKind::Passwords => {
                            "WebView2 đã xóa mật khẩu; không xóa được kho mật khẩu Pip."
                        }
                        BrowsingDataClearKind::SiteData => {
                            "Đã xóa cookie và dữ liệu website; tải lại các trang để đăng xuất."
                        }
                        BrowsingDataClearKind::Selected {
                            clear_pip_passwords: true,
                            ..
                        } if !pip_passwords_removed => {
                            "WebView2 đã xóa dữ liệu đã chọn nhưng không xóa được kho mật khẩu Pip."
                        }
                        BrowsingDataClearKind::Selected { .. } => {
                            "Đã xóa dữ liệu đã chọn. Tệp đã tải xuống và dấu trang được giữ lại."
                        }
                    }
                } else {
                    match kind {
                        BrowsingDataClearKind::Cache => "Không thể xóa bộ nhớ đệm.",
                        BrowsingDataClearKind::Passwords => "Không thể xóa mật khẩu đã lưu.",
                        BrowsingDataClearKind::SiteData => {
                            "Không thể xóa cookie và dữ liệu website."
                        }
                        BrowsingDataClearKind::Selected { .. } => {
                            "Không thể xóa dữ liệu duyệt web đã chọn."
                        }
                    }
                };
                self.notify_settings(win_id, message);
            }
            BrowserEvent::BrowserExtensionsLoaded(win_id, result) => {
                match result {
                    Ok(extensions) => {
                        let json = serde_json::to_string(&extensions)
                            .unwrap_or_else(|_| "[]".into())
                            .replace('<', "\\u003c");
                        if let Some(profile_id) =
                            self.windows.get(&win_id).map(|win| win.profile_id.clone())
                        {
                            for window in self
                                .windows
                                .values()
                                .filter(|window| !window.private && window.profile_id == profile_id)
                            {
                                for (tab_id, view) in &window.tabs {
                                    if window.tab_manager.get_tab_by_id(*tab_id).is_some_and(
                                        |tab| tab.url.starts_with("evergreen://settings"),
                                    ) {
                                        let _ = view.evaluate_script(&format!(
                                        "if(window.__renderBrowserExtensions)window.__renderBrowserExtensions({json})"
                                    ));
                                    }
                                }
                            }
                        }
                    }
                    Err(message) => {
                        let json = serde_json::to_string(&message)
                            .unwrap_or_else(|_| "\"Không đọc được tiện ích.\"".into())
                            .replace('<', "\\u003c");
                        if let Some(win) = self.windows.get(&win_id) {
                            for (tab_id, view) in &win.tabs {
                                if win
                                    .tab_manager
                                    .get_tab_by_id(*tab_id)
                                    .is_some_and(|tab| tab.url.starts_with("evergreen://settings"))
                                {
                                    let _ = view.evaluate_script(&format!(
                                        "if(window.__renderBrowserExtensionsError)window.__renderBrowserExtensionsError({json})"
                                    ));
                                }
                            }
                        }
                        self.notify_settings(win_id, &message);
                    }
                }
            }
            BrowserEvent::BrowserExtensionOperationDone(win_id, result) => match result {
                Ok(()) => {
                    self.notify_settings(win_id, "Đã cập nhật tiện ích của hồ sơ.");
                    self.request_browser_extensions(win_id);
                }
                Err(message) => self.notify_settings(win_id, &message),
            },
            BrowserEvent::SitePermissionsLoaded(win_id, result) => match result {
                Ok(permissions) => {
                    let json = serde_json::to_string(&permissions)
                        .unwrap_or_else(|_| "[]".into())
                        .replace('<', "\\u003c");
                    if let Some(win) = self.windows.get(&win_id) {
                        for (tab_id, view) in &win.tabs {
                            if win
                                .tab_manager
                                .get_tab_by_id(*tab_id)
                                .is_some_and(|tab| tab.url == "evergreen://settings")
                            {
                                let _ = view.evaluate_script(&format!(
                                    "if(window.__renderSitePermissions)window.__renderSitePermissions({json})"
                                ));
                            }
                        }
                    }
                }
                Err(error) => {
                    eprintln!("Could not load website permissions: {error}");
                    self.notify_settings(win_id, "Không thể đọc danh sách quyền website đã lưu.");
                }
            },
            BrowserEvent::SitePermissionReset(win_id, success) => {
                if success {
                    self.notify_settings(
                        win_id,
                        "Đã đặt lại quyền website; trang sẽ hỏi lại lần sau.",
                    );
                    self.request_site_permissions(win_id);
                } else {
                    self.notify_settings(win_id, "Không thể đặt lại quyền website.");
                }
            }
            BrowserEvent::GoogleDnsCheckDone(win_id, result) => {
                self.record_google_dns_health(&result);
                if let Err(error) = &result {
                    eprintln!("Pip Google DNS route test failed: {error}");
                }
                let message = Self::google_dns_check_message(
                    &result,
                    self.google_dns_fallback_used,
                    self.google_dns_resolution_failed,
                );
                self.notify_settings(win_id, &message);
            }
            BrowserEvent::GoogleDnsHealthCheckDone(result) => {
                self.record_google_dns_health(&result);
                if let Err(error) = result {
                    eprintln!("Pip Google DNS startup route check failed: {error}");
                }
            }
            BrowserEvent::GoogleDnsResolverReport(report) => {
                match report {
                    doh_proxy::DnsResolverReport::SystemFallbackUsed => {
                        self.google_dns_fallback_used = true;
                    }
                    doh_proxy::DnsResolverReport::BothResolversFailed => {
                        self.google_dns_resolution_failed = true;
                    }
                }
                self.google_dns_route_ok = Some(false);
                self.sync_google_dns();
            }
            BrowserEvent::AppUpdateCheckDone(win_id, result) => {
                let payload = match result {
                    Ok(info) => serde_json::to_string(&info).unwrap_or_else(|_| {
                        "{\"error\":\"Không đọc được kết quả cập nhật.\"}".into()
                    }),
                    Err(error) => serde_json::to_string(&serde_json::json!({"error":error}))
                        .unwrap_or_else(|_| "{\"error\":\"Không kiểm tra được cập nhật.\"}".into()),
                }
                .replace('<', "\\u003c");
                if let Some(view) = self.windows.get(&win_id).and_then(|win| {
                    win.tab_manager.active_tab_id().and_then(|id| {
                        win.tab_manager
                            .get_tab_by_id(id)
                            .filter(|tab| tab.url.starts_with("evergreen://settings"))
                            .and_then(|_| win.tabs.get(&id))
                    })
                }) {
                    let _ = view.evaluate_script(&format!(
                        "if(window.__renderAppUpdate)window.__renderAppUpdate({payload})"
                    ));
                }
            }
            BrowserEvent::AdblockFilterUpdateDone(win_id, result) => {
                let payload = match result {
                    Ok(summary) => serde_json::to_string(&summary).unwrap_or_else(|_| {
                        "{\"error\":\"Không đọc được kết quả cập nhật bộ lọc.\"}".into()
                    }),
                    Err(error) => serde_json::to_string(&serde_json::json!({"error":error}))
                        .unwrap_or_else(|_| {
                            "{\"error\":\"Không cập nhật được bộ lọc quảng cáo.\"}".into()
                        }),
                }
                .replace('<', "\\u003c");
                if let Some(view) = self.windows.get(&win_id).and_then(|win| {
                    win.tab_manager.active_tab_id().and_then(|id| {
                        win.tab_manager
                            .get_tab_by_id(id)
                            .filter(|tab| tab.url.starts_with("evergreen://settings"))
                            .and_then(|_| win.tabs.get(&id))
                    })
                }) {
                    let _ = view.evaluate_script(&format!(
                        "if(window.__renderAdblockFilterUpdate)window.__renderAdblockFilterUpdate({payload})"
                    ));
                }
            }
            BrowserEvent::TabSuspendSafetyResult(win_id, tab_id, safe, checked_url) => {
                self.sleep_checking_tabs.remove(&(win_id, tab_id));
                let now_secs = Self::now_secs();
                let eligible = safe
                    && self.settings.performance.background_throttling
                    && self.settings.performance.sleep_after_secs > 0
                    && self.windows.get(&win_id).is_some_and(|win| {
                        !win.permission_bar_open
                            && !win
                                .downloads
                                .values()
                                .any(|download| download["state"] == "InProgress")
                            && win.tab_manager.active_tab_id() != Some(tab_id)
                            && win.tab_manager.get_tab_by_id(tab_id).is_some_and(|tab| {
                                tab.url == checked_url
                                    && eligible_for_memory_suspend(
                                        tab,
                                        now_secs,
                                        self.settings.performance.sleep_after_secs,
                                    )
                            })
                    });
                if !safe {
                    return;
                }
                if !eligible {
                    return;
                }
                let Some(view) = self
                    .windows
                    .get(&win_id)
                    .and_then(|win| win.tabs.get(&tab_id))
                else {
                    return;
                };
                self.suspending_tabs.insert((win_id, tab_id));
                self.suspending_urls.insert((win_id, tab_id), checked_url);
                if accelerator::suspend_webview(view, win_id, tab_id, self.proxy.clone()).is_err() {
                    self.suspending_tabs.remove(&(win_id, tab_id));
                    self.suspending_urls.remove(&(win_id, tab_id));
                }
            }
            BrowserEvent::TabSuspendResult(win_id, tab_id, suspended) => {
                self.suspending_tabs.remove(&(win_id, tab_id));
                let checked_url = self.suspending_urls.remove(&(win_id, tab_id));
                if !suspended {
                    return;
                }
                let now_secs = Self::now_secs();
                let can_mark_suspended = self.settings.performance.background_throttling
                    && self.settings.performance.sleep_after_secs > 0
                    && self.windows.get(&win_id).is_some_and(|win| {
                        !win.permission_bar_open
                            && !win
                                .downloads
                                .values()
                                .any(|download| download["state"] == "InProgress")
                            && win.tab_manager.active_tab_id() != Some(tab_id)
                            && win.tab_manager.get_tab_by_id(tab_id).is_some_and(|tab| {
                                checked_url.as_deref() == Some(tab.url.as_str())
                                    && eligible_for_memory_suspend(
                                        tab,
                                        now_secs,
                                        self.settings.performance.sleep_after_secs,
                                    )
                            })
                    });
                if can_mark_suspended {
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        win.tab_manager.set_tab_suspended(tab_id, true);
                        win.sync_ui_state();
                    }
                } else if let Some(view) = self
                    .windows
                    .get(&win_id)
                    .and_then(|win| win.tabs.get(&tab_id))
                {
                    accelerator::wake_webview(view);
                }
            }
            BrowserEvent::TabDiscardSafetyResult(win_id, tab_id, safe_to_discard, checked_url) => {
                self.discard_checking_tabs.remove(&(win_id, tab_id));
                let memory_still_low = self.memory_pressure_detected();
                let now_secs = Self::now_secs();
                let can_discard = safe_to_discard
                    && memory_still_low
                    && self.windows.get(&win_id).is_some_and(|win| {
                        !win.permission_bar_open
                            && !win
                                .downloads
                                .values()
                                .any(|download| download["state"] == "InProgress")
                            && win.tab_manager.active_tab_id() != Some(tab_id)
                            && win.tab_manager.get_tab_by_id(tab_id).is_some_and(|tab| {
                                tab.url == checked_url && eligible_for_memory_discard(tab, now_secs)
                            })
                    });
                if can_discard {
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        if let Some(view) = win.tabs.remove(&tab_id) {
                            accelerator::hide_webview(&view);
                            drop(view);
                            win.tab_manager.set_tab_discarded(tab_id);
                            win.sync_ui_state();
                        }
                    }
                    self.save_session(win_id);
                }
            }
            BrowserEvent::SyncBookmarksDone(win_id, profile_id, manual, result) => {
                self.syncing_profiles.remove(&profile_id);
                self.sync_profiles();
                match *result {
                    Ok(snapshot) => {
                        let had_pending_sync = self.pending_sync_profiles.remove(&profile_id);
                        if self
                            .settings
                            .profiles
                            .iter()
                            .any(|profile| profile.id == profile_id)
                        {
                            if let Some(path) = self
                                .windows
                                .get(&win_id)
                                .filter(|win| !win.private && win.profile_id == profile_id)
                                .and_then(|_| self.library_path(win_id))
                            {
                                if let Ok(mut library) =
                                    evergreen_core::library::Library::load(&path)
                                {
                                    let sync_preferences = self
                                        .settings
                                        .profiles
                                        .iter()
                                        .find(|p| p.id == profile_id)
                                        .map(|p| {
                                            (
                                                p.sync_history,
                                                p.sync_settings,
                                                p.sync_open_tabs,
                                                p.sync_passwords,
                                            )
                                        })
                                        .unwrap_or_default();
                                    let requested_settings_fingerprint =
                                        snapshot.requested_settings_fingerprint.clone();
                                    let uploaded = snapshot.payload;
                                    let current = sync::sync_payload_from_library(
                                        &library,
                                        sync_preferences.0,
                                    );
                                    let merged = match current.merge(uploaded.clone()) {
                                        Ok(merged) => merged,
                                        Err(error) => {
                                            self.notify_settings(
                                                win_id,
                                                &format!("Không thể gộp dữ liệu Pip Sync: {error}"),
                                            );
                                            return;
                                        }
                                    };
                                    let mut needs_followup = merged != uploaded || had_pending_sync;
                                    let bookmark_category =
                                        merged.bookmarks.clone().unwrap_or_default();
                                    let folders = bookmark_category
                                        .folders
                                        .iter()
                                        .map(|folder| evergreen_core::library::BookmarkFolder {
                                            id: folder.id.clone(),
                                            name: folder.name.clone(),
                                            parent_id: folder.parent_id.clone(),
                                        })
                                        .collect::<Vec<_>>();
                                    let placements = bookmark_category
                                        .placements
                                        .iter()
                                        .map(|placement| {
                                            evergreen_core::library::BookmarkFolderAssignment {
                                                url: placement.url.clone(),
                                                folder_id: placement.folder_id.clone(),
                                                updated_at_ms: i64::try_from(
                                                    placement.updated_at_ms,
                                                )
                                                .unwrap_or(i64::MAX),
                                            }
                                        })
                                        .collect::<Vec<_>>();
                                    let folder_tombstones = bookmark_category
                                        .folder_tombstones
                                        .iter()
                                        .map(|folder| {
                                            evergreen_core::library::BookmarkFolderTombstone {
                                                id: folder.id.clone(),
                                                deleted_at_ms: i64::try_from(folder.deleted_at_ms)
                                                    .unwrap_or(i64::MAX),
                                            }
                                        })
                                        .collect::<Vec<_>>();
                                    let bookmarks = bookmark_category
                                        .items
                                        .iter()
                                        .map(|item| evergreen_core::library::Page {
                                            url: item.page.url.clone(),
                                            title: item.page.title.clone(),
                                        })
                                        .collect::<Vec<_>>();
                                    let bookmark_updates =
                                        local_bookmark_updates(&bookmark_category);
                                    let bookmark_tombstones =
                                        local_bookmark_tombstones(&bookmark_category);
                                    let removed_bookmarks = bookmark_category
                                        .tombstones
                                        .into_iter()
                                        .map(|item| item.url)
                                        .collect::<Vec<_>>();
                                    let mut local_changed = library.bookmarks != bookmarks
                                        || library.bookmark_updates != bookmark_updates
                                        || library.removed_bookmarks != removed_bookmarks
                                        || library.bookmark_tombstones != bookmark_tombstones
                                        || library.bookmark_folders != folders
                                        || library.bookmark_folder_assignments != placements
                                        || library.bookmark_folder_tombstones != folder_tombstones;
                                    let mut settings_update = None;
                                    if local_changed {
                                        library.bookmarks = bookmarks;
                                        library.bookmark_updates = bookmark_updates;
                                        library.removed_bookmarks = removed_bookmarks;
                                        library.bookmark_tombstones = bookmark_tombstones;
                                        library.bookmark_folders = folders;
                                        library.bookmark_folder_assignments = placements;
                                        library.bookmark_folder_tombstones = folder_tombstones;
                                    }
                                    if sync_preferences.0 {
                                        if let Some(history) = merged.history.as_ref() {
                                            let pages = history
                                                .items
                                                .iter()
                                                .filter({
                                                    let mut seen_urls =
                                                        std::collections::HashSet::new();
                                                    move |item: &&sync_v2::HistoryEntry| {
                                                        seen_urls.insert(item.page.url.as_str())
                                                    }
                                                })
                                                .map(|item| evergreen_core::library::Page {
                                                    url: item.page.url.clone(),
                                                    title: item.page.title.clone(),
                                                })
                                                .collect::<Vec<_>>();
                                            let visits = history
                                                .items
                                                .iter()
                                                .map(|item| evergreen_core::library::HistoryVisit {
                                                    url: item.page.url.clone(),
                                                    title: item.page.title.clone(),
                                                    visited_at_ms: i64::try_from(
                                                        item.visited_at_ms,
                                                    )
                                                    .unwrap_or(i64::MAX),
                                                })
                                                .collect::<Vec<_>>();
                                            let mut tombstones = history
                                                .tombstones
                                                .iter()
                                                .map(|item| {
                                                    evergreen_core::library::HistoryTombstone {
                                                        url: item.url.clone(),
                                                        removed_at_ms: i64::try_from(
                                                            item.deleted_at_ms,
                                                        )
                                                        .unwrap_or(i64::MAX),
                                                        visit_at_ms: None,
                                                    }
                                                })
                                                .collect::<Vec<_>>();
                                            tombstones.extend(history.visit_tombstones.iter().map(
                                                |item| {
                                                    evergreen_core::library::HistoryTombstone {
                                                        url: item.url.clone(),
                                                        removed_at_ms: i64::try_from(
                                                            item.deleted_at_ms,
                                                        )
                                                        .unwrap_or(i64::MAX),
                                                        visit_at_ms: Some(
                                                            i64::try_from(item.visited_at_ms)
                                                                .unwrap_or(i64::MAX),
                                                        ),
                                                    }
                                                },
                                            ));
                                            if library.history != pages
                                                || library.history_visits != visits
                                                || library.removed_history != tombstones
                                            {
                                                library.history = pages;
                                                library.history_visits = visits;
                                                library.removed_history = tombstones;
                                                local_changed = true;
                                            }
                                        }
                                    }
                                    if sync_preferences.1 {
                                        if let Some(synced_settings) = merged.settings.as_ref() {
                                            let current_fingerprint =
                                                sync::syncable_settings_fingerprint(
                                                    &sync::syncable_settings_from(
                                                        &self.settings.for_profile(&profile_id),
                                                        0,
                                                    ),
                                                );
                                            if requested_settings_fingerprint.as_deref()
                                                == Some(current_fingerprint.as_str())
                                            {
                                                let mut settings = self.settings.clone();
                                                if apply_syncable_settings_to_profile(
                                                    &mut settings,
                                                    &profile_id,
                                                    synced_settings,
                                                ) {
                                                    settings_update = Some(settings);
                                                }
                                            } else {
                                                // A preference changed while HTTP was in flight.
                                                // Keep the newer local choice and upload it next.
                                                needs_followup = true;
                                            }
                                        }
                                    }
                                    if sync_preferences.2 {
                                        self.render_remote_sync_tabs(win_id, &profile_id, &merged);
                                    }
                                    if sync_preferences.3 {
                                        if let (Some(category), Some(password_path)) =
                                            (merged.passwords.clone(), self.passwords_path(win_id))
                                        {
                                            let incoming = passwords::PasswordSyncSnapshot {
                                                version: category.version,
                                                entries: category
                                                    .entries
                                                    .into_iter()
                                                    .map(|entry| passwords::PasswordSyncEntry {
                                                        origin: entry.origin,
                                                        username: entry.username,
                                                        password: entry.password,
                                                        updated_at_ms: entry.updated_at_ms,
                                                    })
                                                    .collect(),
                                                tombstones: category
                                                    .tombstones
                                                    .into_iter()
                                                    .map(|item| passwords::PasswordTombstone {
                                                        origin: item.origin,
                                                        username: item.username,
                                                        deleted_at_ms: item.deleted_at_ms,
                                                    })
                                                    .collect(),
                                            };
                                            match passwords::apply_sync_snapshot(
                                                &password_path,
                                                incoming,
                                            ) {
                                                Ok(_) => self.sync_passwords(win_id),
                                                Err(message) => {
                                                    self.notify_settings(win_id, &format!("Không thể lưu mật khẩu đã đồng bộ: {message}"));
                                                    needs_followup = true;
                                                }
                                            }
                                        }
                                    }
                                    if !local_changed || library.save(&path).is_ok() {
                                        if let Some(settings) = settings_update {
                                            if settings.save_to_path(&get_settings_path()).is_ok() {
                                                self.settings = settings;
                                                self.sync_settings();
                                                self.sync_profiles();
                                            } else {
                                                self.notify_settings(
                                                    win_id,
                                                    "Không lưu được cài đặt Pip đã đồng bộ.",
                                                );
                                            }
                                        }
                                        if local_changed {
                                            self.sync_library_profile(&profile_id);
                                        }
                                        if needs_followup {
                                            self.start_bookmark_sync(win_id, manual);
                                        } else if manual {
                                            self.notify_settings(win_id, &snapshot.message);
                                        }
                                    } else {
                                        self.notify_settings(
                                            win_id,
                                            "Không lưu được dữ liệu đã đồng bộ vào hồ sơ.",
                                        );
                                        self.notify_library(
                                            win_id,
                                            "Không lưu được dữ liệu đã đồng bộ vào hồ sơ.",
                                        );
                                    }
                                }
                            }
                        }
                    }
                    Err(message) => {
                        self.notify_settings(win_id, &message);
                        self.notify_library(win_id, &message);
                    }
                }
            }
            BrowserEvent::SyncDisabled(win_id, profile_id, vault_id, result) => {
                self.syncing_profiles.remove(&profile_id);
                match result {
                    Ok(()) => {
                        let mut settings = self.settings.clone();
                        let Some(profile) = settings
                            .profiles
                            .iter_mut()
                            .find(|profile| profile.id == profile_id)
                        else {
                            return;
                        };
                        let still_same_vault = profile
                            .sync_credentials_protected
                            .as_deref()
                            .and_then(|value| sync::unprotect_credentials(value).ok())
                            .is_some_and(|credentials| credentials.vault_id == vault_id);
                        if !still_same_vault {
                            return;
                        }
                        profile.sync_credentials_protected = None;
                        if settings.save_to_path(&get_settings_path()).is_ok() {
                            self.settings = settings;
                            self.sync_profiles();
                            self.notify_settings(
                                win_id,
                                "Đã xóa kho đồng bộ trên máy chủ và tắt đồng bộ cho hồ sơ này.",
                            );
                        } else {
                            self.notify_settings(
                                win_id,
                                "Máy chủ đã xóa dữ liệu, nhưng không lưu được thay đổi hồ sơ.",
                            );
                        }
                    }
                    Err(message) => self.notify_settings(win_id, &message),
                }
            }
            BrowserEvent::DownloadRequested(win_id, id) => {
                let settings = self
                    .windows
                    .get(&win_id)
                    .map(|win| self.settings.for_profile(&win.profile_id))
                    .unwrap_or_else(|| self.settings.clone());
                downloads::start(win_id, id, self.proxy.clone(), &settings);
            }
            BrowserEvent::DownloadProgress {
                window_id,
                download_id,
                filename,
                path,
                received_bytes,
                total_bytes,
                state,
            } => {
                let json = serde_json::json!({
                    "download_id": download_id,
                    "filename": filename,
                    "path": path,
                    "received_bytes": received_bytes,
                    "total_bytes": total_bytes,
                    "state": state,
                });
                let script_chrome = format!(
                    "if (window.__downloadProgress) {{ window.__downloadProgress({}); }}",
                    json
                );
                let script_sidebar = format!(
                    "if (window.__syncDownloads) {{ window.__syncDownloads({}); }}",
                    json
                );
                if let Some(win) = self.windows.get_mut(&window_id) {
                    win.downloads.insert(download_id, json.clone());
                    if self.settings.downloads.show_progress_toolbar {
                        if let Some(chrome) = &win.chrome_webview {
                            let _ = chrome.evaluate_script(&script_chrome);
                        }
                    }
                    if win.is_sidebar_open {
                        if let Some(sidebar) = &win.sidebar_webview {
                            let _ = sidebar.evaluate_script(&script_sidebar);
                        }
                    }
                } else {
                    downloads::clear_finished(window_id);
                }
                if state != "InProgress" {
                    let profile_id = self
                        .windows
                        .get(&window_id)
                        .filter(|win| !win.private)
                        .map(|win| win.profile_id.clone());
                    if let Some(profile_id) = profile_id {
                        for win in self
                            .windows
                            .values_mut()
                            .filter(|win| !win.private && win.profile_id == profile_id)
                        {
                            win.downloads.insert(download_id, json.clone());
                            trim_download_history(&mut win.downloads);
                            win.update_sidebar_sync(&self.cert_cache);
                        }
                        self.save_download_history(&profile_id);
                    }
                }
            }
            BrowserEvent::PasswordFillDone(win_id, true) => {
                self.notify_settings(
                    win_id,
                    "Đã điền vào biểu mẫu của tab đã chọn. Hãy kiểm tra trước khi gửi.",
                );
            }
            BrowserEvent::PasswordFillDone(win_id, false) => {
                self.notify_settings(
                    win_id,
                    "Không tìm thấy biểu mẫu đăng nhập trong tab đã chọn.",
                );
            }
            BrowserEvent::PermissionPrompt {
                window_id,
                permission_id,
                origin,
                permission_kind,
            } => {
                let choice = match permission_kind.as_str() {
                    "Microphone" => self.settings.permissions.microphone.as_str(),
                    "Camera" => self.settings.permissions.camera.as_str(),
                    "Geolocation" => self.settings.permissions.location.as_str(),
                    "Notifications" => self.settings.permissions.notifications.as_str(),
                    _ => "ask",
                };
                if choice == "block" || !self.settings.features.enable_permissions_prompt {
                    accelerator::resolve_permission(permission_id, false);
                    return;
                }
                if choice == "allow" {
                    accelerator::resolve_permission(permission_id, true);
                    return;
                }
                accelerator::cancel_permissions(window_id, None, Some(permission_id));
                if let Some(win) = self.windows.get(&window_id) {
                    if let Some(chrome) = &win.chrome_webview {
                        let json = serde_json::json!({
                            "permission_id": permission_id,
                            "origin": origin,
                            "permission_kind": permission_kind,
                        });
                        let _ = chrome.evaluate_script(&format!(
                            "if (window.__permissionPrompt) {{ window.__permissionPrompt({}); }}",
                            json
                        ));
                    }
                }
            }
            BrowserEvent::TabCrashed(win_id, tab_id, kind) => {
                eprintln!(
                    "[TAB CRASH CONTAINMENT] Window {:?}, Tab {:?}, kind={}",
                    win_id, tab_id, kind
                );
                if let Some(win) = self.windows.get_mut(&win_id) {
                    win.tab_manager.mark_tab_crashed(tab_id);
                    win.sync_ui_state();

                    // If the entire browser engine process exited (kind == 0), reconstruct state from snapshot
                    if kind == 0 {
                        eprintln!(
                            "[ENGINE PROCESS RECONSTRUCTION] Reconstructing tabs from snapshot"
                        );
                        let snapshot = win.tab_manager.snapshot();
                        for tab in snapshot {
                            if let Some(wv) = win.tabs.get(&tab.id) {
                                let _ = wv.load_url(&tab.url);
                            }
                        }
                    } else if let Some(wv) = win.tabs.get(&tab_id) {
                        // Render per-tab recovery banner directly inside the crashed tab's webview
                        let html = chrome::protect_internal_html(
                            error_ui::get_crash_html(tab_id.0),
                            &self.internal_ipc_token,
                        );
                        let _ = wv.load_html(&html);
                    }
                }
            }
            BrowserEvent::ServerCertificateError {
                window_id,
                tab_id,
                request_uri,
                error_status,
            } => {
                let host = extract_host(&request_uri).unwrap_or_else(|| request_uri.clone());
                eprintln!(
                    "[STRICT TLS] Certificate error {} for {} on Tab {:?}",
                    error_status, request_uri, tab_id
                );
                if let Some(win) = self.windows.get(&window_id) {
                    if let Some(wv) = win.tabs.get(&tab_id) {
                        let html = chrome::protect_internal_html(
                            error_ui::get_certificate_html(&host, &request_uri, tab_id.0),
                            &self.internal_ipc_token,
                        );
                        let _ = wv.load_html(&html);
                    }
                }
            }
            BrowserEvent::Shortcut(win_id, shortcut) => match shortcut.as_str() {
                "Ctrl+Shift+K" => {
                    if let Some(win) = self.windows.get(&win_id) {
                        if let Some(chrome) = &win.chrome_webview {
                            let _ = chrome
                                .evaluate_script("if(window.openTabSearch)window.openTabSearch()");
                        }
                    }
                }
                "Ctrl+Shift+M" => {
                    if let Some(win) = self.windows.get(&win_id) {
                        if let Some(chrome) = &win.chrome_webview {
                            let _ = chrome.evaluate_script(
                                "if(window.toggleProfileMenu)window.toggleProfileMenu()",
                            );
                        }
                    }
                }
                "Ctrl+Shift+A" => {
                    let url = self
                        .windows
                        .get(&win_id)
                        .and_then(|w| w.tab_manager.active_tab())
                        .map(|t| t.url.clone());
                    if let Some(url) = url {
                        let result = self.adblock.borrow_mut().toggle(&url);
                        match result {
                            Ok(Some(enabled)) => {
                                for window in self.windows.values() {
                                    for view in window.tabs.values() {
                                        if let Err(error) =
                                            adblock::refresh_scriptlets(view, &self.adblock)
                                        {
                                            eprintln!(
                                                "Pip: cannot refresh site scriptlets: {error}"
                                            );
                                        }
                                    }
                                    self.sync_library(window.id);
                                }
                                // No automatic reload: the user keeps unsaved input and chooses F5.
                                let state = if enabled { "bật" } else { "tắt" };
                                if let Some(win) = self.windows.get(&win_id) {
                                    win.window.set_title(&format!("Pip — Đã {state} chặn quảng cáo cho trang này; nhấn F5 để áp dụng"));
                                }
                            }
                            Ok(None) => {}
                            Err(err) => {
                                if let Some(win) = self.windows.get(&win_id) {
                                    win.window.set_title(&format!(
                                        "Pip — Không thể lưu lựa chọn chặn quảng cáo: {err}"
                                    ));
                                }
                            }
                        }
                    }
                }
                "RepairPlayback" => {
                    let Some(tab_id) = self
                        .windows
                        .get(&win_id)
                        .and_then(|win| win.tab_manager.active_tab_id())
                    else {
                        return;
                    };
                    if !self.ensure_tab_view(win_id, tab_id) {
                        self.notify_user(
                            win_id,
                            "Không thể mở tab hiện tại để sửa lỗi phát video.",
                        );
                        return;
                    }
                    let Some(url) = self
                        .windows
                        .get(&win_id)
                        .and_then(|win| win.tab_manager.get_tab_by_id(tab_id))
                        .map(|tab| tab.url.clone())
                    else {
                        return;
                    };
                    let was_enabled = match self.adblock.borrow_mut().disable_for_playback(&url) {
                        Ok(Some(was_enabled)) => was_enabled,
                        Ok(None) => {
                            self.notify_user(
                                win_id,
                                "Sửa lỗi phát video chỉ áp dụng cho trang web HTTP hoặc HTTPS.",
                            );
                            return;
                        }
                        Err(error) => {
                            self.notify_user(
                                win_id,
                                &format!("Không lưu được ngoại lệ chặn quảng cáo: {error}"),
                            );
                            return;
                        }
                    };
                    if was_enabled {
                        for window in self.windows.values() {
                            for view in window.tabs.values() {
                                let _ = adblock::refresh_scriptlets(view, &self.adblock);
                            }
                        }
                    }
                    let Some(view) = self
                        .windows
                        .get(&win_id)
                        .and_then(|win| win.tabs.get(&tab_id))
                    else {
                        self.notify_user(win_id, "Không tìm thấy nội dung tab để tải lại.");
                        return;
                    };
                    if let Err(error) = view.reload() {
                        self.notify_user(
                            win_id,
                            &format!("Pip đã giữ thiết lập chặn quảng cáo nhưng không tải lại được trang: {error}"),
                        );
                        return;
                    }
                    self.notify_user(
                        win_id,
                        if was_enabled {
                            "Đã tắt chặn quảng cáo cho website này; Pip đang tải lại trang để thử sửa video. Bật lại bằng nút chiếc khiên nếu cần."
                        } else {
                            "Đang tải lại trang với chặn quảng cáo đã tắt cho website này."
                        },
                    );
                }
                "F11" => {
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        win.toggle_fullscreen();
                    }
                }
                "Alt+Shift+P" => {
                    let Some(win) = self.windows.get(&win_id) else {
                        return;
                    };
                    if win.private {
                        win.window
                            .set_title("Pip — Kho mật khẩu tắt trong cửa sổ ẩn danh");
                        return;
                    }
                    let Some(tab) = win.tab_manager.active_tab() else {
                        return;
                    };
                    let tab_id = tab.id;
                    let url = tab.url.clone();
                    let Some(path) = self.passwords_path(win_id) else {
                        return;
                    };
                    let entries = match passwords::load(&path) {
                        Ok(entries) => entries,
                        Err(error) => {
                            win.window.set_title(&format!("Pip — {error}"));
                            return;
                        }
                    };
                    let matches = passwords::matching_origin(&entries, &url);
                    match matches.as_slice() {
                        [entry] => self.user_event(
                            event_loop,
                            BrowserEvent::Ipc(
                                win_id,
                                UiToHostMessage::FillPassword {
                                    id: entry.id,
                                    tab_id: tab_id.0,
                                },
                            ),
                        ),
                        [] => win
                            .window
                            .set_title("Pip — Không có mật khẩu đã lưu cho website này"),
                        _ => win.window.set_title(&format!(
                            "Pip — Có {} tài khoản; chọn trong Cài đặt → Mật khẩu và tự điền",
                            matches.len()
                        )),
                    }
                }
                "Ctrl+T" => self.handle_create_tab(win_id, None),
                "Ctrl+Shift+T" => self.handle_reopen_closed_tab(win_id),
                "Ctrl+N" => {
                    let private = self.windows.get(&win_id).is_some_and(|w| w.private);
                    self.open_related_window(event_loop, win_id, private);
                }
                "Ctrl+Shift+N" => self.open_related_window(event_loop, win_id, true),
                "Ctrl+W" => {
                    if let Some(win) = self.windows.get(&win_id) {
                        if let Some(active) = win.tab_manager.active_tab() {
                            let id = active.id;
                            self.handle_close_tab(win_id, id, event_loop);
                        }
                    }
                }
                "Ctrl+Shift+W" => self.close_window(win_id, event_loop),
                "Ctrl+Shift+Delete" => self.handle_open_clear_data_settings(win_id),
                "Ctrl+L" | "Ctrl+E" | "Ctrl+K" => {
                    if let Some(win) = self.windows.get(&win_id) {
                        win.handle_focus_omnibox();
                    }
                }
                "Ctrl+F" if shortcut_enabled_by_feature("Ctrl+F", &self.settings) => {
                    if let Some(win) = self.windows.get(&win_id) {
                        if let Some(chrome) = &win.chrome_webview {
                            let _ = chrome.evaluate_script(
                                "if (window.__toggleFindBar) { window.__toggleFindBar(); }",
                            );
                        }
                    }
                }
                "Ctrl+F" => {}
                "FindNext" | "FindPrevious"
                    if shortcut_enabled_by_feature(&shortcut, &self.settings) =>
                {
                    let forward = shortcut == "FindNext";
                    if let Some(win) = self.windows.get(&win_id) {
                        if let Some(chrome) = &win.chrome_webview {
                            let _ = chrome.evaluate_script(&format!(
                                "if (window.__findNext) window.__findNext({forward});"
                            ));
                        }
                    }
                }
                "FindNext" | "FindPrevious" => {}
                "Ctrl+J" => {
                    if !shortcut_enabled_by_feature("Ctrl+J", &self.settings) {
                        return;
                    }
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        win.toggle_sidebar("downloads", &self.cert_cache, self.proxy.clone());
                    }
                }
                "Ctrl+H" | "Ctrl+Shift+O" => {
                    if let Some(target) = shortcut_library_target(&shortcut) {
                        // Keep the current page open, matching Chrome's History and Bookmarks shortcuts.
                        self.handle_create_tab(win_id, Some(target.to_owned()));
                    }
                }
                "Ctrl+D" => {
                    if let Some(page) = self
                        .windows
                        .get(&win_id)
                        .and_then(|win| win.tab_manager.active_tab())
                        .map(|tab| (tab.url.clone(), tab.title.clone()))
                    {
                        let mut changed = false;
                        self.edit_library(win_id, |library| {
                            changed = library.toggle_bookmark(&page.0, &page.1)
                        });
                        if !changed {
                            self.notify_user(
                                win_id,
                                "Không thể thêm dấu trang: địa chỉ không hợp lệ hoặc đã đạt giới hạn 200 mục.",
                            );
                        }
                    }
                }
                "Ctrl+P" => {
                    if let Some(win) = self.windows.get(&win_id) {
                        if let Some(active) = win.tab_manager.active_tab() {
                            if let Some(wv) = win.tabs.get(&active.id) {
                                if let Err(error) = accelerator::print_page(wv) {
                                    self.notify_user(
                                        win_id,
                                        &format!("Không mở được hộp thoại in: {error}"),
                                    );
                                }
                            }
                        }
                    }
                }
                "Ctrl+S" => {
                    if let Some(win) = self.windows.get(&win_id) {
                        if let Some(active) = win.tab_manager.active_tab() {
                            if let Some(wv) = win.tabs.get(&active.id) {
                                if let Err(error) = accelerator::save_page(wv) {
                                    self.notify_user(
                                        win_id,
                                        &format!("Không lưu được trang: {error}"),
                                    );
                                }
                            }
                        }
                    }
                }
                "Ctrl+Shift+R" => {
                    if let Some(win) = self.windows.get(&win_id) {
                        if let Some(active) = win.tab_manager.active_tab() {
                            if let Some(wv) = win.tabs.get(&active.id) {
                                accelerator::reload_without_cache(wv);
                            }
                        }
                    }
                }
                "Ctrl+R" | "F5" => {
                    if let Some(win) = self.windows.get(&win_id) {
                        if let Some(active) = win.tab_manager.active_tab() {
                            if let Some(wv) = win.tabs.get(&active.id) {
                                let _ = wv.reload();
                            }
                        }
                    }
                }
                "Alt+Left" => {
                    if let Some(win) = self.windows.get(&win_id) {
                        if let Some(active) = win.tab_manager.active_tab() {
                            if let Some(wv) = win.tabs.get(&active.id) {
                                let _ = wv.go_back();
                            }
                        }
                    }
                }
                "Alt+Right" => {
                    if let Some(win) = self.windows.get(&win_id) {
                        if let Some(active) = win.tab_manager.active_tab() {
                            if let Some(wv) = win.tabs.get(&active.id) {
                                let _ = wv.go_forward();
                            }
                        }
                    }
                }
                "Alt+Home" => self.handle_navigate(win_id, "evergreen://newtab"),
                "Ctrl+Tab" | "Ctrl+PageDown" => {
                    let target = self
                        .windows
                        .get(&win_id)
                        .and_then(|win| win.tab_manager.adjacent_tab_id(true));
                    if let Some(target) = target {
                        if let Some(win) = self.windows.get_mut(&win_id) {
                            win.handle_switch_tab(target, &self.cert_cache);
                        }
                    }
                }
                "Ctrl+Shift+Tab" | "Ctrl+PageUp" => {
                    let target = self
                        .windows
                        .get(&win_id)
                        .and_then(|win| win.tab_manager.adjacent_tab_id(false));
                    if let Some(target) = target {
                        if let Some(win) = self.windows.get_mut(&win_id) {
                            win.handle_switch_tab(target, &self.cert_cache);
                        }
                    }
                }
                "Ctrl+1" => {
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        win.switch_to_tab_index(0, &self.cert_cache);
                    }
                }
                "Ctrl+2" => {
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        win.switch_to_tab_index(1, &self.cert_cache);
                    }
                }
                "Ctrl+3" => {
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        win.switch_to_tab_index(2, &self.cert_cache);
                    }
                }
                "Ctrl+4" => {
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        win.switch_to_tab_index(3, &self.cert_cache);
                    }
                }
                "Ctrl+5" => {
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        win.switch_to_tab_index(4, &self.cert_cache);
                    }
                }
                "Ctrl+6" => {
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        win.switch_to_tab_index(5, &self.cert_cache);
                    }
                }
                "Ctrl+7" => {
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        win.switch_to_tab_index(6, &self.cert_cache);
                    }
                }
                "Ctrl+8" => {
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        win.switch_to_tab_index(7, &self.cert_cache);
                    }
                }
                "Ctrl+9" => {
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        let tabs = win.tab_manager.tabs();
                        if !tabs.is_empty() {
                            let last_id = tabs.last().unwrap().id;
                            win.handle_switch_tab(last_id, &self.cert_cache);
                        }
                    }
                }
                shortcut @ ("Ctrl+Plus" | "Ctrl+Minus" | "Ctrl+Zero")
                    if zoom_shortcut_enabled(
                        shortcut,
                        self.settings.features.enable_zoom_controls,
                    ) =>
                {
                    match shortcut {
                        "Ctrl+Plus" => {
                            if let Some(win) = self.windows.get_mut(&win_id) {
                                let next = win.get_active_zoom() + 0.1;
                                win.set_active_zoom(next);
                            }
                        }
                        "Ctrl+Minus" => {
                            if let Some(win) = self.windows.get_mut(&win_id) {
                                let next = win.get_active_zoom() - 0.1;
                                win.set_active_zoom(next);
                            }
                        }
                        "Ctrl+Zero" => {
                            let default_zoom = self
                                .windows
                                .get(&win_id)
                                .map(|win| {
                                    self.settings
                                        .for_profile(&win.profile_id)
                                        .appearance
                                        .default_zoom_level
                                })
                                .unwrap_or(self.settings.appearance.default_zoom_level);
                            if let Some(win) = self.windows.get_mut(&win_id) {
                                win.reset_active_zoom(default_zoom);
                            }
                        }
                        _ => unreachable!("guard accepts only zoom shortcuts"),
                    }
                }
                "Ctrl+Plus" | "Ctrl+Minus" | "Ctrl+Zero" => {}
                "Escape" => {
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        win.handle_escape();
                    }
                }
                "F12" | "Ctrl+Shift+C" | "Ctrl+Shift+I" | "Ctrl+Shift+J" => {
                    if let Some(win) = self.windows.get(&win_id) {
                        if let Some(active) = win.tab_manager.active_tab() {
                            if let Some(wv) = win.tabs.get(&active.id) {
                                wv.open_devtools();
                            }
                        }
                    }
                }
                _ => {}
            },
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                self.close_window(window_id, event_loop);
            }
            WindowEvent::Resized(physical_size) => {
                if let Some(win_ctx) = self.windows.get_mut(&window_id) {
                    let scale = win_ctx.window.scale_factor();
                    let logical = physical_size.to_logical::<f64>(scale);
                    win_ctx.window_width = logical.width;
                    win_ctx.window_height = logical.height;
                    win_ctx.update_layout();
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::Focused(true) => {
                if let Some(win_ctx) = self.windows.get(&window_id) {
                    win_ctx.wake_active();
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state.is_pressed() => {
                match event.logical_key {
                    Key::Named(NamedKey::F11) => {
                        if let Some(win) = self.windows.get_mut(&window_id) {
                            win.toggle_fullscreen();
                        }
                    }
                    Key::Named(NamedKey::F12) => {
                        if let Some(win) = self.windows.get(&window_id) {
                            if let Some(active) = win.tab_manager.active_tab() {
                                if let Some(wv) = win.tabs.get(&active.id) {
                                    wv.open_devtools();
                                }
                            }
                        }
                    }
                    Key::Character(ref s)
                        if s.eq_ignore_ascii_case("t") && self.modifiers.control_key() =>
                    {
                        if self.modifiers.shift_key() {
                            self.handle_reopen_closed_tab(window_id);
                        } else {
                            self.handle_create_tab(window_id, None);
                        }
                    }
                    Key::Character(ref s)
                        if s.eq_ignore_ascii_case("n") && self.modifiers.control_key() =>
                    {
                        let private = self.modifiers.shift_key()
                            || self.windows.get(&window_id).is_some_and(|w| w.private);
                        self.open_related_window(event_loop, window_id, private);
                    }
                    Key::Character(ref s)
                        if s.eq_ignore_ascii_case("w") && self.modifiers.control_key() =>
                    {
                        if let Some(win) = self.windows.get(&window_id) {
                            if let Some(active) = win.tab_manager.active_tab() {
                                let id = active.id;
                                self.handle_close_tab(window_id, id, event_loop);
                            }
                        }
                    }
                    Key::Character(ref s)
                        if (s.eq_ignore_ascii_case("l")
                            || s.eq_ignore_ascii_case("e")
                            || s.eq_ignore_ascii_case("k"))
                            && self.modifiers.control_key() =>
                    {
                        if let Some(win) = self.windows.get(&window_id) {
                            win.handle_focus_omnibox();
                        }
                    }
                    Key::Character(ref s)
                        if s.eq_ignore_ascii_case("f") && self.modifiers.control_key() =>
                    {
                        if shortcut_enabled_by_feature("Ctrl+F", &self.settings) {
                            if let Some(win) = self.windows.get(&window_id) {
                                if let Some(chrome) = &win.chrome_webview {
                                    let _ = chrome.evaluate_script(
                                        "if (window.__toggleFindBar) { window.__toggleFindBar(); }",
                                    );
                                }
                            }
                        }
                    }
                    Key::Character(ref s)
                        if s.eq_ignore_ascii_case("j") && self.modifiers.control_key() =>
                    {
                        if shortcut_enabled_by_feature("Ctrl+J", &self.settings) {
                            if let Some(win) = self.windows.get_mut(&window_id) {
                                win.toggle_sidebar(
                                    "downloads",
                                    &self.cert_cache,
                                    self.proxy.clone(),
                                );
                            }
                        }
                    }
                    Key::Character(ref s)
                        if s.eq_ignore_ascii_case("p") && self.modifiers.control_key() =>
                    {
                        if let Some(win) = self.windows.get(&window_id) {
                            if let Some(active) = win.tab_manager.active_tab() {
                                if let Some(wv) = win.tabs.get(&active.id) {
                                    if let Err(error) = accelerator::print_page(wv) {
                                        self.notify_user(
                                            window_id,
                                            &format!("Không mở được hộp thoại in: {error}"),
                                        );
                                    }
                                }
                            }
                        }
                    }
                    Key::Character(ref s)
                        if s.eq_ignore_ascii_case("s") && self.modifiers.control_key() =>
                    {
                        if let Some(win) = self.windows.get(&window_id) {
                            if let Some(active) = win.tab_manager.active_tab() {
                                if let Some(wv) = win.tabs.get(&active.id) {
                                    if let Err(error) = accelerator::save_page(wv) {
                                        self.notify_user(
                                            window_id,
                                            &format!("Không lưu được trang: {error}"),
                                        );
                                    }
                                }
                            }
                        }
                    }
                    Key::Character(ref s)
                        if s.eq_ignore_ascii_case("r") && self.modifiers.control_key() =>
                    {
                        if let Some(win) = self.windows.get(&window_id) {
                            if let Some(active) = win.tab_manager.active_tab() {
                                if let Some(wv) = win.tabs.get(&active.id) {
                                    if self.modifiers.shift_key() {
                                        accelerator::reload_without_cache(wv);
                                    } else {
                                        let _ = wv.reload();
                                    }
                                }
                            }
                        }
                    }
                    Key::Named(NamedKey::F5) => {
                        if let Some(win) = self.windows.get(&window_id) {
                            if let Some(active) = win.tab_manager.active_tab() {
                                if let Some(wv) = win.tabs.get(&active.id) {
                                    if self.modifiers.control_key() || self.modifiers.shift_key() {
                                        accelerator::reload_without_cache(wv);
                                    } else {
                                        let _ = wv.reload();
                                    }
                                }
                            }
                        }
                    }
                    Key::Named(NamedKey::Escape) => {
                        if let Some(win) = self.windows.get_mut(&window_id) {
                            win.handle_escape();
                        }
                    }
                    Key::Character(ref s)
                        if self.modifiers.control_key()
                            && s.len() == 1
                            && s.chars().next().is_some_and(|c| c.is_ascii_digit()) =>
                    {
                        let digit = s.chars().next().unwrap().to_digit(10).unwrap();
                        if let Some(win) = self.windows.get_mut(&window_id) {
                            if (1..=8).contains(&digit) {
                                win.switch_to_tab_index((digit - 1) as usize, &self.cert_cache);
                            } else if digit == 9 {
                                let tabs = win.tab_manager.tabs();
                                if !tabs.is_empty() {
                                    let last_id = tabs.last().unwrap().id;
                                    win.handle_switch_tab(last_id, &self.cert_cache);
                                }
                            }
                        }
                    }
                    Key::Character(ref s)
                        if self.modifiers.control_key() && (s == "+" || s == "=") =>
                    {
                        if zoom_shortcut_enabled(
                            "Ctrl+Plus",
                            self.settings.features.enable_zoom_controls,
                        ) {
                            if let Some(win) = self.windows.get_mut(&window_id) {
                                let next = win.get_active_zoom() + 0.1;
                                win.set_active_zoom(next);
                            }
                        }
                    }
                    Key::Character(ref s)
                        if self.modifiers.control_key() && (s == "-" || s == "_") =>
                    {
                        if zoom_shortcut_enabled(
                            "Ctrl+Minus",
                            self.settings.features.enable_zoom_controls,
                        ) {
                            if let Some(win) = self.windows.get_mut(&window_id) {
                                let next = win.get_active_zoom() - 0.1;
                                win.set_active_zoom(next);
                            }
                        }
                    }
                    Key::Character(ref s) if self.modifiers.control_key() && s == "0" => {
                        if zoom_shortcut_enabled(
                            "Ctrl+Zero",
                            self.settings.features.enable_zoom_controls,
                        ) {
                            let default_zoom = self
                                .windows
                                .get(&window_id)
                                .map(|win| {
                                    self.settings
                                        .for_profile(&win.profile_id)
                                        .appearance
                                        .default_zoom_level
                                })
                                .unwrap_or(self.settings.appearance.default_zoom_level);
                            if let Some(win) = self.windows.get_mut(&window_id) {
                                win.reset_active_zoom(default_zoom);
                            }
                        }
                    }
                    Key::Named(NamedKey::ArrowLeft) if self.modifiers.alt_key() => {
                        if let Some(win) = self.windows.get(&window_id) {
                            if let Some(active) = win.tab_manager.active_tab() {
                                if let Some(wv) = win.tabs.get(&active.id) {
                                    let _ = wv.go_back();
                                }
                            }
                        }
                    }
                    Key::Named(NamedKey::ArrowRight) if self.modifiers.alt_key() => {
                        if let Some(win) = self.windows.get(&window_id) {
                            if let Some(active) = win.tab_manager.active_tab() {
                                if let Some(wv) = win.tabs.get(&active.id) {
                                    let _ = wv.go_forward();
                                }
                            }
                        }
                    }
                    Key::Named(NamedKey::Home) if self.modifiers.alt_key() => {
                        self.handle_navigate(window_id, "evergreen://newtab");
                    }
                    Key::Named(NamedKey::Tab) if self.modifiers.control_key() => {
                        if let Some(win) = self.windows.get_mut(&window_id) {
                            let tabs = win.tab_manager.tabs();
                            if tabs.len() > 1 {
                                let current_idx = tabs
                                    .iter()
                                    .position(|t| {
                                        Some(t.id) == win.tab_manager.active_tab().map(|a| a.id)
                                    })
                                    .unwrap_or(0);
                                if self.modifiers.shift_key() {
                                    let prev_idx = if current_idx == 0 {
                                        tabs.len() - 1
                                    } else {
                                        current_idx - 1
                                    };
                                    let prev_id = tabs[prev_idx].id;
                                    win.handle_switch_tab(prev_id, &self.cert_cache);
                                } else {
                                    let next_idx = (current_idx + 1) % tabs.len();
                                    let next_id = tabs[next_idx].id;
                                    win.handle_switch_tab(next_id, &self.cert_cache);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(debug_assertions)]
    if std::env::args().any(|a| a == "--verify-cold-start-child") {
        headless_ui_tests::run_cold_start_child();
        return Ok(());
    }
    #[cfg(debug_assertions)]
    if std::env::args().any(|a| a == "--verify-google-dns-webview") {
        headless_ui_tests::run_google_dns_webview();
        return Ok(());
    }
    #[cfg(debug_assertions)]
    if std::env::args().any(|a| a == "--verify-native-ui") {
        headless_ui_tests::run();
        return Ok(());
    }
    // 1. Elevation check: enforce non-elevated running invariant
    if is_process_elevated() {
        #[cfg(target_os = "windows")]
        {
            use windows::core::w;
            use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
            unsafe {
                let _ = MessageBoxW(
                    None,
                    w!(
                        "Running Pip with Administrator privileges is prohibited for security reasons.\n\nPlease launch the browser as a standard user."
                    ),
                    w!("Pip - Security Warning"),
                    MB_OK | MB_ICONERROR,
                );
            }
        }
        eprintln!("SECURITY ERROR: Running as Administrator / elevated is strictly prohibited.");
        std::process::exit(1);
    }

    // 2. Portable mode / Data directory initialization
    let exe_dir = std::env::current_exe()
        .map(|p| p.parent().unwrap_or(&p).to_path_buf())
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    let args: Vec<String> = std::env::args().collect();

    // If invoked with --uninstall, delegate to uninstall.exe and exit immediately
    if args.iter().any(|a| a == "--uninstall") {
        let uninst_exe = exe_dir.join("uninstall.exe");
        if uninst_exe.exists() {
            let mut cmd = std::process::Command::new(&uninst_exe);
            cmd.arg("--uninstall");
            #[cfg(target_os = "windows")]
            {
                use std::os::windows::process::CommandExt;
                cmd.creation_flags(0x08000000);
            }
            let _ = cmd.spawn();
        }
        return Ok(());
    }

    let data_dir = evergreen_core::env::resolve_data_directory(&exe_dir, &args);
    let _ = std::fs::create_dir_all(&data_dir);

    // Direct WebView2 runtime to store all cache, state, and profiles inside data_dir
    let wv2_data_dir = data_dir.join("webview2_data");
    let _ = std::fs::create_dir_all(&wv2_data_dir);
    std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", &wv2_data_dir);

    if evergreen_core::env::is_portable_mode(&exe_dir, &args) {
        println!(
            "[PORTABLE MODE] Operating with zero system residue at: {:?}",
            data_dir
        );
    }

    // 3. Preflight runtime check
    let runtime_version = match detect_webview2_runtime() {
        Some(v) => v,
        None => {
            eprintln!("Microsoft WebView2 Runtime is required but was not found.");
            #[cfg(target_os = "windows")]
            {
                use windows::core::w;
                use windows::Win32::UI::Shell::ShellExecuteW;
                use windows::Win32::UI::WindowsAndMessaging::{
                    MessageBoxW, IDYES, MB_ICONINFORMATION, MB_YESNO, SW_SHOWNORMAL,
                };
                unsafe {
                    let choice = MessageBoxW(
                        None,
                        w!(
                            "Pip cần Microsoft Edge WebView2 Runtime để mở trang web.\n\nBạn muốn mở trang tải chính thức của Microsoft? Sau khi cài đặt, hãy mở lại Pip."
                        ),
                        w!("Cần cài WebView2 Runtime"),
                        MB_YESNO | MB_ICONINFORMATION,
                    );
                    if choice == IDYES {
                        let _ = ShellExecuteW(
                            None,
                            w!("open"),
                            w!("https://developer.microsoft.com/microsoft-edge/webview2/"),
                            None,
                            None,
                            SW_SHOWNORMAL,
                        );
                    }
                }
            }
            std::process::exit(1);
        }
    };
    println!("Detected Evergreen WebView2 Runtime: {}", runtime_version);

    // 4. Launch event loop
    let event_loop = EventLoop::<BrowserEvent>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();
    let mut app = BrowserApp::new(proxy, runtime_version);
    event_loop.run_app(&mut app)?;
    let restart_requested = app.restart_requested;
    drop(app);
    if restart_requested {
        std::thread::sleep(std::time::Duration::from_millis(700));
        let executable = std::env::current_exe()?;
        let mut command = std::process::Command::new(executable);
        command.args(args.iter().skip(1));
        command.spawn()?;
    }

    Ok(())
}

fn zoom_shortcut_enabled(shortcut: &str, controls_enabled: bool) -> bool {
    controls_enabled && matches!(shortcut, "Ctrl+Plus" | "Ctrl+Minus" | "Ctrl+Zero")
}

fn shortcut_library_target(shortcut: &str) -> Option<&'static str> {
    match shortcut {
        "Ctrl+H" => Some("evergreen://newtab?history"),
        "Ctrl+Shift+O" => Some("evergreen://newtab?bookmarks"),
        _ => None,
    }
}

fn shortcut_enabled_by_feature(
    shortcut: &str,
    settings: &evergreen_core::settings::Settings,
) -> bool {
    match shortcut {
        "Ctrl+J" => settings.features.enable_downloads_manager,
        "Ctrl+F" | "FindNext" | "FindPrevious" => settings.features.enable_find_in_page,
        "Ctrl+Plus" | "Ctrl+Minus" | "Ctrl+Zero" => {
            zoom_shortcut_enabled(shortcut, settings.features.enable_zoom_controls)
        }
        _ => true,
    }
}

#[cfg(test)]
mod google_dns_preferences_tests {
    use super::*;

    #[test]
    fn google_dns_manual_check_discloses_system_dns_fallback() {
        let message = BrowserApp::google_dns_check_message(&Ok(()), true, false);
        assert!(message.contains("DNS Windows dự phòng"));
        assert!(message.contains("Google DNS chưa được xác minh đầy đủ"));
        let degraded = BrowserApp::google_dns_check_message(&Ok(()), true, true);
        assert!(degraded.contains("Một hostname khác cũng đã thất bại"));

        let failed =
            BrowserApp::google_dns_check_message(&Err("connection failed".into()), false, true);
        assert!(failed.contains("Google DoH và DNS Windows đều không phân giải được"));
    }

    #[test]
    fn close_warning_only_applies_when_enabled_and_more_than_one_tab_is_open() {
        assert!(!should_confirm_close_window(false, 8));
        assert!(!should_confirm_close_window(true, 1));
        assert!(should_confirm_close_window(true, 2));
    }

    #[test]
    fn session_restore_accepts_only_supported_pip_newtab_routes() {
        assert!(is_restorable_session_url("https://example.com/"));
        assert!(is_restorable_session_url("http://example.com/"));
        assert!(is_restorable_session_url("evergreen://newtab"));
        assert!(is_restorable_session_url("evergreen://newtab?history"));
        assert!(is_restorable_session_url("evergreen://newtab?bookmarks"));
        assert!(is_restorable_session_url("evergreen://settings"));
        assert!(is_restorable_session_url("evergreen://settings?clear-data"));
        assert!(!is_restorable_session_url("evergreen://newtab?unknown"));
        assert!(!is_restorable_session_url("evergreen://settings?unknown"));
        assert!(!is_restorable_session_url("file:///private/path"));
    }

    #[test]
    fn fullscreen_layout_hides_chrome_and_sidebar_and_gives_page_the_full_window() {
        let normal = calculate_window_layout(1280.0, 800.0, 86.0, true, false);
        assert_eq!(normal.chrome_height, 86.0);
        assert_eq!(normal.content_width, 960.0);
        assert_eq!(normal.content_height, 714.0);
        assert!(normal.sidebar_visible);

        let fullscreen = calculate_window_layout(1920.0, 1080.0, 86.0, true, true);
        assert_eq!(fullscreen.chrome_height, 0.0);
        assert_eq!(fullscreen.content_width, 1920.0);
        assert_eq!(fullscreen.content_height, 1080.0);
        assert!(!fullscreen.sidebar_visible);
    }

    #[test]
    fn zoom_shortcuts_follow_the_zoom_controls_setting() {
        for shortcut in ["Ctrl+Plus", "Ctrl+Minus", "Ctrl+Zero"] {
            assert!(zoom_shortcut_enabled(shortcut, true), "{shortcut}");
            assert!(!zoom_shortcut_enabled(shortcut, false), "{shortcut}");
        }
        assert!(!zoom_shortcut_enabled("Ctrl+T", true));
    }

    #[test]
    fn chrome_library_shortcuts_route_to_history_and_bookmarks_in_the_active_tab() {
        assert_eq!(
            shortcut_library_target("Ctrl+H"),
            Some("evergreen://newtab?history")
        );
        assert_eq!(
            shortcut_library_target("Ctrl+Shift+O"),
            Some("evergreen://newtab?bookmarks")
        );
        assert_eq!(shortcut_library_target("Ctrl+T"), None);
    }

    #[test]
    fn keyboard_shortcuts_respect_disabled_feature_settings_on_all_routes() {
        let mut settings = evergreen_core::settings::Settings::default();
        settings.features.enable_downloads_manager = false;
        settings.features.enable_find_in_page = false;
        settings.features.enable_zoom_controls = false;

        for shortcut in [
            "Ctrl+J",
            "Ctrl+F",
            "FindNext",
            "FindPrevious",
            "Ctrl+Plus",
            "Ctrl+Minus",
            "Ctrl+Zero",
        ] {
            assert!(
                !shortcut_enabled_by_feature(shortcut, &settings),
                "{shortcut}"
            );
        }
        assert!(shortcut_enabled_by_feature("Ctrl+T", &settings));
    }

    #[test]
    fn private_windows_do_not_record_or_reopen_closed_tabs() {
        let mut private_tabs = TabManager::new();
        let private_id = private_tabs.create_tab("https://private.example/", 1);
        close_tab_for_window(&mut private_tabs, private_id, true);
        assert!(private_tabs.recently_closed_newest_first().is_empty());
        assert_eq!(pop_closed_tab_for_window(&mut private_tabs, true), None);

        let mut regular_tabs = TabManager::new();
        let regular_id = regular_tabs.create_tab("https://regular.example/", 1);
        close_tab_for_window(&mut regular_tabs, regular_id, false);
        assert_eq!(
            pop_closed_tab_for_window(&mut regular_tabs, false).as_deref(),
            Some("https://regular.example/")
        );
    }

    #[test]
    fn tab_state_sync_serializes_a_borrowed_slice_with_the_existing_wire_shape() {
        let mut tabs = TabManager::new();
        tabs.create_tab("https://one.example/", 1);
        tabs.create_tab("https://two.example/", 2);
        let tab_slice = tabs.tabs();
        let active_tab_id = tabs.active_tab().map(|tab| tab.id);
        let borrowed = BorrowedHostToUiMessage::TabStateSync {
            tabs: tab_slice,
            active_tab_id,
        };

        let BorrowedHostToUiMessage::TabStateSync {
            tabs: serialized_tabs,
            ..
        } = &borrowed;
        assert!(std::ptr::eq(tab_slice.as_ptr(), serialized_tabs.as_ptr()));

        let owned = HostToUiMessage::TabStateSync {
            tabs: tab_slice.to_vec(),
            active_tab_id,
        };
        assert_eq!(
            serde_json::to_string(&borrowed).unwrap(),
            serde_json::to_string(&owned).unwrap()
        );
    }

    #[test]
    fn automatic_bookmark_sync_ignores_history_only_edits() {
        let mut before = evergreen_core::library::Library::default();
        before.bookmarks.push(evergreen_core::library::Page {
            url: "https://saved.example/".into(),
            title: "Saved".into(),
        });
        let mut after = evergreen_core::library::Library {
            bookmarks: before.bookmarks.clone(),
            history: vec![evergreen_core::library::Page {
                url: "https://visited.example/".into(),
                title: "Visited".into(),
            }],
            removed_bookmarks: Vec::new(),
            ..Default::default()
        };
        assert!(!bookmark_sync_state_changed(&before, &after, false));
        assert!(bookmark_sync_state_changed(&before, &after, true));
        after
            .bookmark_updates
            .push(evergreen_core::library::BookmarkUpdate {
                url: "https://saved.example/".into(),
                updated_at_ms: 42,
            });
        assert!(bookmark_sync_state_changed(&before, &after, false));
        after
            .removed_bookmarks
            .push("https://deleted.example/".into());
        assert!(bookmark_sync_state_changed(&before, &after, false));
        after
            .bookmark_tombstones
            .push(evergreen_core::library::BookmarkTombstone {
                url: "https://deleted.example/".into(),
                deleted_at_ms: 43,
            });
        assert!(bookmark_sync_state_changed(&before, &after, false));
        before.bookmarks.push(evergreen_core::library::Page {
            url: "https://another.example/".into(),
            title: "Another".into(),
        });
        assert!(bookmark_sync_state_changed(&before, &after, false));
    }

    #[test]
    fn merged_bookmark_timestamps_and_deletions_project_back_to_library() {
        let category = sync_v2::BookmarkCategory {
            items: vec![sync_v2::TimedBookmark {
                page: sync_v2::SyncPage {
                    url: "https://saved.example/".into(),
                    title: "Saved".into(),
                },
                updated_at_ms: 1234,
            }],
            tombstones: vec![sync_v2::UrlTombstone {
                url: "https://removed.example/".into(),
                deleted_at_ms: 5678,
            }],
            ..Default::default()
        };
        assert_eq!(
            local_bookmark_updates(&category),
            vec![evergreen_core::library::BookmarkUpdate {
                url: "https://saved.example/".into(),
                updated_at_ms: 1234,
            }]
        );
        assert_eq!(
            local_bookmark_tombstones(&category),
            vec![evergreen_core::library::BookmarkTombstone {
                url: "https://removed.example/".into(),
                deleted_at_ms: 5678,
            }]
        );
        let next_snapshot = sync::sync_payload_from_library(
            &evergreen_core::library::Library {
                bookmarks: vec![evergreen_core::library::Page {
                    url: "https://saved.example/".into(),
                    title: "Saved".into(),
                }],
                bookmark_updates: local_bookmark_updates(&category),
                removed_bookmarks: vec!["https://removed.example/".into()],
                bookmark_tombstones: local_bookmark_tombstones(&category),
                ..Default::default()
            },
            false,
        );
        let next_bookmarks = next_snapshot.bookmarks.unwrap();
        assert_eq!(next_bookmarks.items[0].updated_at_ms, 1234);
        assert_eq!(next_bookmarks.tombstones[0].deleted_at_ms, 5678);
    }

    #[test]
    fn applying_synced_settings_changes_only_the_selected_profile() {
        let mut settings = evergreen_core::settings::Settings::default();
        let mut work_values = evergreen_core::settings::ProfileSyncSettings::from_global(&settings);
        work_values.search_engine = "bing".into();
        work_values.default_zoom_level = 1.25;
        settings
            .profiles
            .push(evergreen_core::settings::BrowserProfile {
                id: "Work".into(),
                name: "Work".into(),
                sync_settings_values: Some(work_values),
                ..settings.profiles[0].clone()
            });

        let personal_before = sync::syncable_settings_from(&settings.for_profile("Default"), 0);
        let mut incoming = sync::syncable_settings_from(&settings.for_profile("Default"), 1234);
        incoming.search_engine = sync_v2::SearchEngine::Brave;
        incoming.default_zoom_percent = 175;
        incoming.theme = sync_v2::Theme::Dark;
        assert!(apply_syncable_settings_to_profile(
            &mut settings,
            "Work",
            &incoming
        ));

        assert_eq!(
            sync::syncable_settings_from(&settings.for_profile("Default"), 0),
            personal_before
        );
        let work_after = sync::syncable_settings_from(&settings.for_profile("Work"), 0);
        assert_eq!(work_after.search_engine, sync_v2::SearchEngine::Brave);
        assert_eq!(work_after.default_zoom_percent, 175);
        assert_eq!(work_after.theme, sync_v2::Theme::Dark);
        let work_profile = settings
            .profiles
            .iter()
            .find(|profile| profile.id == "Work")
            .unwrap();
        assert_eq!(work_profile.sync_settings_updated_at_ms, 1234);
        assert_eq!(
            work_profile.sync_settings_fingerprint.as_deref(),
            Some(sync::syncable_settings_fingerprint(&incoming).as_str())
        );
    }

    #[test]
    fn session_snapshots_are_atomically_replaced_and_unchanged_bytes_are_skipped() {
        let root = std::env::temp_dir().join(format!(
            "pip-session-snapshot-test-{}-{}",
            std::process::id(),
            BrowserApp::now_secs()
        ));
        let path = root.join("Default.json");
        assert!(persist_session_snapshot(&path, b"first").unwrap());
        assert!(!persist_session_snapshot(&path, b"first").unwrap());
        assert!(persist_session_snapshot(&path, b"second").unwrap());
        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn startup_manifest_roundtrips_normal_windows_in_profile_slot_order_only() {
        let root = std::env::temp_dir().join(format!(
            "pip-startup-manifest-test-{}-{}",
            std::process::id(),
            BrowserApp::now_secs()
        ));
        let path = root.join("restart-profiles.json");
        let profiles = startup_profiles_for_windows([
            ("Default".into(), 1, false),
            ("Work".into(), 0, false),
            ("Default".into(), 0, false),
            ("Private".into(), 0, true),
        ]);
        assert_eq!(profiles, ["Default", "Default", "Work"]);

        assert!(persist_startup_profiles(&path, &profiles).unwrap());
        let bytes = std::fs::read(&path).unwrap();
        let loaded: Vec<String> = serde_json::from_slice(&bytes).unwrap();
        let known_profiles = vec!["Default".into(), "Work".into()];
        assert_eq!(valid_startup_profiles(&loaded, &known_profiles), profiles);

        let stale_and_invalid = vec!["Deleted".into(), "bad id".into()];
        assert_eq!(
            valid_startup_profiles(&stale_and_invalid, &known_profiles),
            ["Default"]
        );
        assert!(!persist_startup_profiles(&path, &[]).unwrap());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn updating_an_earlier_session_slot_preserves_later_windows() {
        let mut sessions = vec![vec!["first-old"], vec!["second-window"]];
        update_session_slot(&mut sessions, 0, vec!["first-new"]);
        assert_eq!(sessions, vec![vec!["first-new"], vec!["second-window"]]);

        update_session_slot(&mut sessions, 2, vec!["third-window"]);
        assert_eq!(
            sessions,
            vec![
                vec!["first-new"],
                vec!["second-window"],
                vec!["third-window"]
            ]
        );
    }

    #[test]
    fn profile_session_readers_preserve_legacy_single_window_files() {
        let root = std::env::temp_dir().join(format!(
            "pip-profile-session-format-test-{}-{}",
            std::process::id(),
            BrowserApp::now_secs()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let tabs_path = root.join("Default.json");
        let closed_path = root.join("Default.closed.json");
        let mut manager = TabManager::new();
        manager.create_tab("https://legacy.example/", 1);
        let legacy_tabs = manager.snapshot();
        let legacy_closed = vec!["https://closed.example/".to_string()];

        std::fs::write(&tabs_path, serde_json::to_vec(&legacy_tabs).unwrap()).unwrap();
        std::fs::write(&closed_path, serde_json::to_vec(&legacy_closed).unwrap()).unwrap();
        assert_eq!(load_session_windows(&tabs_path), vec![legacy_tabs.clone()]);
        assert_eq!(
            load_closed_histories(&closed_path),
            vec![legacy_closed.clone()]
        );

        let second_tabs = vec![legacy_tabs[0].clone()];
        let second_closed = vec!["https://second-closed.example/".to_string()];
        std::fs::write(
            &tabs_path,
            serde_json::to_vec(&vec![legacy_tabs.clone(), second_tabs.clone()]).unwrap(),
        )
        .unwrap();
        std::fs::write(
            &closed_path,
            serde_json::to_vec(&vec![legacy_closed.clone(), second_closed.clone()]).unwrap(),
        )
        .unwrap();
        assert_eq!(
            load_session_windows(&tabs_path),
            vec![legacy_tabs, second_tabs]
        );
        assert_eq!(
            load_closed_histories(&closed_path),
            vec![legacy_closed, second_closed]
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn extension_folder_requires_a_valid_unpacked_manifest() {
        let root = std::env::temp_dir().join(format!(
            "pip-extension-folder-test-{}-{}",
            std::process::id(),
            BrowserApp::now_secs()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("manifest.json"),
            r#"{"manifest_version":3,"name":"Pip smoke","version":"1.0"}"#,
        )
        .unwrap();
        assert_eq!(
            validate_browser_extension_folder(&root).unwrap(),
            root.canonicalize().unwrap()
        );
        std::fs::write(root.join("manifest.json"), r#"{"manifest_version":4}"#).unwrap();
        assert!(validate_browser_extension_folder(&root).is_err());
        std::fs::write(root.join("manifest.json"), r#"["not","an","object"]"#).unwrap();
        assert!(validate_browser_extension_folder(&root).is_err());
        std::fs::write(
            root.join("manifest.json"),
            r#"{"manifest_version":3,"name":"   ","version":"1.0"}"#,
        )
        .unwrap();
        assert!(validate_browser_extension_folder(&root).is_err());
        #[cfg(windows)]
        {
            use std::os::windows::fs::symlink_file;
            let external_manifest = root.with_extension("outside-manifest.json");
            std::fs::write(
                &external_manifest,
                r#"{"manifest_version":3,"name":"External","version":"1.0"}"#,
            )
            .unwrap();
            std::fs::remove_file(root.join("manifest.json")).unwrap();
            if symlink_file(&external_manifest, root.join("manifest.json")).is_ok() {
                assert!(validate_browser_extension_folder(&root).is_err());
            }
            let _ = std::fs::remove_file(external_manifest);
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn website_permission_origins_are_bounded_http_origins() {
        assert!(valid_permission_origin("https://example.com"));
        assert!(valid_permission_origin("http://localhost:8080"));
        assert!(!valid_permission_origin("https://example.com/path"));
        assert!(!valid_permission_origin("javascript:alert(1)"));
        assert!(!valid_permission_origin("https://user@example.com"));
        assert!(!valid_permission_origin("https://example.com\n"));
    }

    #[test]
    fn dns_proxy_browser_args_preserve_wry_defaults_and_only_proxy_when_enabled() {
        let defaults = dns_proxy_browser_args(false, None);
        assert!(defaults.contains("--disable-features=msWebOOUI,msPdfOOUI"));
        assert!(!defaults.contains("msSmartScreenProtection"));
        assert!(!defaults.contains("--proxy-server="));

        let enabled = dns_proxy_browser_args(true, Some(43210));
        assert!(enabled.contains("--proxy-server=http://127.0.0.1:43210"));
        assert!(enabled.contains("--disable-quic"));
        assert!(enabled.contains(
            "--host-resolver-rules=\"MAP * ~NOTFOUND , EXCLUDE localhost , EXCLUDE 127.0.0.1\""
        ));
        assert!(enabled.contains("--disable-features=msWebOOUI,msPdfOOUI"));
        assert!(!enabled.contains("msSmartScreenProtection"));

        let failed_closed = dns_proxy_browser_args(true, None);
        assert!(!failed_closed.contains("--proxy-server="));
        assert!(failed_closed.contains("--disable-quic"));
        assert!(failed_closed.contains("--host-resolver-rules=\"MAP * ~NOTFOUND"));
    }

    #[test]
    fn automatic_memory_discard_requires_a_long_idle_suspended_safe_web_tab() {
        let mut tabs = TabManager::new();
        let id = tabs.create_tab("https://example.com", 100);
        let tab = tabs.get_tab_by_id(id).unwrap();
        assert!(!eligible_for_memory_discard(
            tab,
            100 + AUTO_DISCARD_IDLE_SECS
        ));

        tabs.set_tab_suspended(id, true);
        assert!(eligible_for_memory_discard(
            tabs.get_tab_by_id(id).unwrap(),
            100 + AUTO_DISCARD_IDLE_SECS
        ));

        tabs.set_memory_discard_protected(id, true);
        assert!(!eligible_for_memory_discard(
            tabs.get_tab_by_id(id).unwrap(),
            100 + AUTO_DISCARD_IDLE_SECS + 1
        ));
        tabs.set_memory_discard_protected(id, false);
        tabs.set_audio_playing(id, true);
        assert!(!eligible_for_memory_discard(
            tabs.get_tab_by_id(id).unwrap(),
            100 + AUTO_DISCARD_IDLE_SECS + 1
        ));
    }

    #[test]
    fn automatic_memory_suspend_requires_idle_and_no_native_protection() {
        let mut tabs = TabManager::new();
        let id = tabs.create_tab("https://example.com", 100);
        tabs.create_tab("about:blank", 100);
        let tab = tabs.get_tab_by_id(id).unwrap();
        assert!(eligible_for_memory_suspend(tab, 400, 300));
        assert!(!eligible_for_memory_suspend(tab, 399, 300));

        tabs.set_memory_discard_protected(id, true);
        assert!(!eligible_for_memory_suspend(
            tabs.get_tab_by_id(id).unwrap(),
            400,
            300
        ));
        tabs.set_memory_discard_protected(id, false);
        tabs.set_audio_playing(id, true);
        assert!(!eligible_for_memory_suspend(
            tabs.get_tab_by_id(id).unwrap(),
            400,
            300
        ));
    }

    #[test]
    fn memory_safety_probe_is_navigation_bound_and_tracks_only_real_risks() {
        assert_eq!(
            parse_memory_safety_result(r#"{"safe":true,"href":"https://example.com/"}"#),
            Some((true, "https://example.com/".into()))
        );
        assert_eq!(
            parse_memory_safety_result(
                r#""{\"safe\":false,\"href\":\"https://example.com/draft\"}""#
            ),
            Some((false, "https://example.com/draft".into()))
        );
        assert!(parse_memory_safety_result("true").is_none());

        assert!(!DISCARD_SAFETY_SCRIPT.contains("interacted"));
        assert!(DISCARD_SAFETY_SCRIPT.contains("hasDirtyPageState"));
        assert!(DISCARD_SAFETY_SCRIPT.contains("getUserMedia"));
        assert!(DISCARD_SAFETY_SCRIPT.contains("activeTracks.size > 0"));
    }

    #[test]
    fn favicons_keep_the_sites_protocol_port_and_host() {
        assert_eq!(
            favicon_url("http://localhost:8000/page"),
            Some("http://localhost:8000/favicon.ico".into())
        );
        assert_eq!(
            favicon_url("https://example.com:8443/path"),
            Some("https://example.com:8443/favicon.ico".into())
        );
        assert_eq!(
            extract_host("https://EXAMPLE.com:8443/path"),
            Some("example.com".into())
        );
        assert_eq!(extract_host("http://[::1]:8000/"), Some("[::1]".into()));
        assert!(favicon_url("evergreen://newtab").is_none());
        assert!(extract_host("javascript:alert(1)").is_none());
    }
}

#[cfg(test)]
mod profile_download_settings_patch_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn all_download_preference_casings_are_detected_and_removed_from_shared_updates() {
        let patches = [
            json!({"askWhereToSave": false}),
            json!({"ask_where_to_save": false}),
            json!({"downloads": {"askWhereToSave": false}}),
            json!({"downloads": {"ask_where_to_save": false}}),
            json!({"defaultFolder": "profile-downloads"}),
            json!({"default_folder": "profile-downloads"}),
            json!({"downloads": {"defaultFolder": "profile-downloads"}}),
            json!({"downloads": {"default_folder": "profile-downloads"}}),
        ];

        for patch in patches {
            assert!(profile_download_settings_patch_present(&patch));
            let shared = without_profile_download_settings_patch(&patch);
            assert!(!profile_download_settings_patch_present(&shared));
        }
    }

    #[test]
    fn nested_download_preferences_update_the_active_profile_without_changing_global_fallback() {
        let patch = json!({
            "downloads": {
                "askWhereToSave": false,
                "default_folder": "profile-a-downloads",
                "show_progress_toolbar": false
            }
        });
        let mut settings = evergreen_core::settings::Settings::default();
        settings.downloads.ask_where_to_save = true;
        settings.downloads.default_folder = std::path::PathBuf::from("legacy-downloads");
        let profile_id = settings.profiles[0].id.clone();

        let shared = without_profile_download_settings_patch(&patch);
        settings.update_from_json(&shared);
        assert!(settings.downloads.ask_where_to_save);
        assert_eq!(
            settings.downloads.default_folder,
            std::path::PathBuf::from("legacy-downloads")
        );

        assert!(apply_profile_download_settings_patch(
            &mut settings,
            &profile_id,
            &patch
        ));
        let active = settings.for_profile(&profile_id);
        assert!(!active.downloads.ask_where_to_save);
        assert_eq!(
            active.downloads.default_folder,
            std::path::PathBuf::from("profile-a-downloads")
        );
        assert!(settings.downloads.ask_where_to_save);
        assert_eq!(
            settings.downloads.default_folder,
            std::path::PathBuf::from("legacy-downloads")
        );
    }
}
