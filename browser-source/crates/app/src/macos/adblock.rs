//! Brave's bundled filters converted to WebKit's native request blocker.
//! Conversion is intentionally narrower than the Windows engine: scriptlets,
//! redirect resources and procedural cosmetic rules cannot be expressed here.

use adblock::lists::{FilterSet, ParseOptions, RuleTypes};
use block2::RcBlock;
use objc2::{rc::Retained, MainThreadMarker};
use objc2_foundation::{NSError, NSString};
use objc2_web_kit::{WKContentRuleList, WKContentRuleListStore};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{cell::RefCell, rc::Rc};
use wry::{WebView, WebViewExtMacOS};

const DOMAINS: &str = include_str!(concat!(env!("OUT_DIR"), "/gambling-domains.txt"));
const POPUP_OVERRIDES: &str = include_str!("../../filters/popup-overrides.txt");
include!(concat!(env!("OUT_DIR"), "/gambling-domain-offsets.rs"));

#[derive(Clone, Debug, Serialize)]
pub struct Stats {
    pub native_rules: usize,
    pub converted_source_rules: usize,
    // Includes unsupported syntax; deliberately not advertised as an exact
    // unsupported-rule count, because the upstream parser skips some lines.
    pub input_noncomment_lines: usize,
}

#[derive(Debug)]
pub struct Prepared {
    json: String,
    identifier: String,
    pub stats: Stats,
}

enum State {
    Preparing,
    Compiling,
    Ready(Retained<WKContentRuleList>, Stats),
    Failed(String),
}

#[derive(Clone)]
pub struct Blocker(Rc<RefCell<State>>);

impl Blocker {
    pub fn new() -> Self {
        Self(Rc::new(RefCell::new(State::Preparing)))
    }

    pub fn ready(&self) -> bool {
        matches!(&*self.0.borrow(), State::Ready(..))
    }

    pub fn message(&self) -> String {
        match &*self.0.borrow() {
            State::Preparing | State::Compiling => "Đang chuẩn bị chặn quảng cáo…".into(),
            State::Ready(..) => "Chặn quảng cáo sẵn sàng".into(),
            State::Failed(_) => "Bộ lọc quảng cáo chưa sẵn sàng".into(),
        }
    }

    /// Keep the toolbar short while retaining useful diagnostics for assistive
    /// technology, pointer hover, and troubleshooting notices.
    pub fn details(&self) -> String {
        match &*self.0.borrow() {
            State::Preparing => "Đang chuẩn bị bộ lọc WebKit.".into(),
            State::Compiling => "Đang biên dịch bộ lọc WebKit.".into(),
            State::Ready(_, stats) => format!(
                "Bộ lọc WebKit đang hoạt động với {} quy tắc đã biên dịch. macOS chưa hỗ trợ scriptlet YouTube.",
                stats.native_rules
            ),
            State::Failed(error) => format!("Không thể chuẩn bị bộ lọc WebKit: {error}"),
        }
    }

    pub fn stats(&self) -> Option<Stats> {
        match &*self.0.borrow() {
            State::Ready(_, stats) => Some(stats.clone()),
            _ => None,
        }
    }

    pub fn fail(&self, error: String) {
        eprintln!("Pip macOS adblock: {error}");
        *self.0.borrow_mut() = State::Failed(error);
    }

    /// Only call on the native event-loop thread. The page must still be local.
    pub fn attach(&self, view: &WebView) -> bool {
        if let State::Ready(rules, _) = &*self.0.borrow() {
            // The retained list and controller are valid, and WK objects stay
            // on their main thread. This must precede any remote navigation.
            unsafe { view.manager().addContentRuleList(rules) };
            true
        } else {
            false
        }
    }

