//! User-triggered, read-only checks for published Pip releases.

use semver::Version;
use serde::{Deserialize, Serialize};

const RELEASE_API: &str =
    "https://api.github.com/repos/giangocquoc-art/pip-browser/releases/latest";
const MAX_RELEASE_METADATA_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub update_available: bool,
    pub release_url: String,
}

#[derive(Deserialize)]
struct LatestRelease {
    tag_name: String,
    html_url: String,
    draft: bool,
    prerelease: bool,
}

fn parse_release(
    current: &str,
    tag: &str,
    html_url: &str,
    draft: bool,
    prerelease: bool,
) -> Result<AppUpdateInfo, String> {
    if draft || prerelease {
        return Err("GitHub chưa có bản Pip phát hành ổn định.".into());
    }
    let latest = tag
        .strip_prefix('v')
        .unwrap_or(tag)
        .parse::<Version>()
        .map_err(|_| "Phiên bản Pip trên GitHub không hợp lệ.".to_string())?;
    let current_version =
        Version::parse(current).map_err(|_| "Phiên bản Pip hiện tại không hợp lệ.".to_string())?;
    let release = reqwest::Url::parse(html_url)
        .map_err(|_| "Đường dẫn bản phát hành Pip không hợp lệ.".to_string())?;
    if release.scheme() != "https"
        || release.host_str() != Some("github.com")
        || release.path() != format!("/giangocquoc-art/pip-browser/releases/tag/{tag}")
    {
        return Err("Đường dẫn bản phát hành Pip không thuộc GitHub chính thức.".into());
    }
    Ok(AppUpdateInfo {
        current_version: current.to_string(),
        latest_version: latest.to_string(),
        update_available: latest > current_version,
        release_url: release.to_string(),
    })
}

pub fn check(proxy_port: Option<u16>) -> Result<AppUpdateInfo, String> {
    check_with_google_dns(false, proxy_port)
}

pub fn check_with_google_dns(
    google_dns_enabled: bool,
    proxy_port: Option<u16>,
) -> Result<AppUpdateInfo, String> {
    let builder = crate::doh_proxy::with_google_dns_proxy(
        reqwest::blocking::Client::builder(),
        google_dns_enabled,
        proxy_port,
    )?;
    let client = builder
        .timeout(std::time::Duration::from_secs(12))
        .user_agent(concat!("Pip-Browser/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|_| "Không tạo được kết nối kiểm tra cập nhật Pip.".to_string())?;
    let response = client
        .get(RELEASE_API)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .map_err(|_| "Không kết nối được GitHub để kiểm tra cập nhật Pip.".to_string())?;
    if !response.status().is_success() {
        return Err(format!(
            "GitHub chưa trả được thông tin phát hành Pip (HTTP {}).",
            response.status().as_u16()
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length as usize > MAX_RELEASE_METADATA_BYTES)
    {
        return Err("Thông tin phát hành Pip vượt giới hạn an toàn.".into());
    }
    let body = response
        .bytes()
        .map_err(|_| "Không đọc được thông tin phát hành Pip.".to_string())?;
    if body.len() > MAX_RELEASE_METADATA_BYTES {
        return Err("Thông tin phát hành Pip vượt giới hạn an toàn.".into());
    }
    let release: LatestRelease = serde_json::from_slice(&body)
        .map_err(|_| "Thông tin phát hành Pip không hợp lệ.".to_string())?;
    parse_release(
        env!("CARGO_PKG_VERSION"),
        &release.tag_name,
        &release.html_url,
        release.draft,
        release.prerelease,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_check_compares_semantic_versions_and_trusts_only_the_project_release_page() {
        let available = parse_release(
            "0.1.9",
            "v0.1.10",
            "https://github.com/giangocquoc-art/pip-browser/releases/tag/v0.1.10",
            false,
            false,
        )
        .unwrap();
        assert!(available.update_available);
        assert_eq!(available.latest_version, "0.1.10");

        let current = parse_release(
            "0.1.13",
            "v0.1.9",
            "https://github.com/giangocquoc-art/pip-browser/releases/tag/v0.1.9",
            false,
            false,
        )
        .unwrap();
        assert!(!current.update_available);

        assert!(parse_release(
            "0.1.9",
            "v0.1.10",
            "https://attacker.example/releases/tag/v0.1.10",
            false,
            false,
        )
        .is_err());
    }

    #[test]
    fn prereleases_are_not_offered_as_stable_updates() {
        assert!(parse_release(
            "0.1.9",
            "v0.2.0-rc.1",
            "https://github.com/giangocquoc-art/pip-browser/releases/tag/v0.2.0-rc.1",
            false,
            true,
        )
        .is_err());
    }

    #[test]
    #[ignore = "requires outbound HTTPS access to GitHub Releases"]
    fn live_release_api_returns_a_trusted_pip_release() {
        let info = check(None).unwrap();
        assert!(!info.latest_version.is_empty());
        assert!(info
            .release_url
            .starts_with("https://github.com/giangocquoc-art/pip-browser/releases/tag/"));
    }
}
