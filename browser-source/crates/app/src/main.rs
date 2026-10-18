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
mod sync;
mod translate;
mod window;

use chrome::{create_bounds, get_chrome_html, normalize_url, CHROME_HEIGHT};
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
use winit::window::{Window, WindowId};
use wry::{WebView, WebViewBuilder, WebViewBuilderExtWindows};

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
    TabTitleChanged(WindowId, TabId, String),
    TabNavigated(WindowId, TabId, String),
    TabAudioChanged(WindowId, TabId, bool),
    HistoryChanged(WindowId, TabId, bool, bool),
    CommandDone(String, bool, String),
    BrowsingDataClearDone(WindowId, BrowsingDataClearKind, bool),
    GoogleDnsCheckDone(WindowId, Result<(), String>),
    GoogleDnsHealthCheckDone(Result<(), String>),
    AppUpdateCheckDone(WindowId, Result<app_updates::AppUpdateInfo, String>),
    AdblockFilterUpdateDone(WindowId, Result<adblock_updates::UpdateSummary, String>),
    TabSuspendResult(WindowId, TabId, bool),
    SyncBookmarksDone(WindowId, String, bool, Result<sync::SyncOutcome, String>),
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
    dns_proxy_port: Option<u16>,
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
    window_width: f64,
    window_height: f64,
    find_bar_open: bool,
    permission_bar_open: bool,
    profile_menu_open: bool,
}
impl Drop for WindowContext {
    fn drop(&mut self) {
        accelerator::cancel_permissions(self.id, None, None);
        downloads::close_window(self.id);
    }
}

impl WindowContext {
    pub fn current_chrome_height(&self) -> f64 {
        CHROME_HEIGHT
            + if self.find_bar_open { 38.0 } else { 0.0 }
            + if self.permission_bar_open { 38.0 } else { 0.0 }
    }

