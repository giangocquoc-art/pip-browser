use super::navigation::{allows_page_url, destination};
#[path = "adblock.rs"]
mod adblock;
use evergreen_core::tabs::{TabId, TabManager};
use serde::Deserialize;
use serde_json::json;
use std::{
    cell::Cell,
    collections::HashMap,
    rc::Rc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
    keyboard::{Key, ModifiersState},
    window::{Window, WindowId},
};
use wry::{
    dpi::{LogicalPosition, LogicalSize},
    NewWindowResponse, PageLoadEvent, PermissionResponse, Rect, WebView, WebViewBuilder,
};

const BAR_HEIGHT: f64 = 110.0;
const MAX_TABS: usize = 32;
const LOGO: &str = include_str!("../../ui/logo.b64");
const VERIFY_FIXTURE: &str = "<!doctype html><html><head><meta charset=\"utf-8\"><title>Pip runtime verification</title></head><body><p id=\"pip-runtime-probe\">pending</p></body></html>";

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "camelCase", deny_unknown_fields)]
enum Command {
    Ready,
    NewTab,
    Navigate { input: String },
    Select { id: u64 },
    Close { id: u64 },
    Back,
    Forward,
    Reload,
    VerifyToolbar { dom: bool },
}

#[derive(Debug)]
enum Event {
    Command(Command),
    Title(TabId, String),
    Load(TabId, String, bool),
    Notice(String),
    VerifyContent(String),
    VerifyTimeout,
    AdblockPrepared(Result<adblock::Prepared, String>),
    AdblockChanged,
}

#[derive(Default)]
struct Verification {
    toolbar_ready: bool,
    first_page_ready: bool,
    started: bool,
    tabs_checked: bool,
    toolbar_ipc: Option<bool>,
    content_dom: Option<bool>,
    content_ipc_absent: Option<bool>,
    failure: Option<String>,
    fixture_loaded: bool,
    adblock_requests: Option<bool>,
    adblock_stats: Option<adblock::Stats>,
}

impl Verification {
    fn complete(&self) -> bool {
        self.toolbar_ipc.is_some()
            && self.content_dom.is_some()
            && self.content_ipc_absent.is_some()
            && self.adblock_requests.is_some()
    }

    fn report(&self) -> serde_json::Value {
        let ok = self.complete()
            && self.failure.is_none()
            && self.tabs_checked
            && self.toolbar_ipc == Some(true)
            && self.content_dom == Some(true)
            && self.content_ipc_absent == Some(true);
        let ok = ok && self.adblock_requests == Some(true);
        json!({
            "mode": "pip-macos-native-ui",
            "ok": ok,
            "toolbarIpc": self.toolbar_ipc.unwrap_or(false),
            "contentDom": self.content_dom.unwrap_or(false),
            "contentIpcAbsent": self.content_ipc_absent.unwrap_or(false),
            "createSwitchCloseTabs": self.tabs_checked,
            "adblockRequests": self.adblock_requests.unwrap_or(false),
            "adblock": self.adblock_stats,
            "networkUsed": true,
            "networkScope": "loopback-only",
            "screenshotsUsed": false,
            "error": self.failure,
        })
    }
}

struct App {
    // Drop native views before their containing window.
    views: HashMap<TabId, WebView>,
    protected: HashMap<TabId, Rc<Cell<bool>>>,
    adblock: adblock::Blocker,
    toolbar: Option<WebView>,
    window: Option<Window>,
    tabs: TabManager,
    proxy: EventLoopProxy<Event>,
    modifiers: ModifiersState,
    verification: Option<Verification>,
    fixture: Option<adblock::Fixture>,
}

