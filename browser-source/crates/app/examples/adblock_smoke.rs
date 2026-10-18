//! cargo run -p evergreen-browser --example adblock_smoke --release
//! Real WebView2 interception + persisted site exception, against loopback only.
#[allow(dead_code)]
#[path = "../src/adblock.rs"]
mod adblock;
#[allow(dead_code)]
#[path = "../src/adblock_updates.rs"]
mod adblock_updates;
use std::{
    cell::RefCell,
    collections::HashMap,
    io::{Read, Write},
    net::TcpListener,
    rc::Rc,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
    window::{Window, WindowId},
};
use wry::WebViewBuilderExtWindows;

struct Smoke {
    proxy: EventLoopProxy<String>,
    blocker: Rc<RefCell<adblock::Blocker>>,
    window: Option<Window>,
    view: Option<wry::WebView>,
    url: String,
    phase: u8,
    deadline: Instant,
    requests: Arc<Mutex<HashMap<String, u32>>>,
}
impl ApplicationHandler<String> for Smoke {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let window = event_loop
            .create_window(Window::default_attributes().with_visible(false))
            .unwrap();
        let proxy = self.proxy.clone();
        let view = wry::WebViewBuilder::new()
            // Test-only loopback host aliases: EasyList exempts cosmetic rules on localhost.
            .with_additional_browser_args("--host-resolver-rules=\"MAP pip-adblock.test 127.0.0.1, MAP frame.pip-adblock.test 127.0.0.1\" --no-proxy-server")
            .with_ipc_handler(move |req| {
                let _ = proxy.send_event(req.body().clone());
            })
            .build_as_child(&window)
            .unwrap();
        adblock::attach(&view, self.blocker.clone()).unwrap();
        view.load_url(&self.url).unwrap();
        self.view = Some(view);
        self.window = Some(window);
    }
    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        assert!(
            Instant::now() < self.deadline,
            "WebView2 smoke test timed out at phase {}: {:?}",
            self.phase,
            self.requests.lock().unwrap()
        );
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.deadline));
    }
    fn user_event(&mut self, event_loop: &ActiveEventLoop, message: String) {
        let value: serde_json::Value = serde_json::from_str(&message).unwrap();
        println!("phase {} report {value}", self.phase);
        if self.phase >= 2 {
            let enabled = self.phase != 3;
            assert_eq!(
                value["initial_pruned"], enabled,
                "inline player config: {value}"
            );
            assert_eq!(
                value["fetch_pruned"], enabled,
                "fetch player config: {value}"
            );
            assert_eq!(value["normal_video"], "kept", "content data must survive");
            if self.phase == 4 {
                self.phase = 5;
                println!("PASS: YouTube upstream scriptlets ran before inline page scripts, pruned player ads from fetch, preserved content, and respected OFF/ON after refresh.");
                event_loop.exit();
            } else {
                assert_eq!(
                    self.blocker
                        .borrow_mut()
                        .toggle("https://www.youtube.com/")
                        .unwrap(),
                    Some(!enabled)
                );
                self.phase += 1;
                self.youtube_fixture();
            }
            return;
        }
        assert_eq!(value["normal"], "ok");
        assert_eq!(value["media"], "ok");
        assert_eq!(value["ad"], self.phase == 1);
        assert_eq!(value["frame_ad"], self.phase == 1);
        assert_eq!(value["cosmetic_hidden"], self.phase == 0);
        assert_eq!(
            self.blocker.borrow().blocked,
            3,
            "top/frame ad scripts and ad media must be intercepted"
        );
        let requests = self.requests.lock().unwrap();
        assert_eq!(
            requests.get("/ads/cbr.js").copied().unwrap_or(0),
            u32::from(self.phase) * 2
        );
        assert_eq!(
            requests.get("/adserver.mp4").copied().unwrap_or(0),
            u32::from(self.phase)
        );
        assert_eq!(
            requests.get("/normal.mp4").copied().unwrap_or(0),
            u32::from(self.phase) + 1
        );
        drop(requests);
        if self.phase == 0 {
            assert_eq!(
                self.blocker.borrow_mut().toggle(&self.url).unwrap(),
                Some(false)
            );
            self.phase = 1;
            adblock::refresh_scriptlets(self.view.as_ref().unwrap(), &self.blocker).unwrap();
            self.view.as_ref().unwrap().load_url(&self.url).unwrap();
        } else {
            self.phase = 2;
            println!("PASS: native Brave blocked top/frame ad scripts and ad media before network; regular media/manifest passed; cosmetic CSS hid the ad; site exception restored all requests and CSS.");
            self.youtube_fixture();
        }
    }
}
impl Smoke {
    fn youtube_fixture(&mut self) {
        self.view = None;
        // HSTS prevents a plain HTTP YouTube fixture. Only in this test, substitute
        // the hostname guard in the exact engine-produced script; source JS is unchanged.
        let script = self
            .blocker
            .borrow()
            .youtube_script()
            .replace("case \"www.youtube.com\"", "case \"pip-adblock.test\"");
        let proxy = self.proxy.clone();
        let view = wry::WebViewBuilder::new()
            .with_additional_browser_args("--host-resolver-rules=\"MAP pip-adblock.test 127.0.0.1, MAP frame.pip-adblock.test 127.0.0.1\" --no-proxy-server")
            .with_initialization_script(&script)
            .with_ipc_handler(move |req| { let _=proxy.send_event(req.body().clone()); })
            .build_as_child(self.window.as_ref().unwrap()).unwrap();
        view.load_url(&(self.url.clone() + "youtube-test")).unwrap();
        self.view = Some(view);
    }
}
fn main() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!(
        "http://pip-adblock.test:{}/",
        listener.local_addr().unwrap().port()
    );
    let requests = Arc::new(Mutex::new(HashMap::<String, u32>::new()));
    let server_requests = requests.clone();
    std::thread::spawn(move || {
        for mut socket in listener.incoming().flatten() {
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut buffer = [0; 8192];
            let Ok(n) = socket.read(&mut buffer) else {
                continue;
            };
            let request = String::from_utf8_lossy(&buffer[..n]);
            let path = request.split_whitespace().nth(1).unwrap_or("");
            *server_requests
                .lock()
                .unwrap()
                .entry(path.to_owned())
                .or_default() += 1;
            let (mime, body) = match path {
                "/youtube-test" => ("text/html", r#"<!doctype html><script>
                  window.ytInitialPlayerResponse={playerAds:[{ad:true}],adPlacements:[{ad:true}],adSlots:[{ad:true}],videoDetails:{title:'kept'}};
                  const initialPruned = !window.ytInitialPlayerResponse.playerAds && !window.ytInitialPlayerResponse.adPlacements && !window.ytInitialPlayerResponse.adSlots;
                  addEventListener('load',async()=>{
                    const response=await (await fetch('/youtubei/v1/player?test=1')).json();
                    window.ipc.postMessage(JSON.stringify({initial_pruned:initialPruned,fetch_pruned:!response.adPlacements&&!response.adSlots,normal_video:response.videoDetails.title}));
                  });
                </script>"#),
                "/youtubei/v1/player?test=1" => ("application/json", r#"{"adPlacements":[{"test":"ad"}],"adSlots":[{"test":"ad"}],"videoDetails":{"title":"kept"}}"#),
                "/" => (
                    "text/html",
                    r#"<!doctype html><script src='/ads/cbr.js'></script><body><div data-ad-name='smoke'>Ad</div><script>
                const frame = document.createElement('iframe');
                const frameResult = new Promise(resolve => addEventListener('message', e => {
                  if(e.source === frame.contentWindow) resolve(e.data.ad);
                }));
                frame.src = 'http://frame.pip-adblock.test:' + location.port + '/frame';
                document.body.appendChild(frame);
                function video(url) { return new Promise(resolve => {
                  const v = document.createElement('video');
                  v.preload = 'auto'; v.onerror = v.onloadeddata = resolve;
                  v.src = url; document.body.appendChild(v); v.load();
                }); }
                const videos = Promise.all([video('/normal.mp4'), video('/adserver.mp4')]);
                addEventListener('load', async () => {
                  const normal = await (await fetch('/normal')).text();
                  const media = await (await fetch('/ads/stream.m3u8')).text();
                  const frame_ad = await frameResult; await videos;
                  const hidden = () => getComputedStyle(document.querySelector('[data-ad-name]')).display === 'none';
                  const end = performance.now() + 2000;
                  while(!hidden() && performance.now() < end) await new Promise(r => setTimeout(r, 25));
                  window.ipc.postMessage(JSON.stringify({normal, media, ad: !!window.adLoaded, frame_ad, cosmetic_hidden: hidden()}));
                });</script>"#,
                ),
                "/frame" => ("text/html", "<script src='/ads/cbr.js'></script><script>addEventListener('load',()=>parent.postMessage({ad:!!window.adLoaded},'*'))</script>"),
                "/ads/cbr.js" => ("application/javascript", "window.adLoaded=true;"),
                "/normal.mp4" | "/adserver.mp4" => ("video/mp4", "not a real video; request transport test"),
                _ => ("text/plain", "ok"),
            };
            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nContent-Security-Policy: style-src 'none'\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}", body.len());
            let _ = socket.write_all(response.as_bytes());
        }
    });
    let directory = std::env::temp_dir().join(format!("pip-smoke-{}", std::process::id()));
    std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", directory.join("webview"));
    let event_loop = EventLoop::<String>::with_user_event().build().unwrap();
    let mut smoke = Smoke {
        proxy: event_loop.create_proxy(),
        blocker: Rc::new(RefCell::new(adblock::Blocker::load(directory.clone()))),
        window: None,
        view: None,
        url,
        phase: 0,
        deadline: Instant::now() + Duration::from_secs(40),
        requests,
    };
    event_loop.run_app(&mut smoke).unwrap();
    assert_eq!(smoke.phase, 5);
    assert!(directory.join("adblock-exceptions.json").is_file());
    // Runtime may still hold its temporary profile; leave it for OS temp cleanup.
}