    pub fn update_layout(&self) {
        let chrome_h = self.current_chrome_height();
        if let Some(chrome) = &self.chrome_webview {
            let chrome_height = if self.profile_menu_open {
                chrome_h.max(440.0)
            } else {
                chrome_h
            };
            let chrome_bounds = create_bounds(0.0, 0.0, self.window_width, chrome_height);
            let _ = chrome.set_bounds(chrome_bounds);
            if self.profile_menu_open {
                raise_webview(chrome);
            }
        }

        let content_width = if self.is_sidebar_open {
            (self.window_width - 320.0).max(1.0)
        } else {
            self.window_width
        };

        let content_bounds = create_bounds(
            0.0,
            chrome_h,
            content_width,
            (self.window_height - chrome_h).max(1.0),
        );
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

        if self.is_sidebar_open {
            if let Some(sidebar) = &self.sidebar_webview {
                let sidebar_bounds = create_bounds(
                    content_width,
                    chrome_h,
                    320.0,
                    (self.window_height - chrome_h).max(1.0),
                );
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
            let sync_msg = HostToUiMessage::TabStateSync {
                tabs: self.tab_manager.tabs().to_vec(),
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

    pub fn set_active_zoom(&self, factor: f64) {
        let clamped = (factor * 100.0).round() / 100.0;
        let clamped = clamped.clamp(0.25, 3.0);
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
                .with_additional_browser_args(dns_proxy_browser_args(self.dns_proxy_port))
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
                    "if(window.__syncDownloads)window.__syncDownloads({snapshot});if(window.__renderPrivacy)window.__renderPrivacy({private});if(window.__setKeepAwake)window.__setKeepAwake({keep_awake});if(window.__sidebarSync)window.__sidebarSync({json_str});"
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
            self.sync_zoom(self.get_active_zoom());
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
) -> bool {
    before.bookmarks != after.bookmarks || before.removed_bookmarks != after.removed_bookmarks
}

fn dns_proxy_browser_args(port: Option<u16>) -> String {
    let mut args = String::from("--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection");
    if let Some(port) = port {
        args.push_str(&format!(
            " --proxy-server=http://127.0.0.1:{port} --disable-quic"
        ));
    }
    args
}

fn restart_profiles_path() -> std::path::PathBuf {
    get_data_directory().join("restart-profiles.json")
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
    let metadata = std::fs::metadata(&manifest_path)
        .map_err(|_| "Thư mục không có manifest.json của tiện ích.".to_string())?;
    if !metadata.is_file() || metadata.len() > 1024 * 1024 {
        return Err("manifest.json không hợp lệ hoặc vượt quá 1 MiB.".into());
    }
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(manifest_path)
            .map_err(|_| "Không đọc được manifest.json của tiện ích.".to_string())?,
    )
    .map_err(|_| "manifest.json không phải JSON hợp lệ.".to_string())?;
    if !matches!(manifest["manifest_version"].as_u64(), Some(2 | 3))
        || manifest["name"].as_str().is_none_or(str::is_empty)
        || manifest["version"].as_str().is_none_or(str::is_empty)
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
    active_google_dns: bool,
    google_dns_start_failed: bool,
    google_dns_health_check_started: bool,
    google_dns_route_ok: Option<bool>,
    doh_proxy: Option<doh_proxy::DohProxy>,
    restart_requested: bool,
    restart_profiles: Vec<String>,
    next_tab_id: AtomicU64,
    allowed_cert_hosts: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    next_session_autosave: Instant,
    next_background_tab_check: Instant,
    suspending_tabs: HashSet<(WindowId, TabId)>,
    syncing_profiles: HashSet<String>,
}

impl BrowserApp {
    fn sync_settings(&self) {
        let json = serde_json::to_string(&self.settings).unwrap();
        let preview = self.settings.features.enable_link_preview
            && self.settings.appearance.enable_link_preview
            && self.settings.appearance.show_status_preview;
        for win in self.windows.values() {
            if let Some(chrome) = &win.chrome_webview {
                let _ = chrome.evaluate_script(&format!(
                    "if(window.__applySettings)window.__applySettings({json});"
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
        let enabled = !private
            && self
                .settings
                .profiles
                .iter()
                .any(|p| p.id == profile_id && p.autofill);
        unsafe {
            if let Ok(profile) = view
                .controller()
                .CoreWebView2()
                .and_then(|c| c.cast::<ICoreWebView2_13>())
                .and_then(|c| c.Profile())
                .and_then(|p| p.cast::<ICoreWebView2Profile6>())
            {
                let _ = profile.SetIsPasswordAutosaveEnabled(enabled);
                let _ = profile.SetIsGeneralAutofillEnabled(enabled);
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
                let result = result.and_then(|()| {
                    extension
                        .ok_or_else(|| windows::core::Error::from(windows::core::HRESULT(-1)))
                        .map(|_| ())
                });
                let _ = proxy.send_event(BrowserEvent::BrowserExtensionOperationDone(
                    win_id,
                    result.map_err(|_| "WebView2 không thể cài tiện ích này.".to_string()),
                ));
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
                    let message = if enabled.is_some() {
                        "Không thể đổi trạng thái tiện ích."
                    } else {
                        "Không thể xóa tiện ích."
                    };
                    let _ = proxy_done.send_event(BrowserEvent::BrowserExtensionOperationDone(
                        win_id,
                        result.map_err(|_| message.to_string()),
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
        let tabs = win.tab_manager.snapshot();
        if let Ok(bytes) = serde_json::to_vec(&tabs) {
            let path = get_data_directory()
                .join("sessions")
                .join(format!("{}.json", win.profile_id));
            if let Err(error) = persist_session_snapshot(&path, &bytes) {
                eprintln!("Save session: {error}");
            }
        }
    }

    fn restore_session(&mut self, win_id: WindowId) {
        let Some(win) = self.windows.get(&win_id) else {
            return;
        };
        if win.private {
            return;
        }
        let path = get_data_directory()
            .join("sessions")
            .join(format!("{}.json", win.profile_id));
        let Ok(bytes) = std::fs::read(path) else {
            return;
        };
        let Ok(tabs) = serde_json::from_slice::<Vec<TabState>>(&bytes) else {
            return;
        };
        let mut restored = Vec::new();
        for mut tab in tabs.into_iter().take(100) {
            if !tab.url.starts_with("https://")
                && !tab.url.starts_with("http://")
                && tab.url != "evergreen://newtab"
            {
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
        for win in self.windows.values() {
            if let Some(chrome) = &win.chrome_webview {
                let _ = chrome.evaluate_script(&format!(
                    "if(window.__renderGoogleDns) window.__renderGoogleDns({enabled},{active},{failed},{route_ok});"
                ));
            }
            for (id, view) in &win.tabs {
                if win
                    .tab_manager
                    .get_tab_by_id(*id)
                    .is_some_and(|tab| tab.url == "evergreen://settings")
                {
                    let _ = view.evaluate_script(&format!(
                        "if(window.__renderGoogleDns) window.__renderGoogleDns({enabled},{active},{failed},{route_ok});"
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
        self.google_dns_route_ok = Some(result.is_ok());
        self.sync_google_dns();
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
                    "syncEnabled": profile.sync_credentials_protected.is_some(),
                    "isOpen": self.windows.values().any(|window| window.profile_id == profile.id),
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
        let doh_proxy = if settings.google_dns {
            match doh_proxy::DohProxy::start() {
                Ok(proxy) => Some(proxy),
                Err(error) => {
                    eprintln!("Pip Google DNS proxy failed to start: {error}");
                    None
                }
            }
        } else {
            None
        };
        let google_dns_start_failed = settings.google_dns && doh_proxy.is_none();
        let restart_profiles = std::fs::read(restart_profiles_path())
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
            ))),
            proxy,
            cert_cache: cert::CertificateCache::new(),
            runtime_version,
            modifiers: ModifiersState::default(),
            background_check: std::env::args().any(|a| a == "--background-check")
                && std::env::args().any(|a| a == "--portable"),
            active_google_dns: doh_proxy.is_some(),
            google_dns_start_failed,
            google_dns_health_check_started: false,
            google_dns_route_ok: None,
            doh_proxy,
            restart_requested: false,
            restart_profiles,
            settings,
            next_tab_id: AtomicU64::new(1),
            allowed_cert_hosts: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
            next_session_autosave: Instant::now() + SESSION_AUTOSAVE_INTERVAL,
            next_background_tab_check: Instant::now() + Duration::from_secs(15),
            suspending_tabs: HashSet::new(),
            syncing_profiles: HashSet::new(),
        }
    }

    pub fn now_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
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
        let mut candidates = Vec::new();
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
                if Some(tab.id) == active
                    || tab.status == evergreen_core::tabs::TabStatus::Crashed
                    || tab.keep_awake
                    || tab.is_loading
                    || tab.is_audio_playing
                    || tab.pinned
                    || now_secs.saturating_sub(tab.last_active_timestamp_secs) < idle_after
                    || !(tab.url.starts_with("http://") || tab.url.starts_with("https://"))
                    || !win.tabs.contains_key(&tab.id)
                    || self.suspending_tabs.contains(&(*window_id, tab.id))
                {
                    continue;
                }
                candidates.push((*window_id, tab.id));
            }
        }
        for (window_id, tab_id) in candidates {
            let Some(view) = self
                .windows
                .get(&window_id)
                .and_then(|win| win.tabs.get(&tab_id))
            else {
                continue;
            };
            self.suspending_tabs.insert((window_id, tab_id));
            if accelerator::suspend_webview(view, window_id, tab_id, self.proxy.clone()).is_err() {
                self.suspending_tabs.remove(&(window_id, tab_id));
            }
        }
    }

    fn create_browser_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        initial_url: Option<String>,
    ) -> Option<WindowId> {
        self.create_profile_window(event_loop, initial_url, "Default".into(), false)
    }

    fn open_related_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        source: WindowId,
        private: bool,
    ) {
        if let Some(win) = self.windows.get(&source) {
            let profile = win.profile_id.clone();
            self.create_profile_window(event_loop, None, profile, private);
        }
    }

    fn create_profile_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        initial_url: Option<String>,
        profile_id: String,
        private: bool,
    ) -> Option<WindowId> {
        let restore = initial_url.is_none()
            && !private
            && !self
                .windows
                .values()
                .any(|w| w.profile_id == profile_id && !w.private);
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
            private,
            dns_proxy_port: self.doh_proxy.as_ref().map(doh_proxy::DohProxy::port),
            id: win_id,
            window: window.clone(),
            tab_manager: TabManager::new(),
            chrome_webview: None,
            sidebar_webview: None,
            is_sidebar_open: false,
            sidebar_mode: "menu".to_string(),
            tabs: HashMap::new(),
            tab_persisted: HashMap::new(),
            window_width: win_width,
            window_height: win_height,
            find_bar_open: false,
            permission_bar_open: false,
            profile_menu_open: false,
        };

        // Create Chrome Strip WebView
        let chrome_bounds = create_bounds(0.0, 0.0, win_width, CHROME_HEIGHT);
        let proxy_ipc = self.proxy.clone();

        let chrome_html = get_chrome_html(&self.settings.search_engine)
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
            .with_additional_browser_args(dns_proxy_browser_args(win_ctx.dns_proxy_port))
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
                    self.settings.search_engine
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
        if restore {
            self.restore_session(win_id);
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
    ) -> Result<WebView, wry::Error> {
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
            "window.__pipLinkPreviewEnabled={};\n{NAV_WATCHER_SCRIPT}",
            self.settings.features.enable_link_preview
                && self.settings.appearance.enable_link_preview
                && self.settings.appearance.show_status_preview
        );
        let context = self.windows.get(&win_id).expect("tab window exists");
        let is_persistent = !context.private;
        let profile_id = context.profile_id.clone();
        let google_dns_active = self.active_google_dns;
        let google_dns_failed = self.google_dns_start_failed;
        let google_dns_route_ok = self.google_dns_route_ok;

        let mut builder = WebViewBuilder::new()
            .with_additional_browser_args(dns_proxy_browser_args(context.dns_proxy_port))
            .with_bounds(bounds)
            .with_browser_extensions_enabled(true)
            .with_incognito(!is_persistent)
            .with_profile_name(&profile_id)
            .with_transparent(true)
            .with_background_color((24, 24, 32, 255))
            .with_initialization_script(&init_script)
            .with_devtools(true);
        let search_name = self.settings.search_engine_display_name().to_string();
        let ipc_token = self.internal_ipc_token.clone();
        let settings_clone = self.settings.clone();
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
                    &self.settings,
                    google_dns_active,
                    google_dns_failed,
                    google_dns_route_ok,
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
        );

        // Register interception before the first remote navigation, including restored tabs.
        if !is_newtab && !is_settings && !is_not_found && !is_loading_page {
            webview.load_url(url)?;
        }

        Ok(webview)
    }

    fn handle_create_tab(&mut self, win_id: WindowId, url: Option<String>) {
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
                        let default_zoom = self.settings.appearance.default_zoom_level;
                        if (default_zoom - 1.0).abs() > f64::EPSILON {
                            win_ctx.set_active_zoom(default_zoom);
                        } else {
                            win_ctx.sync_zoom(1.0);
                        }
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
        let data = serde_json::json!({"bookmarks":library.bookmarks,"history":if win.private {vec![]}else{library.history},"shortcuts":shortcuts,"private":win.private});
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
        let result = (|| -> std::io::Result<bool> {
            let mut library = evergreen_core::library::Library::load(&path)?;
            let before = library.clone();
            edit(&mut library);
            let bookmarks_changed = bookmark_sync_state_changed(&before, &library);
            if before == library {
                return Ok(false);
            }
            library.save(&path).map(|()| bookmarks_changed)
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
            if manual {
                self.notify_settings(win_id, "Pip Sync của hồ sơ này đang chạy.");
            }
            return;
        }
        let Some(protected) = self
            .settings
            .profiles
            .iter()
            .find(|profile| profile.id == profile_id)
            .and_then(|profile| profile.sync_credentials_protected.as_deref())
        else {
            if manual {
                self.notify_settings(win_id, "Tạo hoặc nhập mã Pip Sync trước.");
            }
            return;
        };
        let credentials = match sync::unprotect_credentials(protected) {
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
        let snapshot = sync::BookmarkSnapshot {
            version: 1,
            bookmarks: library.bookmarks,
            removed_bookmarks: library.removed_bookmarks,
        };
        let proxy = self.proxy.clone();
        self.syncing_profiles.insert(profile_id.clone());
        let event_profile_id = profile_id.clone();
        if std::thread::Builder::new()
            .name("pip-bookmark-sync".into())
            .spawn(move || {
                let result = sync::sync_bookmarks(&credentials, snapshot);
                let _ = proxy.send_event(BrowserEvent::SyncBookmarksDone(
                    win_id,
                    event_profile_id,
                    manual,
                    result,
                ));
            })
            .is_ok()
        {
            if manual {
                self.notify_settings(win_id, "Đang mã hóa và đồng bộ dấu trang…");
            }
        } else {
            self.syncing_profiles.remove(&profile_id);
            if manual {
                self.notify_settings(win_id, "Không khởi chạy được đồng bộ.");
            }
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
        let result = (|| -> Result<usize, String> {
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
                if imported > 0 {
                    self.start_bookmark_sync(win_id, false);
                }
                let message = if imported == 0 {
                    "Không có dấu trang mới để nhập. Mục trùng được bỏ qua; hồ sơ đã đủ 200 mục cũng không nhận thêm.".to_string()
                } else {
                    format!("Đã nhập {imported} dấu trang vào hồ sơ hiện tại.")
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
        if self.windows.contains_key(&win_id) {
            self.save_session(win_id);
            self.windows.remove(&win_id);
            if self.windows.is_empty() {
                event_loop.exit();
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

            match win_ctx.tab_manager.close_tab(target_id) {
                Some(next_active_id) => {
                    win_ctx.handle_switch_tab(next_active_id, &self.cert_cache);
                    false
                }
                None => true,
            }
        } else {
            false
        };

        if should_close_window {
            self.windows.remove(&win_id);
            if self.windows.is_empty() {
                event_loop.exit();
            }
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
            } else if let Some(new_win_id) =
                self.create_profile_window(event_loop, Some(tab_state.url), profile_id, private)
            {
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
                }
            }
        }
    }

    fn handle_navigate(&mut self, win_id: WindowId, raw_url: &str) {
        match normalize_url(raw_url, self.settings.search_url_template()) {
            Ok(target) if target.starts_with("evergreen://settings") => {
                self.handle_open_settings(win_id)
            }
            Ok(target) if target.starts_with("evergreen://newtab") => {
                self.handle_navigate_to_newtab(win_id, &target);
            }
            Ok(target) => {
                if let Some(win) = self.windows.get_mut(&win_id) {
                    if let Some(id) = win.tab_manager.active_tab_id() {
                        if let Some(view) = win.tabs.get(&id) {
                            // SourceChanged is the authoritative committed URL.
                            if let Err(error) = view.load_url(&target) {
                                eprintln!("Pip navigation failed for {target}: {error}");
                            }
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
            self.create_browser_window(event_loop, None);
            let profiles = std::mem::take(&mut self.restart_profiles);
            for profile_id in profiles {
                if profile_id != "Default"
                    && evergreen_core::settings::valid_profile_id(&profile_id)
                    && self
                        .settings
                        .profiles
                        .iter()
                        .any(|profile| profile.id == profile_id)
                {
                    self.create_profile_window(event_loop, None, profile_id, false);
                }
            }
            let _ = std::fs::remove_file(restart_profiles_path());
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
                        self.edit_library(win_id, |library| library.toggle_bookmark(&url, &title));
                    }
                }
                UiToHostMessage::RemoveBookmark { url } => {
                    self.edit_library(win_id, |library| library.remove_bookmark(&url))
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
                UiToHostMessage::ImportBookmarks => self.import_bookmarks(win_id),
                UiToHostMessage::ExportBookmarks => self.export_bookmarks(win_id),
                UiToHostMessage::ClearHistory => {
                    if self.windows.get(&win_id).is_some_and(|win| !win.private) {
                        self.edit_library(win_id, |library| library.history.clear());
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
                        self.create_profile_window(event_loop, None, id, false);
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
                                    sync_credentials_protected: None,
                                });
                            if settings.save_to_path(&get_settings_path()).is_ok() {
                                self.settings = settings;
                                self.sync_profiles();
                                self.create_profile_window(event_loop, None, id, false);
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
                    let profile_view = self
                        .windows
                        .values()
                        .find(|window| window.profile_id == id)
                        .and_then(|window| window.tabs.values().next());
                    let Some(profile_view) = profile_view else {
                        self.notify_settings(win_id, "Hãy mở hồ sơ cần xóa trước.");
                        return;
                    };

                    // Save the restart list before destructive work so other open profiles
                    // return after the WebView2 profile has been deleted and Pip restarts.
                    let restart_profiles: std::collections::HashSet<String> = self
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
                    let restart_profiles = restart_profiles.into_iter().collect::<Vec<_>>();
                    let restart_list_saved = serde_json::to_vec(&restart_profiles)
                        .map_err(std::io::Error::other)
                        .and_then(|data| std::fs::write(restart_profiles_path(), data));
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
                        let _ = std::fs::remove_file(restart_profiles_path());
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
                        let _ = std::fs::remove_file(restart_profiles_path());
                        self.notify_settings(win_id, "WebView2 không thể xóa hồ sơ này. Hãy cập nhật WebView2 Runtime rồi thử lại.");
                        return;
                    }

                    self.settings = settings;
                    let data_dir = get_data_directory();
                    let _ =
                        std::fs::remove_file(data_dir.join("sessions").join(format!("{id}.json")));
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
                }
                UiToHostMessage::RestoreSession => self.restore_session(win_id),
                UiToHostMessage::ReopenClosedTab => {
                    let url = self
                        .windows
                        .get_mut(&win_id)
                        .and_then(|w| w.tab_manager.pop_last_closed());
                    if let Some(url) = url {
                        self.handle_create_tab(win_id, Some(url));
                    }
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
                    let proxy = self.proxy.clone();
                    let event_profile_id = profile_id.clone();
                    self.syncing_profiles.insert(profile_id.clone());
                    if std::thread::Builder::new()
                        .name("pip-bookmark-sync-delete".into())
                        .spawn(move || {
                            let result = sync::delete_vault(&credentials);
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
                UiToHostMessage::SavePassword {
                    origin,
                    username,
                    password,
                } => {
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
                    if let Err(message) =
                        passwords::upsert(&mut entries, &origin, &username, &password)
                            .and_then(|_| passwords::save(&path, &entries))
                    {
                        self.notify_settings(win_id, &message);
                        return;
                    }
                    self.sync_passwords(win_id);
                    self.notify_settings(
                        win_id,
                        "Đã lưu mật khẩu được mã hóa bằng tài khoản Windows này.",
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
                    let profiles: std::collections::HashSet<String> = self
                        .windows
                        .values()
                        .filter(|win| !win.private)
                        .map(|win| win.profile_id.clone())
                        .collect();
                    for open_win_id in self.windows.keys().copied().collect::<Vec<_>>() {
                        self.save_session(open_win_id);
                    }
                    let profiles: Vec<String> = profiles.into_iter().collect();
                    let saved = serde_json::to_vec(&profiles)
                        .map_err(std::io::Error::other)
                        .and_then(|data| std::fs::write(restart_profiles_path(), data));
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
                    self.settings.search_engine = engine.clone();
                    let path = get_settings_path();
                    let _ = self.settings.save_to_path(&path);
                    for win in self.windows.values() {
                        if let Some(chrome) = &win.chrome_webview {
                            let _ = chrome.evaluate_script(&format!(
                                "if (window.__syncSearchEngine) {{ window.__syncSearchEngine('{}'); }}",
                                engine
                            ));
                        }
                    }
                }
                UiToHostMessage::ReorderTab {
                    from_index,
                    to_index,
                } => {
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        win.tab_manager.reorder_tab(from_index, to_index);
                        win.sync_ui_state();
                    }
                }
                UiToHostMessage::DetachTabToNewWindow {
                    tab_id,
                    screen_x,
                    screen_y,
                } => {
                    self.handle_detach_tab(win_id, tab_id, screen_x, screen_y, event_loop);
                }
                UiToHostMessage::SetZoom { factor } => {
                    if let Some(win) = self.windows.get(&win_id) {
                        win.set_active_zoom(factor);
                    }
                }
                UiToHostMessage::ZoomIn => {
                    if let Some(win) = self.windows.get(&win_id) {
                        win.set_active_zoom(win.get_active_zoom() + 0.1);
                    }
                }
                UiToHostMessage::ZoomOut => {
                    if let Some(win) = self.windows.get(&win_id) {
                        win.set_active_zoom(win.get_active_zoom() - 0.1);
                    }
                }
                UiToHostMessage::ZoomReset => {
                    if let Some(win) = self.windows.get(&win_id) {
                        win.set_active_zoom(1.0);
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
                    let result = self
                        .windows
                        .get(&win_id)
                        .map(|win| choose_download_folder(&win.window));
                    match result {
                        Some(Ok(folder)) => {
                            let mut settings = self.settings.clone();
                            settings.downloads.default_folder = folder;
                            if settings.save_to_path(&get_settings_path()).is_ok() {
                                self.settings = settings;
                                let folder = serde_json::to_string(
                                    &self.settings.downloads.default_folder.to_string_lossy(),
                                )
                                .unwrap();
                                for win in self.windows.values() {
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
                        let mut settings = self.settings.clone();
                        settings.update_from_json(&val);
                        let path = get_settings_path();
                        if settings.save_to_path(&path).is_err() {
                            self.notify_settings(win_id, "Không thể lưu cài đặt. Hãy thử lại.");
                            return;
                        }
                        self.settings = settings;
                        self.notify_settings(win_id, "Đã lưu lựa chọn của bạn.");
                        self.sync_settings();
                    }
                }
                UiToHostMessage::CheckAppUpdates => {
                    let proxy = self.proxy.clone();
                    std::thread::spawn(move || {
                        let _ = proxy.send_event(BrowserEvent::AppUpdateCheckDone(
                            win_id,
                            app_updates::check(),
                        ));
                    });
                }
                UiToHostMessage::UpdateAdblockFilters => {
                    let proxy = self.proxy.clone();
                    let directory = get_data_directory();
                    std::thread::spawn(move || {
                        let _ = proxy.send_event(BrowserEvent::AdblockFilterUpdateDone(
                            win_id,
                            adblock_updates::fetch_and_store(directory),
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
                    eprintln!("[TLS BYPASS INITIATED] Bypassing certificate error for host: {} on Tab {:?}", host, tab_id);
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
                if let Some(win) = self.windows.get_mut(&win_id) {
                    if win
                        .tab_manager
                        .get_tab_by_id(tab_id)
                        .is_none_or(|tab| tab.url == url && tab.favicon_uri == favicon_url(&url))
                    {
                        return;
                    }
                    win.tab_manager.update_url(tab_id, url.clone());
                    win.tab_manager.update_favicon(tab_id, favicon_url(&url));
                    win.sync_ui_state();
                    win.update_sidebar_sync(&self.cert_cache);
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
                    let removed =
                        self.passwords_path(win_id)
                            .is_some_and(|path| match std::fs::remove_file(path) {
                                Ok(()) => true,
                                Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
                                Err(error) => {
                                    eprintln!("Pip password vault removal failed: {error}");
                                    false
                                }
                            });
                    if removed {
                        self.sync_passwords(win_id);
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
                    self.edit_library(win_id, |library| library.history.clear());
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
                let message = match result {
                    Ok(()) => {
                        "Đã thử tải Google thành công qua proxy DNS của Pip. DNS Windows không bị thay đổi.".to_string()
                    }
                    Err(error) => {
                        eprintln!("Pip Google DNS route test failed: {error}");
                        let reason: String = error.chars().take(120).collect();
                        format!("Tuyến DNS của Pip chưa tải được Google. Kiểm tra mạng hoặc tường lửa. Chi tiết: {reason}")
                    }
                };
                self.notify_settings(win_id, &message);
            }
            BrowserEvent::GoogleDnsHealthCheckDone(result) => {
                self.record_google_dns_health(&result);
                if let Err(error) = result {
                    eprintln!("Pip Google DNS startup route check failed: {error}");
                }
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
            BrowserEvent::TabSuspendResult(win_id, tab_id, suspended) => {
                self.suspending_tabs.remove(&(win_id, tab_id));
                if !suspended {
                    return;
                }
                if let Some(win) = self.windows.get_mut(&win_id) {
                    if win.tab_manager.active_tab_id() != Some(tab_id) {
                        win.tab_manager.set_tab_suspended(tab_id, true);
                        win.sync_ui_state();
                    }
                }
            }
            BrowserEvent::SyncBookmarksDone(win_id, profile_id, manual, result) => {
                self.syncing_profiles.remove(&profile_id);
                match result {
                    Ok(snapshot) => {
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
                                    let uploaded = sync::BookmarkSnapshot {
                                        version: 1,
                                        bookmarks: snapshot.bookmarks,
                                        removed_bookmarks: snapshot.removed_bookmarks,
                                    };
                                    let current = sync::BookmarkSnapshot {
                                        version: 1,
                                        bookmarks: library.bookmarks.clone(),
                                        removed_bookmarks: library.removed_bookmarks.clone(),
                                    };
                                    let merged = sync::merge_snapshots(current, uploaded.clone());
                                    let needs_followup = merged != uploaded;
                                    let local_changed = library.bookmarks != merged.bookmarks
                                        || library.removed_bookmarks != merged.removed_bookmarks;
                                    if local_changed {
                                        library.bookmarks = merged.bookmarks;
                                        library.removed_bookmarks = merged.removed_bookmarks;
                                    }
                                    if !local_changed || library.save(&path).is_ok() {
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
                                            "Không lưu được dấu trang đã đồng bộ vào hồ sơ.",
                                        );
                                        self.notify_library(
                                            win_id,
                                            "Không lưu được dấu trang đã đồng bộ vào hồ sơ.",
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
                downloads::start(win_id, id, self.proxy.clone(), &self.settings);
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
                "Ctrl+Shift+T" => {
                    let target_url = if let Some(win) = self.windows.get_mut(&win_id) {
                        win.tab_manager.pop_last_closed()
                    } else {
                        None
                    };
                    if let Some(url) = target_url {
                        self.handle_create_tab(win_id, Some(url));
                    }
                }
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
                "Ctrl+F" => {
                    if let Some(win) = self.windows.get(&win_id) {
                        if let Some(chrome) = &win.chrome_webview {
                            let _ = chrome.evaluate_script(
                                "if (window.__toggleFindBar) { window.__toggleFindBar(); }",
                            );
                        }
                    }
                }
                "Ctrl+J" => {
                    if !self.settings.features.enable_downloads_manager {
                        return;
                    }
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        win.toggle_sidebar("downloads", &self.cert_cache, self.proxy.clone());
                    }
                }
                "Ctrl+H" => {
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        win.close_sidebar();
                    }
                    self.handle_create_tab(win_id, Some("evergreen://newtab?history".into()));
                }
                "Ctrl+Shift+O" => {
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        win.close_sidebar();
                    }
                    self.handle_create_tab(win_id, Some("evergreen://newtab?bookmarks".into()));
                }
                "Ctrl+D" => {
                    if let Some(page) = self
                        .windows
                        .get(&win_id)
                        .and_then(|win| win.tab_manager.active_tab())
                        .map(|tab| (tab.url.clone(), tab.title.clone()))
                    {
                        self.edit_library(win_id, |library| {
                            library.toggle_bookmark(&page.0, &page.1)
                        });
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
                "Ctrl+Tab" => {
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        let tabs = win.tab_manager.tabs();
                        if tabs.len() > 1 {
                            let current_idx = tabs
                                .iter()
                                .position(|t| {
                                    Some(t.id) == win.tab_manager.active_tab().map(|a| a.id)
                                })
                                .unwrap_or(0);
                            let next_idx = (current_idx + 1) % tabs.len();
                            let next_id = tabs[next_idx].id;
                            win.handle_switch_tab(next_id, &self.cert_cache);
                        }
                    }
                }
                "Ctrl+Shift+Tab" => {
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        let tabs = win.tab_manager.tabs();
                        if tabs.len() > 1 {
                            let current_idx = tabs
                                .iter()
                                .position(|t| {
                                    Some(t.id) == win.tab_manager.active_tab().map(|a| a.id)
                                })
                                .unwrap_or(0);
                            let prev_idx = if current_idx == 0 {
                                tabs.len() - 1
                            } else {
                                current_idx - 1
                            };
                            let prev_id = tabs[prev_idx].id;
                            win.handle_switch_tab(prev_id, &self.cert_cache);
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
                "Ctrl+Plus" => {
                    if let Some(win) = self.windows.get(&win_id) {
                        win.set_active_zoom(win.get_active_zoom() + 0.1);
                    }
                }
                "Ctrl+Minus" => {
                    if let Some(win) = self.windows.get(&win_id) {
                        win.set_active_zoom(win.get_active_zoom() - 0.1);
                    }
                }
                "Ctrl+Zero" => {
                    if let Some(win) = self.windows.get(&win_id) {
                        win.set_active_zoom(1.0);
                    }
                }
                "Escape" => {
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        if win.is_sidebar_open {
                            win.close_sidebar();
                            if let Some(c) = &win.chrome_webview {
                                let _ = c.evaluate_script("if (window.__syncSidebarState) window.__syncSidebarState(null);");
                            }
                        } else if let Some(active) = win.tab_manager.active_tab() {
                            if let Some(wv) = win.tabs.get(&active.id) {
                                let _ = wv.evaluate_script("window.stop()");
                            }
                        }
                    }
                }
                "F12" | "Ctrl+Shift+J" => {
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
                self.save_session(window_id);
                self.windows.remove(&window_id);
                if self.windows.is_empty() {
                    event_loop.exit();
                }
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
                            let target_url = if let Some(win) = self.windows.get_mut(&window_id) {
                                win.tab_manager.pop_last_closed()
                            } else {
                                None
                            };
                            if let Some(url) = target_url {
                                self.handle_create_tab(window_id, Some(url));
                            }
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
                        if let Some(win) = self.windows.get(&window_id) {
                            if let Some(chrome) = &win.chrome_webview {
                                let _ = chrome.evaluate_script(
                                    "if (window.__toggleFindBar) { window.__toggleFindBar(); }",
                                );
                            }
                        }
                    }
                    Key::Character(ref s)
                        if s.eq_ignore_ascii_case("j") && self.modifiers.control_key() =>
                    {
                        if let Some(win) = self.windows.get_mut(&window_id) {
                            win.toggle_sidebar("downloads", &self.cert_cache, self.proxy.clone());
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
                            if win.sidebar_webview.is_some() {
                                win.sidebar_webview = None;
                                win.is_sidebar_open = false;
                                win.update_layout();
                                if let Some(c) = &win.chrome_webview {
                                    let _ = c.evaluate_script("if (window.__syncSidebarState) window.__syncSidebarState(null);");
                                }
                            } else if let Some(active) = win.tab_manager.active_tab() {
                                if let Some(wv) = win.tabs.get(&active.id) {
                                    let _ = wv.evaluate_script("window.stop()");
                                }
                            }
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
                        if let Some(win) = self.windows.get(&window_id) {
                            win.set_active_zoom(win.get_active_zoom() + 0.1);
                        }
                    }
                    Key::Character(ref s)
                        if self.modifiers.control_key() && (s == "-" || s == "_") =>
                    {
                        if let Some(win) = self.windows.get(&window_id) {
                            win.set_active_zoom(win.get_active_zoom() - 0.1);
                        }
                    }
                    Key::Character(ref s) if self.modifiers.control_key() && s == "0" => {
                        if let Some(win) = self.windows.get(&window_id) {
                            win.set_active_zoom(1.0);
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
                    w!("Running Pip with Administrator privileges is prohibited for security reasons.\n\nPlease launch the browser as a standard user."),
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
                        w!("Pip cần Microsoft Edge WebView2 Runtime để mở trang web.\n\nBạn muốn mở trang tải chính thức của Microsoft? Sau khi cài đặt, hãy mở lại Pip."),
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

#[cfg(test)]
mod google_dns_preferences_tests {
    use super::*;

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
        assert!(!bookmark_sync_state_changed(&before, &after));
        after
            .removed_bookmarks
            .push("https://deleted.example/".into());
        assert!(bookmark_sync_state_changed(&before, &after));
        before.bookmarks.push(evergreen_core::library::Page {
            url: "https://another.example/".into(),
            title: "Another".into(),
        });
        assert!(bookmark_sync_state_changed(&before, &after));
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
        let defaults = dns_proxy_browser_args(None);
        assert!(defaults.contains("--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection"));
        assert!(!defaults.contains("--proxy-server="));

        let enabled = dns_proxy_browser_args(Some(43210));
        assert!(enabled.contains("--proxy-server=http://127.0.0.1:43210"));
        assert!(enabled.contains("--disable-quic"));
        assert!(enabled.contains("--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection"));
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
