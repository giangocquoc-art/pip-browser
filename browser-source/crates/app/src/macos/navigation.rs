//! Reuses the existing engine-independent omnibox policy, then restricts this
//! preview to pages that WKWebView can actually handle.

pub const SEARCH: &str = "https://www.google.com/search?q=%s";

pub fn destination(input: &str) -> Result<String, String> {
    if input.len() > 8192 || input.chars().any(char::is_control) {
        return Err("Địa chỉ quá dài hoặc chứa ký tự không hợp lệ.".into());
    }
    let url = evergreen_core::tabs::normalize_url(input, SEARCH)?;
    if url == "evergreen://newtab" || url == "about:blank" {
        return Ok(url);
    }
    if !allows_page_url(&url) {
        return Err("Bản macOS hiện chỉ mở địa chỉ HTTP/HTTPS.".into());
    }
    Ok(url)
}

pub fn allows_page_url(url: &str) -> bool {
    if url == "about:blank" {
        return true;
    }
    if url.len() > 8192 || url.chars().any(char::is_control) {
        return false;
    }
    let Ok(uri) = url.parse::<wry::http::Uri>() else {
        return false;
    };
    matches!(uri.scheme_str(), Some("http" | "https"))
        && uri.host().is_some_and(|host| !host.is_empty())
        && uri
            .authority()
            .is_some_and(|authority| !authority.as_str().contains('@'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_google_search_and_new_tab_policy_is_reused() {
        assert_eq!(
            destination("rust browser").unwrap(),
            "https://www.google.com/search?q=rust+browser"
        );
        assert_eq!(destination("github.com").unwrap(), "https://github.com");
        assert_eq!(destination("").unwrap(), "evergreen://newtab");
        assert_eq!(
            destination("http://localhost:3000").unwrap(),
            "http://localhost:3000"
        );
    }

    #[test]
    fn untrusted_navigation_never_opens_local_files_or_credential_urls() {
        for url in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,test",
            "https://name:secret@example.com/",
            "https://",
            "https://example.com/\nfile",
        ] {
            assert!(!allows_page_url(url), "unexpected allowed page: {url}");
            assert!(
                destination(url).is_err(),
                "unexpected omnibox navigation: {url}"
            );
        }
        assert!(allows_page_url("https://example.com/path?q=one"));
        assert!(destination("pip://settings").is_err());
    }
}