    /// WebKit caches compiled lists. The content hash prevents stale filters
    /// being reused after an update. Completion handlers run on the main queue.
    pub fn compile(&self, prepared: Prepared, changed: impl Fn() + 'static) {
        let Some(mtm) = MainThreadMarker::new() else {
            self.fail("WebKit phải được khởi tạo trên luồng chính.".into());
            changed();
            return;
        };
        let Some(store) = (unsafe { WKContentRuleListStore::defaultStore(mtm) }) else {
            self.fail("Không mở được kho bộ lọc WebKit.".into());
            changed();
            return;
        };
        *self.0.borrow_mut() = State::Compiling;
        let blocker = self.clone();
        let identifier = NSString::from_str(&prepared.identifier);
        let lookup_identifier = identifier.clone();
        let lookup_store = store.clone();
        let changed: Rc<dyn Fn()> = Rc::new(changed);
        let lookup = RcBlock::new(move |cached: *mut WKContentRuleList, _: *mut NSError| {
            if let Some(rules) = unsafe { Retained::retain(cached) } {
                *blocker.0.borrow_mut() = State::Ready(rules, prepared.stats.clone());
                changed();
                return;
            }
            let encoded = NSString::from_str(&prepared.json);
            let completed_blocker = blocker.clone();
            let completed_changed = changed.clone();
            let stats = prepared.stats.clone();
            let completed =
                RcBlock::new(move |result: *mut WKContentRuleList, error: *mut NSError| {
                    if let Some(rules) = unsafe { Retained::retain(result) } {
                        *completed_blocker.0.borrow_mut() = State::Ready(rules, stats.clone());
                    } else {
                        let detail = unsafe { error.as_ref() }
                            .map(|error| error.localizedDescription().to_string())
                            .unwrap_or_else(|| "WebKit không trả lại bộ lọc đã biên dịch.".into());
                        completed_blocker.fail(detail);
                    }
                    completed_changed();
                });
            unsafe {
                lookup_store
                    .compileContentRuleListForIdentifier_encodedContentRuleList_completionHandler(
                        Some(&lookup_identifier),
                        Some(&encoded),
                        Some(&completed),
                    );
            }
        });
        unsafe {
            store.lookUpContentRuleListForIdentifier_completionHandler(
                Some(&identifier),
                Some(&lookup),
            );
        }
    }
}

/// Runs on a worker: native WebKit objects never leave the event-loop thread.
pub fn prepare(verify: bool) -> Result<Prepared, String> {
    let mut filters = FilterSet::new(true);
    let mut input_noncomment_lines = 0;
    let sources = [
        (include_str!("../../filters/easylist.txt"), false),
        (
            include_str!("../../filters/upstream/uBlockOrigin/uAssets/filters/filters.txt"),
            true,
        ),
        (
            include_str!("../../filters/upstream/uBlockOrigin/uAssets/filters/quick-fixes.txt"),
            true,
        ),
        (
            include_str!("../../filters/upstream/uBlockOrigin/uAssets/filters/unbreak.txt"),
            true,
        ),
        (include_str!("../../filters/popup-overrides.txt"), true),
        (include_str!("../../filters/youtube.txt"), false),
    ];
    for (source, network_only) in sources {
        input_noncomment_lines += source
            .lines()
            .filter(|line| {
                let line = line.trim();
                !line.is_empty() && !line.starts_with(['!', '['])
            })
            .count();
        filters.add_filter_list(
            source.to_owned(),
            ParseOptions {
                rule_types: if network_only {
                    RuleTypes::NetworkOnly
                } else {
                    RuleTypes::All
                },
                ..Default::default()
            },
        );
    }
    if verify {
        // Test-only URL paths; no production site depends on these rules.
        filters.add_filter_list(
            "/__pip_adblock_verify__/blocked.js$script\n/__pip_adblock_verify__/exception.js$script\n@@/__pip_adblock_verify__/exception.js$script".into(),
            ParseOptions::default(),
        );
    }
    let (rules, used) = filters
        .into_content_blocking()
        .map_err(|_| "Không chuyển được bộ lọc Brave sang WebKit.".to_owned())?;
    if rules.is_empty() {
        return Err("Bộ lọc WebKit rỗng; dừng trước khi mở web.".into());
    }
    let stats = Stats {
        native_rules: rules.len(),
        converted_source_rules: used.len(),
        input_noncomment_lines,
    };
    // Some trigger fields are HashSets upstream. Canonicalize their arrays so
    // equivalent rules get the same cache key across process launches. Preserve
    // rule ordering: ignore-previous-rules semantics depend on that ordering.
    let mut value = serde_json::to_value(&rules).map_err(|error| error.to_string())?;
    drop(rules);
    drop(used);
    if let Some(rules) = value.as_array_mut() {
        for rule in rules {
            if let Some(trigger) = rule
                .get_mut("trigger")
                .and_then(serde_json::Value::as_object_mut)
            {
                for field in trigger.values_mut() {
                    if let Some(values) = field.as_array_mut() {
                        values.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
                    }
                }
            }
        }
    }
    let json = serde_json::to_string(&value).map_err(|error| error.to_string())?;
    let identifier = format!("pip-brave-webkit-v1-{:x}", Sha256::digest(json.as_bytes()));
    Ok(Prepared {
        json,
        identifier,
        stats,
    })
}

