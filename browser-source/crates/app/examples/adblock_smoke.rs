//! cargo run -p evergreen-browser --example adblock_smoke --release
//! Real WebView2 interception + persisted site exception, against loopback only.
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
    collections::HashMap,
    io::{Read, Write},
    net::TcpListener,
    rc::Rc,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use webview2_com::{
    AddScriptToExecuteOnDocumentCreatedCompletedHandler, NewWindowRequestedEventHandler,
};
use windows::core::{HSTRING, PWSTR};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
    window::{Window, WindowId},
};
use wry::{WebViewBuilderExtWindows, WebViewExtWindows};

struct Smoke {
    proxy: EventLoopProxy<String>,
    blocker: Rc<RefCell<adblock::Blocker>>,
    window: Option<Window>,
    view: Option<wry::WebView>,
    url: String,
    phase: u8,
    deadline: Instant,
    requests: Arc<Mutex<HashMap<String, u32>>>,
    youtube_script_id: Option<String>,
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
            .with_additional_browser_args("--host-resolver-rules=\"MAP pip-adblock.test 127.0.0.1, MAP frame.pip-adblock.test 127.0.0.1, MAP casino.pip-adblock.test 127.0.0.1\" --no-proxy-server")
            .with_ipc_handler(move |req| {
                let _ = proxy.send_event(req.body().clone());
            })
            .build_as_child(&window)
            .unwrap();
        adblock::attach(&view, self.blocker.clone()).unwrap();
        let core = unsafe { view.controller().CoreWebView2() }.unwrap();
        let popup_proxy = self.proxy.clone();
        let popup_blocker = self.blocker.clone();
        let popup_handler =
            NewWindowRequestedEventHandler::create(Box::new(move |sender, args| {
                let Some(args) = args else {
                    return Ok(());
                };
                let mut raw = PWSTR::null();
                let target = if unsafe { args.Uri(&mut raw) }.is_ok() && !raw.is_null() {
                    webview2_com::take_pwstr(raw)
                } else {
                    String::new()
                };
                let source = sender
                    .and_then(|sender| {
                        let mut raw = PWSTR::null();
                        unsafe { sender.Source(&mut raw) }.ok()?;
                        (!raw.is_null()).then(|| webview2_com::take_pwstr(raw))
                    })
                    .unwrap_or_default();
                let mut user_initiated = windows::core::BOOL(0);
                let user_initiated_read = unsafe { args.IsUserInitiated(&mut user_initiated) };
                let blocked = popup_blocker.borrow().should_block_popup_with_popup_context(
                    &target,
                    &source,
                    None,
                    user_initiated.as_bool(),
                );
                // The harness owns this event and never creates an unmanaged popup.
                // The production handler likewise handles every event before creating
                // a Pip tab for an allowed target.
                let handled = unsafe { args.SetHandled(true) }.is_ok();
                let _ = popup_proxy.send_event(
                    serde_json::json!({
                        "kind": "popup_event",
                        "target": target,
                        "source": source,
                        "user_initiated": user_initiated.as_bool(),
                        "user_initiated_read": user_initiated_read.is_ok(),
                        "blocked": blocked,
                        "handled": handled,
                    })
                    .to_string(),
                );
                Ok(())
            }));
        let mut popup_token = 0;
        unsafe { core.add_NewWindowRequested(&popup_handler, &mut popup_token) }.unwrap();
        view.load_url(&(self.url.clone() + "popup-test")).unwrap();
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
        if self.phase == 10 {
            let value: serde_json::Value = serde_json::from_str(&message).unwrap();
            if value["kind"] != "popup_event" {
                return;
            }
            println!("popup event report {value}");
            assert_eq!(value["user_initiated_read"], true);
            assert_eq!(
                value["user_initiated"], false,
                "window.open without a user gesture must be reported as automatic"
            );
            assert_eq!(
                value["blocked"], true,
                "the fixture's popup decision must block the automatic gambling target"
            );
            assert_eq!(
                value["handled"], true,
                "the automatic popup event must not escape as an unmanaged WebView"
            );
            assert!(value["target"]
                .as_str()
                .and_then(|target| target.parse::<wry::http::Uri>().ok())
                .is_some_and(|target| {
                    target.host() == Some("casino.pip-adblock.test") && target.path() == "/popup"
                }));
            assert_eq!(
                self.requests
                    .lock()
                    .unwrap()
                    .get("/popup")
                    .copied()
                    .unwrap_or(0),
                0,
                "blocked popup must never reach the loopback server"
            );
            println!("PASS: real WebView2 NewWindowRequested reported an automatic window.open; the fixture's popup decision returned blocked, and the popup request did not reach the loopback server.");
            println!("LIMIT: the fixture attaches its own event listener and duplicates production decision wiring; WebView2/Wry default handling can also suppress unmanaged popups, so this does not prove production callback integration.");
            println!("LIMIT: this hidden-window harness cannot produce a genuine mouse/keyboard user gesture, so it does not claim to verify the user-initiated safe-link path; it never fabricates IsUserInitiated=true.");
            self.phase = 0;
            self.view.as_ref().unwrap().load_url(&self.url).unwrap();
            return;
        }
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
        // HSTS prevents a plain HTTP YouTube fixture. Only in this test, substitute
        // the hostname guard in the exact engine-produced script; source JS is unchanged.
        let script = self
            .blocker
            .borrow()
            .youtube_script()
            .replace("case \"www.youtube.com\"", "case \"pip-adblock.test\"");
        install_document_start_script(
            self.view.as_ref().unwrap(),
            &script,
            &mut self.youtube_script_id,
        )
        .unwrap();
        self.view
            .as_ref()
            .unwrap()
            .load_url(&(self.url.clone() + "youtube-test"))
            .unwrap();
    }
}

