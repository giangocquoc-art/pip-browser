//! Android's real Brave adblock-rust network engine. Engine initialization runs
//! off the UI thread; interception callbacks query its thread-safe feature set.

use adblock::{
    lists::{FilterSet, ParseOptions, RuleTypes},
    request::Request,
    resources::{PermissionMask, Resource},
    Engine,
};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    OnceLock,
};

static ENGINE: OnceLock<Engine> = OnceLock::new();
static BLOCKED: AtomicU64 = AtomicU64::new(0);
static YOUTUBE_SCRIPT: OnceLock<Option<String>> = OnceLock::new();
const NAVIGATION_DOMAINS: &str = include_str!(concat!(env!("OUT_DIR"), "/navigation-domains.txt"));
include!(concat!(env!("OUT_DIR"), "/navigation-offsets.rs"));

const EASYLIST: &str = include_str!("../../../crates/app/filters/easylist.txt");
const YOUTUBE_RULES: &str = include_str!("../../../crates/app/filters/youtube.txt");
const SCRIPTLET_RESOURCES: &str =
    include_str!("../../../crates/app/filters/scriptlet-resources.json");

#[cfg(test)]
fn engine_from_rules(rules: &str) -> Engine {
    let mut filters = FilterSet::new(false);
    filters.add_filter_list(
        rules.to_owned(),
        ParseOptions {
            rule_types: RuleTypes::NetworkOnly,
            ..Default::default()
        },
    );
    Engine::new_with_filter_set(filters)
}

fn initialize() {
    ENGINE.get_or_init(|| {
        let mut filters = FilterSet::new(false);
        for rules in [
            EASYLIST,
            include_str!(
                "../../../crates/app/filters/upstream/uBlockOrigin/uAssets/filters/filters.txt"
            ),
            include_str!(
                "../../../crates/app/filters/upstream/uBlockOrigin/uAssets/filters/quick-fixes.txt"
            ),
            include_str!(
                "../../../crates/app/filters/upstream/uBlockOrigin/uAssets/filters/unbreak.txt"
            ),
            include_str!("../../../crates/app/filters/popup-overrides.txt"),
        ] {
            filters.add_filter_list(
                rules.to_owned(),
                ParseOptions {
                    rule_types: RuleTypes::NetworkOnly,
                    ..Default::default()
                },
            );
        }
        filters.add_filter_list(
            YOUTUBE_RULES.to_owned(),
            ParseOptions {
                permissions: PermissionMask::from_bits(1),
                ..Default::default()
            },
        );
        let mut engine = Engine::new_with_filter_set(filters);
        if let Ok(resources) = serde_json::from_str::<Vec<Resource>>(SCRIPTLET_RESOURCES) {
            engine.use_resources(resources);
        }
        engine
    });

    // Scriptlet support is an enhancement over network filtering. If its pinned
    // resources fail to parse, keep the working network engine available.
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if let Some(engine) = ENGINE.get() {
            YOUTUBE_SCRIPT.get_or_init(|| build_youtube_script(engine).ok());
        }
    }));
}

fn build_youtube_script(engine: &Engine) -> Result<String, String> {
    let mut script = String::from("(()=>{if(window!==window.top)return;switch(location.hostname){");
    for host in [
        "www.youtube.com",
        "youtube.com",
        "m.youtube.com",
        "music.youtube.com",
        "www.youtube-nocookie.com",
        "youtube-nocookie.com",
    ] {
        let resources = engine.url_cosmetic_resources(&format!("https://{host}/"));
        if !resources.injected_script.is_empty() {
            script.push_str(&format!(
                "case {}:{{const scriptletGlobals={{}};{}\nbreak;}}",
                serde_json::to_string(host).map_err(|error| error.to_string())?,
                resources.injected_script
            ));
        }
    }
    script.push_str("}})();");
    if script.len() <= "(()=>{if(window!==window.top)return;switch(location.hostname){}})();".len()
    {
        return Err("no YouTube scriptlets were compiled".into());
    }
    Ok(script)
}

fn youtube_script() -> Option<&'static str> {
    YOUTUBE_SCRIPT.get().and_then(Option::as_deref)
}

fn valid_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host.contains('.')
        && host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'.')
        && host.split('.').all(|label| !label.is_empty())
}

/// Main-frame destination policy; callers separately preserve explicit user navigation.
fn check_navigation(host: &str) -> i32 {
    if host.len() > 254 {
        return 0;
    }
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if !valid_host(&host) {
        return 0;
    }
    let mut suffix = host.as_str();
    loop {
        if NAVIGATION_DOMAIN_OFFSETS
            .binary_search_by(|offset| {
                NAVIGATION_DOMAINS[*offset as usize..]
                    .split_once('\n')
                    .expect("build-time canonical domain")
                    .0
                    .cmp(suffix)
            })
            .is_ok()
        {
            BLOCKED.fetch_add(1, Ordering::Relaxed);
            return 1;
        }
        let Some((_, rest)) = suffix.split_once('.') else {
            return 0;
        };
        suffix = rest;
    }
}

/// -1 unavailable, 0 allowed/skipped invalid input, 1 blocked. The Java host preserves
/// normal networking on an engine error and exposes that failure in the UI.
fn check(url: &str, source: &str, kind: &str, method: &str) -> i32 {
    if [url, source].iter().any(|value| value.len() > 16_384)
        || kind.len() > 32
        || method.len() > 16
    {
        return 0;
    }
    let Some(engine) = ENGINE.get() else {
        return -1;
    };
    let Ok(request) = Request::new(url, source, kind, method) else {
        return 0;
    };
    if engine.check_network_request(&request).should_block() {
        BLOCKED.fetch_add(1, Ordering::Relaxed);
        1
    } else {
        0
    }
}