/// The converter adds a first-party document exception. Enforce gambling
/// navigation separately using the existing generated sorted domain table.
pub fn blocks_navigation(url: &str) -> bool {
    let Ok(uri) = url.parse::<wry::http::Uri>() else {
        return false;
    };
    let Some(host) = uri.host() else { return false };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let mut candidate = host.as_str();
    loop {
        // This user-reported popup host is in Pip's maintained overrides, not
        // necessarily in the upstream HaGeZi snapshot. WebKit conversion drops
        // $popup, so preserve these explicit host rules in navigation policy.
        if POPUP_OVERRIDES.lines().any(|line| {
            line.trim()
                .strip_prefix("||")
                .and_then(|rule| rule.strip_suffix("^$popup"))
                == Some(candidate)
        }) {
            return true;
        }
        if GAMBLING_DOMAIN_OFFSETS
            .binary_search_by(|offset| {
                DOMAINS[*offset as usize..]
                    .lines()
                    .next()
                    .unwrap_or("")
                    .cmp(candidate)
            })
            .is_ok()
        {
            return true;
        }
        let Some((_, suffix)) = candidate.split_once('.') else {
            return false;
        };
        candidate = suffix;
    }
}

#[cfg(test)]
mod tests {
    use super::blocks_navigation;

    #[test]
    fn reported_gambling_popup_host_is_blocked_without_matching_lookalikes() {
        assert!(blocks_navigation(
            "https://ad88vip6.com/home/register?id=940814853"
        ));
        assert!(blocks_navigation("https://offers.ad88vip6.com/register"));
        assert!(!blocks_navigation("https://ad88vip6.com.evil.example/"));
        assert!(!blocks_navigation("https://www.google.com/search?q=pip"));
    }
}

/// Loopback-only native verification server. Counters prove an allowed script
/// reached the server and a blocked script never reached the network hook.
pub struct Fixture {
    pub url: String,
    allowed: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    blocked: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    exception: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl Fixture {
    pub fn start() -> std::io::Result<Self> {
        use std::{
            io::{Read, Write},
            net::TcpListener,
            sync::{
                atomic::{AtomicUsize, Ordering},
                Arc,
            },
            time::Duration,
        };
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let fixture = Self {
            url: format!(
                "http://{}/__pip_adblock_verify__/index.html",
                listener.local_addr()?
            ),
            allowed: Arc::new(AtomicUsize::new(0)),
            blocked: Arc::new(AtomicUsize::new(0)),
            exception: Arc::new(AtomicUsize::new(0)),
        };
        let (allowed, blocked, exception) = (
            fixture.allowed.clone(),
            fixture.blocked.clone(),
            fixture.exception.clone(),
        );
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let mut request = Vec::new();
                let mut chunk = [0; 1024];
                while request.len() < 8192 && !request.windows(4).any(|part| part == b"\r\n\r\n") {
                    match stream.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(count) => request.extend_from_slice(&chunk[..count]),
                    }
                }
                let request = String::from_utf8_lossy(&request);
                let path = request.split_whitespace().nth(1).unwrap_or("");
                let (mime, body) = match path {
                    "/__pip_adblock_verify__/index.html" => ("text/html", "<!doctype html><meta charset=utf-8><title>Pip adblock verification</title><p id=pip-runtime-probe>pending</p><script src=allowed.js></script><script src=blocked.js></script><script src=exception.js></script>"),
                    "/__pip_adblock_verify__/allowed.js" => {
                        allowed.fetch_add(1, Ordering::Relaxed);
                        ("application/javascript", "window.__pipAllowed=true;")
                    },
                    "/__pip_adblock_verify__/blocked.js" => {
                        blocked.fetch_add(1, Ordering::Relaxed);
                        ("application/javascript", "window.__pipBlocked=true;")
                    },
                    "/__pip_adblock_verify__/exception.js" => {
                        exception.fetch_add(1, Ordering::Relaxed);
                        ("application/javascript", "window.__pipException=true;")
                    },
                    _ => ("text/plain", "not found"),
                };
                let response = format!("HTTP/1.1 200 OK\r\nContent-Type: {mime}; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}", body.len());
                let _ = stream.write_all(response.as_bytes());
            }
        });
        Ok(fixture)
    }

    pub fn network_passed(&self) -> bool {
        use std::sync::atomic::Ordering;
        self.allowed.load(Ordering::Relaxed) > 0
            && self.exception.load(Ordering::Relaxed) > 0
            && self.blocked.load(Ordering::Relaxed) == 0
    }
}