// Reuse the same child WebView for all phases. Rebuilding a WebView immediately after
// dropping one can race WebView2's controller teardown and fail with ERROR_INVALID_STATE.
// This mirrors the production document-start script replacement used by refresh_scriptlets.
fn install_document_start_script(
    view: &wry::WebView,
    script: &str,
    previous_id: &mut Option<String>,
) -> windows::core::Result<()> {
    let core = unsafe { view.controller().CoreWebView2()? };
    let new_id = Rc::new(RefCell::new(None));
    let callback_id = new_id.clone();
    let registration_core = core.clone();
    let script = script.to_owned();
    AddScriptToExecuteOnDocumentCreatedCompletedHandler::wait_for_async_operation(
        Box::new(move |handler| unsafe {
            registration_core
                .AddScriptToExecuteOnDocumentCreated(&HSTRING::from(script), &handler)
                .map_err(Into::into)
        }),
        Box::new(move |result, id| {
            result?;
            *callback_id.borrow_mut() = Some(id);
            Ok(())
        }),
    )
    .map_err(|error| {
        windows::core::Error::new(
            windows::core::HRESULT(0x80004005u32 as i32),
            error.to_string(),
        )
    })?;
    let id = new_id.borrow_mut().take().ok_or_else(|| {
        windows::core::Error::new(
            windows::core::HRESULT(0x80004005u32 as i32),
            "WebView2 did not return a document-start script ID",
        )
    })?;
    if let Some(previous_id) = previous_id.replace(id) {
        unsafe { core.RemoveScriptToExecuteOnDocumentCreated(&HSTRING::from(previous_id))? };
    }
    Ok(())
}
fn main() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!(
        "http://pip-adblock.test:{}/",
        listener.local_addr().unwrap().port()
    );
    let popup_url = format!(
        "http://casino.pip-adblock.test:{}/popup",
        listener.local_addr().unwrap().port()
    );
    let popup_url_for_server = serde_json::to_string(&popup_url).unwrap();
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
                "/popup-test" => (
                    "text/html",
                    "<!doctype html><script>window.open(__POPUP_URL__,'_blank')</script>",
                ),
                "/popup" => ("text/plain", "popup reached server"),
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
            let body = body.replace("__POPUP_URL__", &popup_url_for_server);
            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nContent-Security-Policy: style-src 'none'\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}", body.len());
            let _ = socket.write_all(response.as_bytes());
        }
    });
    let directory = std::env::temp_dir().join(format!("pip-smoke-{}", std::process::id()));
    std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", directory.join("webview"));
    let event_loop = EventLoop::<String>::with_user_event().build().unwrap();
    let mut smoke = Smoke {
        proxy: event_loop.create_proxy(),
        blocker: Rc::new(RefCell::new(adblock::Blocker::load(
            directory.clone(),
            evergreen_core::settings::AdblockMode::Balanced,
        ))),
        window: None,
        view: None,
        url,
        phase: 10,
        deadline: Instant::now() + Duration::from_secs(40),
        requests,
        youtube_script_id: None,
    };
    event_loop.run_app(&mut smoke).unwrap();
    assert_eq!(smoke.phase, 5);
    assert!(directory.join("adblock-exceptions.json").is_file());
    // Runtime may still hold its temporary profile; leave it for OS temp cleanup.
}
