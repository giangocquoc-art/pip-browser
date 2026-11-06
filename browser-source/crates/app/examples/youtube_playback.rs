//! Native WebView2 playback check in an isolated profile, without a debugging port.
#[allow(dead_code)]
#[path = "../src/adblock.rs"]
mod adblock;
#[allow(dead_code)]
#[path = "../src/adblock_updates.rs"]
mod adblock_updates;
#[allow(dead_code)]
#[path = "../src/doh_proxy.rs"]
mod doh_proxy;
use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowId},
};
use wry::WebViewBuilderExtWindows;

struct Check {
    window: Option<Window>,
    view: Option<wry::WebView>,
    blocker: Rc<RefCell<adblock::Blocker>>,
    started: Instant,
    next: Instant,
    samples: Vec<serde_json::Value>,
    proxy: winit::event_loop::EventLoopProxy<String>,
    urls: Vec<String>,
    index: usize,
    enabled: bool,
}
impl ApplicationHandler<String> for Check {
    fn resumed(&mut self, events: &ActiveEventLoop) {
        if self.view.is_some() {
            return;
        }
        let window = events
            .create_window(
                Window::default_attributes()
                    .with_title("Pip — Kiểm tra phát YouTube")
                    .with_inner_size(winit::dpi::LogicalSize::new(1100., 720.)),
            )
            .unwrap();
        let view = wry::WebViewBuilder::new()
            .with_additional_browser_args("--autoplay-policy=no-user-gesture-required")
            .build(&window)
            .unwrap();
        if self.enabled {
            adblock::attach(&view, self.blocker.clone()).unwrap();
        }
        view.load_url(&self.urls[0]).unwrap();
        self.started = Instant::now();
        self.next = self.started + Duration::from_secs(5);
        self.window = Some(window);
        self.view = Some(view);
    }
    fn window_event(&mut self, events: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        if matches!(event, WindowEvent::CloseRequested) {
            events.exit();
        }
    }
    fn about_to_wait(&mut self, events: &ActiveEventLoop) {
        if Instant::now() >= self.next {
            self.next = Instant::now() + Duration::from_secs(5);
            let proxy = self.proxy.clone();
            self.view.as_ref().unwrap().evaluate_script_with_callback(r#"(()=>{const v=document.querySelector('video'),p=document.getElementById('movie_player'),r=p?.getPlayerResponse?.();if(v){v.muted=true;if(v.paused)v.play().catch(()=>{});}return JSON.stringify({url:location.href,title:document.title,video:!!v,time:v?.currentTime,duration:v?.duration,ready:v?.readyState,paused:v?.paused,ad:!!p?.classList.contains('ad-showing'),status:r?.playabilityStatus?.status,error:document.querySelector('#error-screen')?.innerText,videoId:r?.videoDetails?.videoId,initialAds:window.ytInitialPlayerResponse?.adPlacements?.length,visibleSponsored:[...document.querySelectorAll('ytd-ad-slot-renderer')].filter(e=>e.getBoundingClientRect().height>0&&getComputedStyle(e).display!=='none').length});})()"#, move|value|{let _=proxy.send_event(value);}).unwrap();
        }
        events.set_control_flow(ControlFlow::WaitUntil(self.next));
    }
    fn user_event(&mut self, events: &ActiveEventLoop, value: String) {
        let data: serde_json::Value = serde_json::from_str(&value)
            .ok()
            .and_then(|v: serde_json::Value| v.as_str().and_then(|s| serde_json::from_str(s).ok()))
            .unwrap_or(serde_json::Value::Null);
        println!("{}s {}", self.started.elapsed().as_secs(), data);
        self.samples.push(data);
        if self.started.elapsed() > Duration::from_secs(35) {
            self.index += 1;
            if self.index == self.urls.len() {
                events.exit();
                return;
            }
            self.view
                .as_ref()
                .unwrap()
                .load_url(&self.urls[self.index])
                .unwrap();
            self.started = Instant::now();
        }
    }
}
fn main() {
    let enabled = !std::env::args().any(|a| a == "--no-block");
    let directory = std::env::temp_dir().join(format!("pip-youtube-{}", std::process::id()));
    std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", directory.join("webview"));
    let events = EventLoop::<String>::with_user_event().build().unwrap();
    let mut check = Check {
        window: None,
        view: None,
        blocker: Rc::new(RefCell::new(adblock::Blocker::load(
            directory,
            evergreen_core::settings::AdblockMode::Balanced,
        ))),
        started: Instant::now(),
        next: Instant::now(),
        samples: vec![],
        proxy: events.create_proxy(),
        urls: vec![
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ".into(),
            "https://www.youtube.com/watch?v=aqz-KE-bpKQ".into(),
        ],
        index: 0,
        enabled,
    };
    events.run_app(&mut check).unwrap();
    let artifact = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../target/youtube-{}-verification.json",
        if enabled { "blocked" } else { "baseline" }
    ));
    std::fs::write(artifact,serde_json::to_vec_pretty(&serde_json::json!({"enabled":enabled,"blocked":check.blocker.borrow().blocked,"samples":check.samples})).unwrap()).unwrap();
}