fn bounds(x: f64, y: f64, width: f64, height: f64) -> Rect {
    Rect {
        position: LogicalPosition::new(x, y).into(),
        size: LogicalSize::new(width.max(1.0), height.max(1.0)).into(),
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl App {
    fn content_bounds(&self) -> Rect {
        let window = self
            .window
            .as_ref()
            .expect("window initialized before views");
        let size = window.inner_size().to_logical::<f64>(window.scale_factor());
        bounds(0.0, BAR_HEIGHT, size.width, size.height - BAR_HEIGHT)
    }

    fn notice(&self, message: &str) {
        if let Some(toolbar) = &self.toolbar {
            let _ = toolbar.evaluate_script(&format!("window.pipNotice({});", json!(message)));
        }
    }

    fn sync(&mut self) {
        for tab in self.tabs.tabs_mut() {
            if let Some(view) = self.views.get(&tab.id) {
                if let Ok(url) = view.url() {
                    if allows_page_url(&url) && tab.url != "evergreen://newtab" {
                        tab.url = url.clone();
                    }
                }
                tab.can_go_back = view.can_go_back().unwrap_or(false);
                tab.can_go_forward = view.can_go_forward().unwrap_or(false);
            }
        }
        let active_id = self.tabs.active_tab_id();
        for (id, view) in &self.views {
            let _ = view.set_visible(Some(*id) == active_id);
        }
        if let Some(toolbar) = &self.toolbar {
            let active = self.tabs.active_tab();
            let state = json!({
                "tabs": self.tabs.tabs().iter().map(|tab| json!({"id":tab.id.0,"title":tab.title,"loading":tab.is_loading})).collect::<Vec<_>>(),
                "activeId": active_id.map(|id| id.0),
                "url": active.map(|tab| if tab.url == "evergreen://newtab" { "" } else { &tab.url }).unwrap_or(""),
                "back": active.is_some_and(|tab| tab.can_go_back),
                "forward": active.is_some_and(|tab| tab.can_go_forward),
                "adblock": self.adblock.message(),
                "adblockReady": self.adblock.ready(),
            });
            let _ = toolbar.evaluate_script(&format!("window.pipState({state});"));
        }
        if let Some(window) = &self.window {
            let title = self
                .tabs
                .active_tab()
                .map_or("Pip", |tab| tab.title.as_str());
            window.set_title(&format!("{title} — Pip"));
        }
    }

    fn new_tab(&mut self) -> wry::Result<()> {
        if self.views.len() >= MAX_TABS {
            self.notice("Đã mở 32 tab. Đóng bớt tab để giữ Pip nhẹ.");
            return Ok(());
        }
        let id = self.tabs.create_tab("evergreen://newtab", now());
        let title_proxy = self.proxy.clone();
        let load_proxy = self.proxy.clone();
        let popup_proxy = self.proxy.clone();
        let download_proxy = self.proxy.clone();
        let navigation_proxy = self.proxy.clone();
        let protected = Rc::new(Cell::new(false));
        let navigation_protected = protected.clone();
        let home = if self.verification.is_some() {
            VERIFY_FIXTURE.to_string()
        } else {
            include_str!("home.html").replace("{{LOGO}}", LOGO.trim())
        };
        let result = WebViewBuilder::new()
            .with_bounds(self.content_bounds())
            .with_html(home)
            .with_back_forward_navigation_gestures(true)
            .with_devtools(cfg!(debug_assertions))
            // The preview has no permission picker yet; never implicitly grant
            // camera/microphone access or trigger missing macOS usage prompts.
            .with_permission_handler(|_| PermissionResponse::Deny)
            .with_navigation_handler(move |url| {
                if url == "about:blank" {
                    return true;
                }
                if !navigation_protected.get() {
                    let _ = navigation_proxy.send_event(Event::Notice(
                        "Bộ lọc chưa sẵn sàng. Pip giữ trang web đóng để tránh tải thiếu bảo vệ."
                            .into(),
                    ));
                    return false;
                }
                if adblock::blocks_navigation(&url) {
                    let _ = navigation_proxy.send_event(Event::Notice(
                        "Pip đã chặn điều hướng tới tên miền cờ bạc trong danh sách bảo vệ.".into(),
                    ));
                    return false;
                }
                allows_page_url(&url)
            })
            .with_document_title_changed_handler(move |title| {
                let _ = title_proxy.send_event(Event::Title(id, title));
            })
            .with_on_page_load_handler(move |event, url| {
                let _ = load_proxy.send_event(Event::Load(
                    id,
                    url,
                    matches!(event, PageLoadEvent::Started),
                ));
            })
            .with_new_window_req_handler(move |_, _| {
                let _ = popup_proxy.send_event(Event::Notice(
                    "Pip đã chặn cửa sổ tự bật. Mở liên kết trong tab hiện tại.".into(),
                ));
                NewWindowResponse::Deny
            })
            .with_download_started_handler(move |_, _| {
                let _ = download_proxy.send_event(Event::Notice(
                    "Tải tệp chưa được hỗ trợ trong bản macOS thử nghiệm.".into(),
                ));
                false
            })
            // Remote pages deliberately get no native IPC handler.
            .build_as_child(self.window.as_ref().expect("window initialized"));
        match result {
            Ok(view) => {
                protected.set(self.adblock.attach(&view));
                self.protected.insert(id, protected);
                self.views.insert(id, view);
            }
            Err(error) => {
                self.tabs.close_tab(id);
                return Err(error);
            }
        }
        self.sync();
        Ok(())
    }

    fn execute(&mut self, command: Command) {
        let active = self.tabs.active_tab_id();
        let result = match command {
            Command::Ready => {
                self.sync();
                return;
            }
            Command::VerifyToolbar { .. } => return,
            Command::NewTab => self.new_tab(),
            Command::Navigate { input } => match destination(&input) {
                Ok(url) => {
                    if let Some(id) = active {
                        if let Some(view) = self.views.get(&id) {
                            let result = if url == "evergreen://newtab" {
                                view.load_html(
                                    &include_str!("home.html").replace("{{LOGO}}", LOGO.trim()),
                                )
                            } else {
                                if !self.protected.get(&id).is_some_and(|state| state.get()) {
                                    self.notice(&self.adblock.message());
                                    return;
                                }
                                if adblock::blocks_navigation(&url) {
                                    self.notice(
                                        "Pip đã chặn tên miền cờ bạc trong danh sách bảo vệ.",
                                    );
                                    return;
                                }
                                view.load_url(&url)
                            };
                            if result.is_ok() {
                                self.tabs.update_url(id, url);
                            }
                            result
                        } else {
                            Ok(())
                        }
                    } else {
                        Ok(())
                    }
                }
                Err(message) => {
                    self.notice(&message);
                    return;
                }
            },
            Command::Select { id } => {
                self.tabs.switch_tab(TabId(id), now());
                Ok(())
            }
            Command::Close { id } => {
                self.views.remove(&TabId(id));
                self.protected.remove(&TabId(id));
                self.tabs.close_tab(TabId(id));
                if self.views.is_empty() {
                    self.new_tab()
                } else {
                    Ok(())
                }
            }
            Command::Back => active
                .and_then(|id| self.views.get(&id))
                .map_or(Ok(()), WebView::go_back),
            Command::Forward => active
                .and_then(|id| self.views.get(&id))
                .map_or(Ok(()), WebView::go_forward),
            Command::Reload => active
                .and_then(|id| self.views.get(&id))
                .map_or(Ok(()), WebView::reload),
        };
        if let Err(error) = result {
            self.notice(&format!("Không thể thực hiện: {error}"));
        }
        self.sync();
    }

    fn verify_if_ready(&mut self, event_loop: &ActiveEventLoop) {
        let ready = self.verification.as_ref().is_some_and(|verification| {
            verification.toolbar_ready
                && verification.first_page_ready
                && verification.fixture_loaded
                && self.adblock.ready()
                && !verification.started
        });
        if !ready {
            return;
        }
        self.verification
            .as_mut()
            .expect("verification mode")
            .started = true;
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            let first = self
                .tabs
                .active_tab_id()
                .ok_or("first native tab missing")?;
            self.new_tab()?;
            let second = self
                .tabs
                .active_tab_id()
                .ok_or("second native tab missing")?;
            if first == second || self.views.len() != 2 || self.tabs.tabs().len() != 2 {
                return Err("native tab creation failed".into());
            }
            self.execute(Command::Select { id: first.0 });
            if self.tabs.active_tab_id() != Some(first) {
                return Err("native tab switch failed".into());
            }
            self.execute(Command::Close { id: second.0 });
            if self.views.len() != 1
                || self.tabs.tabs().len() != 1
                || self.tabs.active_tab_id() != Some(first)
            {
                return Err("native tab close failed".into());
            }
            self.verification
                .as_mut()
                .expect("verification mode")
                .tabs_checked = true;
            let proxy = self.proxy.clone();
            self.views.get(&first).ok_or("first native view disappeared")?
                .evaluate_script_with_callback(
                    r#"(() => {
                        const element = document.querySelector('#pip-runtime-probe');
                        if (element) {
                            element.textContent = String(6 * 7);
                            element.dataset.changed = 'native-evaluation';
                        }
                        return {
                            dom: !!element && element.textContent === '42' && element.dataset.changed === 'native-evaluation',
                            contentIpcAbsent: !(window.ipc && typeof window.ipc.postMessage === 'function'),
                            adblockRequests: window.__pipAllowed === true && window.__pipException === true && window.__pipBlocked !== true
                        };
                    })()"#,
                    move |result| { let _ = proxy.send_event(Event::VerifyContent(result)); },
                )?;
            self.toolbar.as_ref().ok_or("native toolbar missing")?
                .evaluate_script(
                    r#"window.ipc.postMessage(JSON.stringify({
                        action: 'verifyToolbar',
                        dom: !!document.querySelector('#address') && typeof window.pipState === 'function'
                    }));"#,
                )?;
            Ok(())
        })();
        if let Err(error) = result {
            self.verification
                .as_mut()
                .expect("verification mode")
                .failure = Some(error.to_string());
            event_loop.exit();
        }
    }

    fn finish_verification(&self, event_loop: &ActiveEventLoop) {
        if self
            .verification
            .as_ref()
            .is_some_and(Verification::complete)
        {
            event_loop.exit();
        }
    }
}

