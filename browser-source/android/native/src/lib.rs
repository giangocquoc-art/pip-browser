//! Android's real Brave adblock-rust network engine. Engine initialization runs
//! off the UI thread; interception callbacks query its thread-safe feature set.

use adblock::{
    lists::{FilterSet, ParseOptions, RuleTypes},
    request::Request,
    Engine,
};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    OnceLock,
};

static ENGINE: OnceLock<Engine> = OnceLock::new();
static BLOCKED: AtomicU64 = AtomicU64::new(0);

const EASYLIST: &str = include_str!("../../../crates/app/filters/easylist.txt");

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
            include_str!("../../../crates/app/filters/youtube.txt"),
        ] {
            filters.add_filter_list(
                rules.to_owned(),
                ParseOptions {
                    rule_types: RuleTypes::NetworkOnly,
                    ..Default::default()
                },
            );
        }
        Engine::new_with_filter_set(filters)
    });
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
        sys::{jboolean, jint, jlong, JNI_FALSE, JNI_TRUE},
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
    pub extern "system" fn Java_com_psharehub_pip_NativeAdblock_nativeBlockedCount(
        _env: JNIEnv,
        _class: JClass,
    ) -> jlong {
        BLOCKED.load(Ordering::Relaxed).min(i64::MAX as u64) as jlong
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
