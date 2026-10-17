#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Pip — Main Application Entrypoint

mod accelerator;
mod adblock;
mod cert;
mod chrome;
#[cfg(target_os = "windows")]
mod doh_proxy;
mod downloads;
mod error_ui;
#[cfg(any(test, debug_assertions))]
mod headless_ui_tests;
mod home_ui;
mod loading_ui;
mod settings_ui;
mod sidebar_ui;
mod window;

use chrome::{create_bounds, get_chrome_html, normalize_url, CHROME_HEIGHT};
use evergreen_core::env::{detect_webview2_runtime, is_process_elevated};
use evergreen_core::ipc::{HostToUiMessage, UiToHostMessage};
use evergreen_core::tabs::{TabId, TabManager, TabState};
use evergreen_core::updater::run_local_command;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
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
    HistoryChanged(WindowId, TabId, bool, bool),
    CommandDone(String, bool, String),
    BrowsingDataClearDone(WindowId, bool, bool),
    Shortcut(WindowId, String),
    DownloadRequested(WindowId, u64),
    DownloadProgress {
        window_id: WindowId,
        download_id: u64,
        filename: String,
        received_bytes: i64,
        total_bytes: i64,
        state: String,
    },
    PermissionPrompt {
        window_id: WindowId,
        permission_id: u64,
        origin: String,
        permission_kind: String,
    },
    TabCrashed(WindowId, TabId, i32),
    ServerCertificateError {
        window_id: WindowId,
        tab_id: TabId,
        request_uri: String,
        error_status: i32,
    },
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
                .with_bounds(sidebar_bounds);
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
            let url = active.map(|t| t.url.clone()).unwrap_or_default();
            let sec_info = cert_cache.query_or_default(&url);

            let sync_msg = HostToUiMessage::SidebarStateSync {
                open: self.is_sidebar_open,
                mode: self.sidebar_mode.clone(),
                security_info: Some(Box::new(sec_info)),
            };

            if let Ok(json_str) = serde_json::to_string(&sync_msg) {
                let script = format!(
                    "if(window.__syncDownloads)window.__syncDownloads({snapshot});if(window.__renderPrivacy)window.__renderPrivacy({private});if(window.__sidebarSync)window.__sidebarSync({json_str});"
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

fn get_settings_path() -> std::path::PathBuf {
    get_data_directory().join("settings.json")
}

fn dns_proxy_browser_args(port: Option<u16>) -> String {
    port.map(|port| format!("--proxy-server=http://127.0.0.1:{port}"))
        .unwrap_or_default()
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
    doh_proxy: Option<doh_proxy::DohProxy>,
    next_tab_id: AtomicU64,
    allowed_cert_hosts: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
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

    fn save_session(&self, win_id: WindowId) {
        let Some(win) = self.windows.get(&win_id) else {
            return;
        };
        if win.private {
            return;
        }
        let directory = get_data_directory().join("sessions");
        let tabs = win.tab_manager.snapshot();
        if let Ok(bytes) = serde_json::to_vec(&tabs) {
            if std::fs::create_dir_all(&directory).is_ok() {
                let path = directory.join(format!("{}.json", win.profile_id));
                let staging = path.with_extension("json.tmp");
                if let Err(error) =
                    std::fs::write(&staging, bytes).and_then(|_| std::fs::rename(staging, path))
                {
                    eprintln!("Save session: {error}");
                }
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
        for win in self.windows.values() {
            if let Some(chrome) = &win.chrome_webview {
                let _ = chrome.evaluate_script(&format!(
                    "if(window.__renderGoogleDns) window.__renderGoogleDns({enabled},{active},{failed});"
                ));
            }
            for (id, view) in &win.tabs {
                if win
                    .tab_manager
                    .get_tab_by_id(*id)
                    .is_some_and(|tab| tab.url == "evergreen://settings")
                {
                    let _ = view.evaluate_script(&format!(
                        "if(window.__renderGoogleDns) window.__renderGoogleDns({enabled},{active},{failed});"
                    ));
                }
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

    fn sync_profiles(&self) {
        let json = serde_json::to_string(&self.settings.profiles).unwrap();
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
            doh_proxy,
            settings,
            next_tab_id: AtomicU64::new(1),
            allowed_cert_hosts: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
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
            downloads: HashMap::new(),
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
            .with_bounds(chrome_bounds);
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

        let mut builder = WebViewBuilder::new()
            .with_additional_browser_args(dns_proxy_browser_args(context.dns_proxy_port))
            .with_bounds(bounds)
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
        let data = serde_json::json!({"bookmarks":library.bookmarks,"history":if win.private {vec![]}else{library.history},"private":win.private});
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
    fn edit_library(
        &self,
        win_id: WindowId,
        edit: impl FnOnce(&mut evergreen_core::library::Library),
    ) {
        let Some(path) = self.library_path(win_id) else {
            return;
        };
        let result = (|| -> std::io::Result<()> {
            let mut library = evergreen_core::library::Library::load(&path)?;
            edit(&mut library);
            library.save(&path)
        })();
        if let Err(error) = result {
            if let Some(win) = self.windows.get(&win_id) {
                win.window
                    .set_title(&format!("Pip — Không lưu được thư viện: {error}"));
            }
            return;
        }
        let profile = &self.windows.get(&win_id).unwrap().profile_id;
        for win in self.windows.values().filter(|w| &w.profile_id == profile) {
            self.sync_library(win.id);
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
            Ok(target) => {
                if let Some(win) = self.windows.get_mut(&win_id) {
                    if let Some(id) = win.tab_manager.active_tab_id() {
                        if let Some(view) = win.tabs.get(&id) {
                            // SourceChanged is the authoritative committed URL.
                            let _ = view.load_url(&target);
                        }
                    }
                }
            }
            Err(error) => eprintln!("Navigation error: {error}"),
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
        if self.windows.is_empty() {
            self.create_browser_window(event_loop, None);
        }
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
                }
                UiToHostMessage::GetLibrary => self.sync_library(win_id),
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
                    self.edit_library(win_id, |library| library.bookmarks.retain(|p| p.url != url))
                }
                UiToHostMessage::ClearHistory => {
                    self.edit_library(win_id, |library| library.history.clear())
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
                    self.save_session(win_id);
                    self.windows.remove(&win_id);
                    if self.windows.is_empty() {
                        event_loop.exit();
                    }
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
                UiToHostMessage::TogglePinTab => {
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        if let Some(id) = win.tab_manager.active_tab_id() {
                            win.tab_manager.toggle_pin(id);
                            win.sync_ui_state();
                        }
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
                action @ (UiToHostMessage::ClearSavedPasswords | UiToHostMessage::ClearCache) => {
                    let cache = matches!(action, UiToHostMessage::ClearCache);
                    use webview2_com::Microsoft::Web::WebView2::Win32::*;
                    use windows::core::Interface;
                    use wry::WebViewExtWindows;
                    if let Some(win) = self.windows.get(&win_id).filter(|w| !w.private) {
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
                                                        cache,
                                                        result.is_ok(),
                                                    ),
                                                );
                                                Ok(())
                                            }),
                                        );
                                    let kind = if cache {
                                        COREWEBVIEW2_BROWSING_DATA_KINDS_DISK_CACHE
                                    } else {
                                        COREWEBVIEW2_BROWSING_DATA_KINDS_PASSWORD_AUTOSAVE
                                    };
                                    if profile.ClearBrowsingData(kind, &callback).is_err() {
                                        let _ = self.proxy.send_event(
                                            BrowserEvent::BrowsingDataClearDone(
                                                win_id, cache, false,
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
                } => downloads::open(win_id, download_id, folder),
                UiToHostMessage::ClearDownloads => {
                    downloads::clear_finished(win_id);
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        win.downloads.retain(|_, d| d["state"] == "InProgress");
                        win.update_sidebar_sync(&self.cert_cache);
                    }
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
            BrowserEvent::BrowsingDataClearDone(win_id, cache, success) => {
                let item = if cache {
                    "bộ nhớ đệm"
                } else {
                    "mật khẩu đã lưu"
                };
                self.notify_settings(
                    win_id,
                    &format!(
                        "{} {item}{}",
                        if success {
                            "Đã xóa"
                        } else {
                            "Không thể xóa"
                        },
                        if success && cache {
                            "; đăng nhập và lịch sử được giữ lại."
                        } else {
                            "."
                        }
                    ),
                );
            }
            BrowserEvent::DownloadRequested(win_id, id) => {
                downloads::start(win_id, id, self.proxy.clone(), &self.settings);
            }
            BrowserEvent::DownloadProgress {
                window_id,
                download_id,
                filename,
                received_bytes,
                total_bytes,
                state,
            } => {
                let json = serde_json::json!({
                    "download_id": download_id,
                    "filename": filename,
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
                                let _ = wv.evaluate_script("window.print()");
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
                                    let _ = wv.evaluate_script("window.print()");
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

    Ok(())
}

#[cfg(test)]
mod google_dns_preferences_tests {
    use super::*;

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
