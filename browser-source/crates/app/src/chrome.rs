//! Chrome UI webview management and IPC message routing.

use wry::dpi::{LogicalPosition, LogicalSize, Position, Size};
use wry::Rect;

pub const RAW_CHROME_HTML: &str = include_str!("../ui/index.html");
pub const LOGO_BASE64: &str = include_str!("../ui/logo.b64");
pub const LOADING_MASCOT_BASE64: &str = include_str!("../ui/assets/mascot-loading.webp.b64");

// Only shipped internal HTML receives the capability. Never inject it into remote pages.
pub fn protect_internal_html(html: String, token: &str) -> String {
    let token = serde_json::to_string(token).unwrap();
    let script = format!("<head><script>window.__pipSend = message => window.ipc.postMessage(JSON.stringify({{token:{token},message}}));</script>");
    html.replace("window.ipc.postMessage", "window.__pipSend")
        .replacen("<head>", &script, 1)
}

pub fn internal_message(body: &str, token: &str) -> Option<evergreen_core::ipc::UiToHostMessage> {
    let envelope: serde_json::Value = serde_json::from_str(body).ok()?;
    if envelope.get("token")?.as_str()? != token {
        return None;
    }
    serde_json::from_str(envelope.get("message")?.as_str()?).ok()
}

pub fn get_chrome_html(search_engine: &str) -> String {
    let display_name = match search_engine.to_lowercase().as_str() {
        "google" => "Google",
        "bing" => "Bing",
        "brave" => "Brave",
        "ecosia" => "Ecosia",
        _ => "Google",
    };
    RAW_CHROME_HTML
        .replace("{{LOGO_BASE64}}", LOGO_BASE64.trim())
        .replace("{{MASCOT_LOADING_BASE64}}", LOADING_MASCOT_BASE64.trim())
        .replace("{{SEARCH_ENGINE_NAME}}", display_name)
}

// Must match the 40px tab strip + 48px navigation row in index.html.
pub const CHROME_HEIGHT: f64 = 88.0;

/// Create logical bounds for child webviews
pub fn create_bounds(x: f64, y: f64, width: f64, height: f64) -> Rect {
    Rect {
        position: Position::Logical(LogicalPosition::new(x, y)),
        size: Size::Logical(LogicalSize::new(width, height)),
    }
}

pub use evergreen_core::tabs::normalize_url;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_url() {
        let template = "https://www.google.com/search?q=%s";

        assert_eq!(
            normalize_url("about:blank", template).unwrap(),
            "about:blank"
        );
        assert_eq!(normalize_url("", template).unwrap(), "evergreen://newtab");
        assert_eq!(
            normalize_url("about:newtab", template).unwrap(),
            "evergreen://newtab"
        );
        assert_eq!(
            normalize_url("about:home", template).unwrap(),
            "evergreen://newtab"
        );
        assert_eq!(
            normalize_url("about:settings", template).unwrap(),
            "evergreen://settings"
        );
        assert_eq!(
            normalize_url("PIP://settings", template).unwrap(),
            "evergreen://settings"
        );
        assert_eq!(
            normalize_url("evergreen://settings", template).unwrap(),
            "evergreen://settings"
        );
        assert_eq!(
            normalize_url("evergreen://newtab", template).unwrap(),
            "evergreen://newtab"
        );
        assert_eq!(
            normalize_url("https://example.com", template).unwrap(),
            "https://example.com"
        );
        assert_eq!(
            normalize_url("http://localhost:3000", template).unwrap(),
            "http://localhost:3000"
        );
        assert_eq!(
            normalize_url("github.com", template).unwrap(),
            "https://github.com"
        );
        assert_eq!(
            normalize_url("rust programming language", template).unwrap(),
            "https://www.google.com/search?q=rust+programming+language"
        );

        assert!(normalize_url("javascript:alert(1)", template).is_err());
        assert!(normalize_url("file:///C:/test.txt", template).is_err());
        assert!(normalize_url("data:text/html,<h1>hi</h1>", template).is_err());
    }

    #[test]
    fn test_get_chrome_html_placeholder() {
        let html_fallback = get_chrome_html("unknown");
        assert!(html_fallback.contains("Tìm kiếm Google hoặc nhập địa chỉ web..."));

        let html_google = get_chrome_html("google");
        assert!(html_google.contains("Tìm kiếm Google hoặc nhập địa chỉ web..."));
        assert!(html_google.contains("data:image/webp;base64,"));
        assert!(!html_google.contains("{{MASCOT_LOADING_BASE64}}"));
    }

    #[test]
    fn internal_ipc_requires_capability() {
        let action = r#"{"action":"RunForkUpdate"}"#;
        assert!(internal_message(action, "secret").is_none());
        let wrong = serde_json::json!({"token":"other", "message":action});
        assert!(internal_message(&wrong.to_string(), "secret").is_none());
        let valid = serde_json::json!({"token":"secret", "message":action});
        assert!(internal_message(&valid.to_string(), "secret").is_some());
        let html = protect_internal_html(
            "<head></head><script>window.ipc.postMessage('x')</script>".into(),
            "secret",
        );
        assert!(html.contains("window.__pipSend('x')"));
        assert_eq!(html.matches("window.ipc.postMessage").count(), 1);
    }
}
