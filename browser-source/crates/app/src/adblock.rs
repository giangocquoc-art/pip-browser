//! One Brave engine shared by tabs; native filtering, CSS and upstream YouTube scriptlets.
use adblock::{
    lists::{FilterSet, ParseOptions},
    request::Request,
    resources::{PermissionMask, Resource},
    Engine,
};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::PathBuf,
    rc::{Rc, Weak},
};
use webview2_com::{
    AddScriptToExecuteOnDocumentCreatedCompletedHandler, DOMContentLoadedEventHandler,
    Microsoft::Web::WebView2::Win32::*, WebResourceRequestedEventHandler,
};
use windows::core::{w, Interface, HSTRING, PWSTR};
use wry::WebViewExtWindows;

pub struct Blocker {
    engine: Engine,
    disabled: HashSet<String>,
    exceptions_path: PathBuf,
    pub blocked: u64,
    bootstraps: HashMap<String, Weak<RefCell<Bootstrap>>>,
}

#[derive(Default)]
struct Bootstrap {
    id: Option<String>,
}

fn site(url: &str) -> Option<String> {
    let uri = url.parse::<wry::http::Uri>().ok()?;
    if !matches!(uri.scheme_str(), Some("http" | "https")) {
        return None;
    }
    Some(uri.host()?.to_ascii_lowercase())
}

#[cfg(test)]
fn engine(rules: &str) -> Engine {
    let mut filters = FilterSet::new(false);
    filters.add_filter_list(
        rules.to_owned(),
        ParseOptions {
            rule_types: adblock::lists::RuleTypes::All,
            ..Default::default()
        },
    );
    Engine::new_with_filter_set(filters)
}