impl ApplicationHandler<Event> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title("Pip")
            .with_inner_size(winit::dpi::LogicalSize::new(1150.0, 780.0));
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            let window = event_loop.create_window(attributes)?;
            let size = window.inner_size().to_logical::<f64>(window.scale_factor());
            let proxy = self.proxy.clone();
            let html = include_str!("toolbar.html").replace("{{LOGO}}", LOGO.trim());
            let toolbar = WebViewBuilder::new()
                .with_bounds(bounds(0.0, 0.0, size.width, BAR_HEIGHT))
                .with_html(html)
                .with_navigation_handler(|url| url == "about:blank")
                .with_new_window_req_handler(|_, _| NewWindowResponse::Deny)
                .with_ipc_handler(move |request| {
                    if request.body().len() <= 8192 {
                        if let Ok(command) = serde_json::from_str::<Command>(request.body()) {
                            let _ = proxy.send_event(Event::Command(command));
                        }
                    }
                })
                .build_as_child(&window)?;
            self.toolbar = Some(toolbar);
            self.window = Some(window);
            self.new_tab()?;
            Ok(())
        })();
        if let Err(error) = result {
            eprintln!("Pip macOS failed to start: {error}");
            if let Some(verification) = &mut self.verification {
                verification.failure = Some(error.to_string());
            }
            event_loop.exit();
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Event) {
        match event {
            Event::Command(Command::Ready) => {
                self.execute(Command::Ready);
                if let Some(verification) = &mut self.verification {
                    verification.toolbar_ready = true;
                }
                self.verify_if_ready(event_loop);
            }
            Event::Command(Command::VerifyToolbar { dom }) => {
                if let Some(verification) = &mut self.verification {
                    verification.toolbar_ipc = Some(dom);
                }
                self.finish_verification(event_loop);
            }
            Event::Command(command) => self.execute(command),
            Event::Title(id, title) => {
                self.tabs
                    .update_title(id, title.chars().take(256).collect());
                self.sync();
            }
            Event::Load(id, url, loading) => {
                if let Some(tab) = self.tabs.tabs_mut().iter_mut().find(|tab| tab.id == id) {
                    tab.is_loading = loading;
                    if allows_page_url(&url) && url != "about:blank" {
                        tab.url = url.clone();
                    }
                }
                self.sync();
                if let Some(verification) = &mut self.verification {
                    if !verification.started && !loading && self.tabs.active_tab_id() == Some(id) {
                        verification.first_page_ready = true;
                        if self
                            .fixture
                            .as_ref()
                            .is_some_and(|fixture| fixture.url == url)
                        {
                            verification.fixture_loaded = true;
                        }
                    }
                }
                self.verify_if_ready(event_loop);
            }
            Event::Notice(message) => self.notice(&message),
            Event::VerifyContent(result) => {
                if let Some(verification) = &mut self.verification {
                    let parsed = serde_json::from_str::<serde_json::Value>(&result);
                    match parsed {
                        Ok(value) => {
                            verification.content_dom = Some(
                                value.get("dom").and_then(serde_json::Value::as_bool) == Some(true),
                            );
                            verification.content_ipc_absent = Some(
                                value
                                    .get("contentIpcAbsent")
                                    .and_then(serde_json::Value::as_bool)
                                    == Some(true),
                            );
                            verification.adblock_requests = Some(
                                value
                                    .get("adblockRequests")
                                    .and_then(serde_json::Value::as_bool)
                                    == Some(true)
                                    && self
                                        .fixture
                                        .as_ref()
                                        .is_some_and(adblock::Fixture::network_passed),
                            );
                            verification.adblock_stats = self.adblock.stats();
                        }
                        Err(error) => {
                            verification.content_dom = Some(false);
                            verification.content_ipc_absent = Some(false);
                            verification.adblock_requests = Some(false);
                            verification.failure =
                                Some(format!("native DOM callback was not JSON: {error}"));
                        }
                    }
                }
                self.finish_verification(event_loop);
            }
            Event::VerifyTimeout => {
                if let Some(verification) = &mut self.verification {
                    if !verification.complete() {
                        verification.failure =
                            Some("native UI verification timed out after 90 seconds".into());
                        event_loop.exit();
                    }
                }
            }
            Event::AdblockPrepared(result) => match result {
                Ok(prepared) => {
                    let proxy = self.proxy.clone();
                    self.adblock.compile(prepared, move || {
                        let _ = proxy.send_event(Event::AdblockChanged);
                    });
                    self.sync();
                }
                Err(error) => {
                    self.adblock.fail(error);
                    let _ = self.proxy.send_event(Event::AdblockChanged);
                }
            },
            Event::AdblockChanged => {
                if self.adblock.ready() {
                    for (id, view) in &self.views {
                        if let Some(protected) = self.protected.get(id) {
                            if !protected.get() {
                                protected.set(self.adblock.attach(view));
                            }
                        }
                    }
                    if let (Some(fixture), Some(id)) = (&self.fixture, self.tabs.active_tab_id()) {
                        if let Some(view) = self.views.get(&id) {
                            if let Err(error) = view.load_url(&fixture.url) {
                                if let Some(verification) = &mut self.verification {
                                    verification.failure = Some(error.to_string());
                                }
                                event_loop.exit();
                            }
                        }
                    }
                } else {
                    self.notice(&self.adblock.message());
                    if let Some(verification) = &mut self.verification {
                        verification.failure = Some(self.adblock.message());
                        event_loop.exit();
                    }
                }
                self.sync();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::KeyboardInput { event, .. }
                if event.state.is_pressed() && !event.repeat && self.modifiers.super_key() =>
            {
                if let Key::Character(key) = event.logical_key {
                    match key.to_ascii_lowercase().as_str() {
                        "t" => self.execute(Command::NewTab),
                        "w" => {
                            if let Some(id) = self.tabs.active_tab_id() {
                                self.execute(Command::Close { id: id.0 });
                            }
                        }
                        "r" => self.execute(Command::Reload),
                        "l" => {
                            if let Some(toolbar) = &self.toolbar {
                                let _ = toolbar.focus();
                                let _ = toolbar.evaluate_script("document.querySelector('#address').focus();document.querySelector('#address').select();");
                            }
                        }
                        "q" => event_loop.exit(),
                        _ => {}
                    }
                }
            }
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(window) = &self.window {
                    let size = window.inner_size().to_logical::<f64>(window.scale_factor());
                    if let Some(toolbar) = &self.toolbar {
                        let _ = toolbar.set_bounds(bounds(0.0, 0.0, size.width, BAR_HEIGHT));
                    }
                    for view in self.views.values() {
                        let _ = view.set_bounds(bounds(
                            0.0,
                            BAR_HEIGHT,
                            size.width,
                            size.height - BAR_HEIGHT,
                        ));
                    }
                }
            }
            _ => {}
        }
    }
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let verify_ui = std::env::args_os().any(|argument| argument == "--verify-ui");
    let event_loop = EventLoop::<Event>::with_user_event().build()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App {
        views: HashMap::new(),
        protected: HashMap::new(),
        adblock: adblock::Blocker::new(),
        toolbar: None,
        window: None,
        tabs: TabManager::new(),
        proxy: event_loop.create_proxy(),
        modifiers: ModifiersState::empty(),
        verification: verify_ui.then(Verification::default),
        fixture: if verify_ui {
            Some(adblock::Fixture::start()?)
        } else {
            None
        },
    };
    let adblock_proxy = app.proxy.clone();
    std::thread::spawn(move || {
        let _ = adblock_proxy.send_event(Event::AdblockPrepared(adblock::prepare(verify_ui)));
    });
    if verify_ui {
        let proxy = app.proxy.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(90));
            let _ = proxy.send_event(Event::VerifyTimeout);
        });
    }
    event_loop.run_app(&mut app)?;
    if let Some(verification) = app.verification {
        let report = verification.report();
        println!("{report}");
        if report.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
            return Err("Pip macOS native UI verification failed".into());
        }
    }
    Ok(())
}