#[cfg(target_os = "android")]
mod bridge {
    use super::*;
    use jni::{
        objects::{JClass, JString},
        sys::{jboolean, jint, jlong, jstring, JNI_FALSE, JNI_TRUE},
        JNIEnv,
    };
    use std::panic::{catch_unwind, AssertUnwindSafe};

    #[no_mangle]
    pub extern "system" fn Java_com_psharehub_pip_NativeAdblock_nativeInitialize(
        _env: JNIEnv,
        _class: JClass,
    ) -> jboolean {
        match catch_unwind(initialize) {
            Ok(()) => JNI_TRUE,
            Err(_) => JNI_FALSE,
        }
    }

    #[no_mangle]
    pub extern "system" fn Java_com_psharehub_pip_NativeAdblock_nativeCheck(
        mut env: JNIEnv,
        _class: JClass,
        url: JString,
        source: JString,
        kind: JString,
        method: JString,
    ) -> jint {
        catch_unwind(AssertUnwindSafe(|| {
            let Ok(url) = env.get_string(&url) else {
                return -1;
            };
            let url: String = url.into();
            let Ok(source) = env.get_string(&source) else {
                return -1;
            };
            let source: String = source.into();
            let Ok(kind) = env.get_string(&kind) else {
                return -1;
            };
            let kind: String = kind.into();
            let Ok(method) = env.get_string(&method) else {
                return -1;
            };
            let method: String = method.into();
            check(&url, &source, &kind, &method)
        }))
        .unwrap_or(-1)
    }

    #[no_mangle]
    pub extern "system" fn Java_com_psharehub_pip_NativeAdblock_nativeCheckNavigationHost(
        mut env: JNIEnv,
        _class: JClass,
        host: JString,
    ) -> jint {
        catch_unwind(AssertUnwindSafe(|| {
            let Ok(host) = env.get_string(&host) else {
                return -1;
            };
            let host: String = host.into();
            check_navigation(&host)
        }))
        .unwrap_or(-1)
    }

    #[no_mangle]
    pub extern "system" fn Java_com_psharehub_pip_NativeAdblock_nativeBlockedCount(
        _env: JNIEnv,
        _class: JClass,
    ) -> jlong {
        BLOCKED.load(Ordering::Relaxed).min(i64::MAX as u64) as jlong
    }

    #[no_mangle]
    pub extern "system" fn Java_com_psharehub_pip_NativeAdblock_nativeYoutubeScript(
        mut env: JNIEnv,
        _class: JClass,
    ) -> jstring {
        catch_unwind(AssertUnwindSafe(|| {
            let Some(script) = youtube_script() else {
                return std::ptr::null_mut();
            };
            env.new_string(script)
                .map(|value| value.into_raw())
                .unwrap_or(std::ptr::null_mut())
        }))
        .unwrap_or(std::ptr::null_mut())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_policy_matches_domain_boundaries_and_bundled_gambling_list() {
        initialize();
        for host in [
            "ad88vip6.com",
            "promo.ad88vip6.com",
            "AD88VIP6.COM.",
            "v8v1.021td.com",
        ] {
            assert_eq!(check_navigation(host), 1, "{host}");
        }
        for host in [
            "notad88vip6.com",
            "ad88vip6.com.example.org",
            "google.com",
            "invalid host",
            "",
        ] {
            assert_eq!(check_navigation(host), 0, "{host}");
        }
    }

    #[test]
    fn bundled_network_engine_blocks_known_ad_host_but_allows_control() {
        initialize();
        let engine = ENGINE.get().expect("complete bundled engine initialized");
        let source = "https://fixture.pip.invalid/";
        let blocked = Request::new(
            "https://ad.doubleclick.net/pip-fixture.js",
            source,
            "script",
            "GET",
        )
        .expect("valid fixture request");
        let allowed = Request::new(
            "https://fixture.pip.invalid/control.js",
            source,
            "script",
            "GET",
        )
        .expect("valid control request");
        assert!(engine.check_network_request(&blocked).should_block());
        assert!(!engine.check_network_request(&allowed).should_block());
    }

    #[test]
    fn youtube_video_stream_request_remains_allowed_by_network_only_engine() {
        initialize();
        let request = Request::new(
            "https://rr1---sn.googlevideo.com/videoplayback?expire=9999999999&id=normal-fixture",
            "https://m.youtube.com/watch?v=offline-fixture",
            "media",
            "GET",
        )
        .expect("valid video stream request");
        // Blocking googlevideo broadly would also break ordinary playback.
        assert!(!ENGINE
            .get()
            .expect("native network engine initialized")
            .check_network_request(&request)
            .should_block());
    }

    #[test]
    fn native_engine_respects_network_exceptions_and_resource_types() {
        let engine = engine_from_rules("||ads.example^$script\n@@||ads.example/allowed.js$script");
        let source = "https://fixture.pip.invalid/";
        let request = |url, kind| Request::new(url, source, kind, "GET").expect("valid request");
        assert!(engine
            .check_network_request(&request("https://ads.example/ad.js", "script"))
            .should_block());
        assert!(!engine
            .check_network_request(&request("https://ads.example/allowed.js", "script"))
            .should_block());
        assert!(!engine
            .check_network_request(&request("https://ads.example/cover.jpg", "image"))
            .should_block());
    }

    #[test]
    fn pinned_youtube_scriptlets_compile_for_youtube_hosts_only() {
        initialize();
        let script = youtube_script().expect("bundled YouTube scriptlets compile");
        assert!(script.contains("case \"m.youtube.com\""));
        assert!(script.contains("case \"www.youtube.com\""));
        assert!(!script.contains("case \"example.com\""));
        assert!(
            script.len() > 1_000,
            "scriptlet bundle unexpectedly incomplete"
        );
    }
}