impl Blocker {
    pub fn enabled(&self, url: &str) -> Option<bool> {
        Some(!self.disabled.contains(&site(url)?))
    }
    pub fn load(directory: PathBuf) -> Self {
        let exceptions_path = directory.join("adblock-exceptions.json");
        let disabled = std::fs::read(&exceptions_path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let mut filters = FilterSet::new(false);
        filters.add_filter_list(
            include_str!("../filters/easylist.txt").to_owned(),
            ParseOptions::default(),
        );
        // Only pinned, reviewed upstream lists can invoke trusted scriptlets.
        filters.add_filter_list(
            include_str!("../filters/youtube.txt").to_owned(),
            ParseOptions {
                permissions: PermissionMask::from_bits(1),
                ..Default::default()
            },
        );
        let mut engine = Engine::new_with_filter_set(filters);
        let resources: Vec<Resource> =
            serde_json::from_str(include_str!("../filters/scriptlet-resources.json"))
                .expect("bundled upstream resources must be valid");
        engine.use_resources(resources);
        Self {
            engine,
            disabled,
            exceptions_path,
            blocked: 0,
            bootstraps: HashMap::new(),
        }
    }

    pub fn toggle(&mut self, url: &str) -> Result<Option<bool>, Box<dyn std::error::Error>> {
        let Some(host) = site(url) else {
            return Ok(None);
        };
        let mut disabled = self.disabled.clone();
        let enabled = if disabled.remove(&host) {
            true
        } else {
            disabled.insert(host);
            false
        };
        std::fs::create_dir_all(self.exceptions_path.parent().unwrap())?;
        std::fs::write(&self.exceptions_path, serde_json::to_vec_pretty(&disabled)?)?;
        self.disabled = disabled;
        Ok(Some(enabled))
    }

    fn should_block(&self, url: &str, top: &str, source: &str, kind: &str, method: &str) -> bool {
        let Some(host) = site(top) else {
            return false;
        };
        if self.disabled.contains(&host) || site(source).is_none() {
            return false;
        }
        // Never blank the top-level page. Media is classified by WebView2, not its extension.
        if kind == "document" {
            return false;
        }
        Request::new(url, source, kind, method)
            .ok()
            .is_some_and(|r| self.engine.check_network_request(&r).should_block())
    }

    fn cosmetic_script(&self, url: &str) -> Option<String> {
        let host = site(url)?;
        if self.disabled.contains(&host) {
            return None;
        }
        // ponytail: static URL selectors only; procedural CSS needs a separate DOM executor.
        // CSS also applies to matching elements inserted later by SPA pages.
        let selectors = self.engine.url_cosmetic_resources(url).hide_selectors;
        if selectors.is_empty() {
            return None;
        }
        Some(format!(
            "(()=>{{if(location.href!=={})return;const sheet=new CSSStyleSheet();for(const selector of {}){{try{{sheet.insertRule(selector+'{{display:none!important}}');}}catch(_){{}}}}document.adoptedStyleSheets=[...document.adoptedStyleSheets,sheet];}})()",
            serde_json::to_string(url).unwrap(),
            serde_json::to_string(&selectors).unwrap(),
        ))
    }

    pub(crate) fn youtube_script(&self) -> String {
        let mut script =
            String::from("(()=>{if(window!==window.top)return;switch(location.hostname){");
        // No observers or scriptlet machinery on other websites or internal Pip pages.
        for host in [
            "www.youtube.com",
            "youtube.com",
            "m.youtube.com",
            "music.youtube.com",
            "www.youtube-nocookie.com",
            "youtube-nocookie.com",
        ] {
            if self.disabled.contains(host) {
                continue;
            }
            let resources = self
                .engine
                .url_cosmetic_resources(&format!("https://{host}/"));
            if !resources.injected_script.is_empty() {
                script.push_str(&format!(
                    "case {}:{{const scriptletGlobals={{}};{}\nbreak;}}",
                    serde_json::to_string(host).unwrap(),
                    resources.injected_script
                ));
            }
        }
        script.push_str("}})();");
        script
    }
}

// Reuse WebView2's message-pumping completion helper, as Wry does for its
// initialization scripts. The first remote navigation waits for registration.
fn install_bootstrap(
    core: &ICoreWebView2,
    state: Rc<RefCell<Bootstrap>>,
    script: String,
) -> windows::core::Result<()> {
    let registration_core = core.clone();
    let new_id = Rc::new(RefCell::new(None));
    let callback_id = new_id.clone();
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
    let previous = std::mem::replace(&mut state.borrow_mut().id, new_id.borrow_mut().take());
    if let Some(id) = previous {
        unsafe {
            core.RemoveScriptToExecuteOnDocumentCreated(&HSTRING::from(id))?;
        }
    }
    Ok(())
}

pub fn refresh_scriptlets(
    view: &wry::WebView,
    blocker: &Rc<RefCell<Blocker>>,
) -> windows::core::Result<()> {
    let (state, script) = {
        let blocker = blocker.borrow();
        (
            blocker.bootstraps.get(view.id()).and_then(Weak::upgrade),
            blocker.youtube_script(),
        )
    };
    if let Some(state) = state {
        install_bootstrap(&unsafe { view.controller().CoreWebView2()? }, state, script)?;
    }
    Ok(())
}

pub fn attach(view: &wry::WebView, blocker: Rc<RefCell<Blocker>>) -> windows::core::Result<()> {
    let core = unsafe { view.controller().CoreWebView2()? };
    let bootstrap = Rc::new(RefCell::new(Bootstrap::default()));
    {
        let mut blocker = blocker.borrow_mut();
        blocker
            .bootstraps
            .retain(|_, value| value.strong_count() > 0);
        blocker
            .bootstraps
            .insert(view.id().to_string(), Rc::downgrade(&bootstrap));
    }
    let script = blocker.borrow().youtube_script();
    install_bootstrap(&core, bootstrap.clone(), script)?;
    let environment = view.environment();
    let cosmetic_blocker = blocker.clone();
    let cosmetic_handler = DOMContentLoadedEventHandler::create(Box::new(move |sender, _| {
        let _registration_lifetime = &bootstrap;
        if let Some(sender) = sender {
            let mut raw = PWSTR::null();
            unsafe { sender.Source(&mut raw)? };
            let url = webview2_com::take_pwstr(raw);
            // Release RefCell before calling COM, which may dispatch reentrant callbacks.
            let script = cosmetic_blocker.borrow().cosmetic_script(&url);
            if let Some(script) = script {
                unsafe { sender.ExecuteScript(&HSTRING::from(script), None)? };
            }
        }
        Ok(())
    }));
    let mut cosmetic_token = 0;
    unsafe {
        core.cast::<ICoreWebView2_2>()?
            .add_DOMContentLoaded(&cosmetic_handler, &mut cosmetic_token)?;
    }
    let handler = WebResourceRequestedEventHandler::create(Box::new(move |sender, args| {
        let (Some(sender), Some(args)) = (sender, args) else {
            return Ok(());
        };
        let request = unsafe { args.Request()? };
        let mut raw = PWSTR::null();
        unsafe {
            request.Uri(&mut raw)?;
        }
        let url = webview2_com::take_pwstr(raw);
        let mut raw = PWSTR::null();
        unsafe {
            sender.Source(&mut raw)?;
        }
        let top = webview2_com::take_pwstr(raw);
        let mut raw = PWSTR::null();
        unsafe {
            request.Method(&mut raw)?;
        }
        let method = webview2_com::take_pwstr(raw);
        // Referrer is the best available frame context in this WebView2 event.
        // ponytail: missing referrer falls back to top-level URL; no fabricated frame graph.
        let source = unsafe { request.Headers() }
            .ok()
            .and_then(|headers| {
                let mut raw = PWSTR::null();
                unsafe { headers.GetHeader(w!("Referer"), &mut raw) }.ok()?;
                Some(webview2_com::take_pwstr(raw))
            })
            .filter(|s| site(s).is_some())
            .unwrap_or_else(|| top.clone());
        let mut context = COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL;
        unsafe {
            args.ResourceContext(&mut context)?;
        }
        let kind = match context {
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_SCRIPT => "script",
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE => "image",
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_STYLESHEET => "stylesheet",
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FONT => "font",
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_XML_HTTP_REQUEST
            | COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FETCH => "xmlhttprequest",
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_PING => "ping",
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_MEDIA => "media",
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_OTHER => "other",
            _ => return Ok(()),
        };
        if blocker
            .borrow()
            .should_block(&url, &top, &source, kind, &method)
        {
            // Empty response prevents the network request; no proxy or injected script.
            let response = unsafe {
                environment.CreateWebResourceResponse(
                    None,
                    204,
                    w!("No Content"),
                    w!("Cache-Control: no-store\r\n"),
                )?
            };
            unsafe {
                args.SetResponse(&response)?;
            }
            blocker.borrow_mut().blocked += 1;
        }
        Ok(())
    }));
    let mut token = 0;
    unsafe {
        core.add_WebResourceRequested(&handler, &mut token)?;
        // Explicit document-source filters also cover cross-origin frames on modern runtimes.
        // Worker requests need profile-level ownership/context; don't duplicate them across tabs.
        let modern = core.cast::<ICoreWebView2_22>().ok();
        for context in [
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_SCRIPT,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_STYLESHEET,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FONT,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_XML_HTTP_REQUEST,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FETCH,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_PING,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_MEDIA,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_OTHER,
        ] {
            for pattern in [w!("https://*"), w!("http://*")] {
                if let Some(modern) = &modern {
                    modern.AddWebResourceRequestedFilterWithRequestSourceKinds(
                        pattern,
                        context,
                        COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_DOCUMENT,
                    )?;
                } else {
                    core.AddWebResourceRequestedFilter(pattern, context)?;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn youtube_resources_resolve_and_exceptions_remove_document_start_code() {
        let mut blocker = Blocker::load(std::env::temp_dir().join("pip-unused-test-path"));
        let script = blocker.youtube_script();
        assert!(script.contains("function jsonPruneFetchResponse"));
        assert!(script.contains("function setConstant"));
        assert!(script.contains("function safeSelf"));
        assert!(script.contains("case \"www.youtube.com\""));
        assert!(!script.contains("case \"youtube.com.evil\""));
        let artifact = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/youtube-bootstrap.js");
        std::fs::write(artifact, &script).unwrap();
        blocker.disabled.insert("www.youtube.com".into());
        assert!(!blocker
            .youtube_script()
            .contains("case \"www.youtube.com\""));
    }
    #[test]
    fn blocking_exceptions_and_video() {
        let mut b = Blocker {
            engine: engine("||ads.example^\n@@||ads.example/allowed.js$script"),
            disabled: HashSet::new(),
            exceptions_path: PathBuf::new(),
            blocked: 0,
            bootstraps: HashMap::new(),
        };
        let top = "https://news.example/";
        assert!(b.should_block("https://ads.example/banner.js", top, top, "script", "GET"));
        assert!(!b.should_block("https://ads.example/allowed.js", top, top, "script", "GET"));
        assert!(b.should_block(
            "https://ads.example/live.M3U8?token=x",
            top,
            top,
            "xmlhttprequest",
            "GET"
        ));
        assert!(b.should_block("https://ads.example/video", top, top, "media", "GET"));
        assert!(!b.should_block("https://video.example/live.m3u8", top, top, "media", "GET"));
        assert!(!b.should_block("https://ads.example/", "about:blank", top, "script", "GET"));
        b.disabled.insert("news.example".into());
        assert!(!b.should_block("https://ads.example/banner.js", top, top, "script", "GET"));
        assert!(b.should_block(
            "https://ads.example/banner.js",
            "https://news.example.evil/",
            top,
            "script",
            "GET"
        ));
        let bundled = engine(include_str!("../filters/easylist.txt"));
        assert!(bundled
            .url_cosmetic_resources(top)
            .hide_selectors
            .contains("[data-ad-name]"));
        assert!(
            bundled
                .url_cosmetic_resources("http://127.0.0.1:1234/")
                .generichide
        );
        assert!(bundled
            .check_network_request(
                &Request::new("https://ad.doubleclick.net/banner.js", top, "script", "GET")
                    .unwrap()
            )
            .should_block());
    }

    #[test]
    fn cosmetic_rules_respect_site_exceptions() {
        let mut b = Blocker {
            engine: engine(
                "news.example##.sponsored\nnews.example#@#.allowed\nnews.example##.allowed",
            ),
            disabled: HashSet::new(),
            exceptions_path: PathBuf::new(),
            blocked: 0,
            bootstraps: HashMap::new(),
        };
        let script = b.cosmetic_script("https://news.example/").unwrap();
        assert!(script.contains(".sponsored"));
        assert!(!script.contains(".allowed"));
        assert!(b.cosmetic_script("evergreen://newtab").is_none());
        b.disabled.insert("news.example".into());
        assert!(b.cosmetic_script("https://news.example/").is_none());
    }
}
