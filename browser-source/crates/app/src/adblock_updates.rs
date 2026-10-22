//! Optional, user-triggered refresh of network-only ad blocking lists.

use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};

const EASYLIST_URL: &str = "https://easylist.to/easylist/easylist.txt";
const UBLOCK_FILTERS_URL: &str =
    "https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/filters.txt";
const UBLOCK_QUICK_FIXES_URL: &str =
    "https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/quick-fixes.txt";
const UBLOCK_UNBREAK_URL: &str =
    "https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/unbreak.txt";
const CACHE_FILE: &str = "adblock-updated-lists.json";
const MAX_LIST_BYTES: usize = 8 * 1024 * 1024;
const MAX_CACHE_BYTES: usize = MAX_LIST_BYTES * 4 + 4096;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdatedLists {
    pub version: u8,
    pub easylist: String,
    pub ublock_filters: String,
    pub ublock_quick_fixes: String,
    pub ublock_unbreak: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSummary {
    pub list_count: usize,
    pub rule_count: usize,
}

impl UpdatedLists {
    fn validate(&self) -> Result<UpdateSummary, String> {
        if self.version != 1 {
            return Err("Bộ nhớ đệm bộ lọc Pip không đúng phiên bản.".into());
        }
        let lists = [
            (&self.easylist, "EasyList"),
            (&self.ublock_filters, "uBlock filters"),
            (&self.ublock_quick_fixes, "uBlock quick-fixes"),
            (&self.ublock_unbreak, "uBlock unbreak"),
        ];
        let mut total = 0usize;
        for (content, name) in lists {
            total = total
                .checked_add(validate_list(content, name)?)
                .ok_or_else(|| "Số quy tắc bộ lọc vượt giới hạn.".to_string())?;
        }
        Ok(UpdateSummary {
            list_count: 4,
            rule_count: total,
        })
    }
}

fn validate_list(content: &str, name: &str) -> Result<usize, String> {
    if content.len() > MAX_LIST_BYTES || content.len() < 1024 {
        return Err(format!("Danh sách {name} có kích thước không hợp lệ."));
    }
    let trimmed = content.trim_start();
    // Adblock Plus format intentionally starts with `[Adblock Plus 2.0]`.
    if trimmed.starts_with('<') || trimmed.starts_with('{') {
        return Err(format!(
            "Máy chủ trả về nội dung không phải danh sách {name}."
        ));
    }
    let rules = content
        .lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty()
                && !line.starts_with('!')
                && !line.starts_with('#')
                && !line.starts_with('[')
        })
        .count();
    if rules < 100 {
        return Err(format!("Danh sách {name} quá ngắn hoặc chưa tải đầy đủ."));
    }
    Ok(rules)
}

pub fn load_cached(directory: &Path) -> Option<UpdatedLists> {
    let path = directory.join(CACHE_FILE);
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() > MAX_CACHE_BYTES {
        return None;
    }
    let lists: UpdatedLists = serde_json::from_slice(&bytes).ok()?;
    lists.validate().ok()?;
    Some(lists)
}

fn download(client: &reqwest::blocking::Client, url: &str, name: &str) -> Result<String, String> {
    let response = client
        .get(url)
        .send()
        .map_err(|_| format!("Không tải được danh sách {name}. Kiểm tra mạng rồi thử lại."))?;
    if !response.status().is_success() {
        return Err(format!(
            "Máy chủ danh sách {name} trả về HTTP {}.",
            response.status().as_u16()
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length as usize > MAX_LIST_BYTES)
    {
        return Err(format!("Danh sách {name} vượt giới hạn dung lượng Pip."));
    }
    let mut bytes = Vec::with_capacity(
        response
            .content_length()
            .unwrap_or(0)
            .min(MAX_LIST_BYTES as u64) as usize,
    );
    response
        .take(MAX_LIST_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| format!("Không đọc được danh sách {name}."))?;
    if bytes.len() > MAX_LIST_BYTES {
        return Err(format!("Danh sách {name} vượt giới hạn dung lượng Pip."));
    }
    String::from_utf8(bytes).map_err(|_| format!("Danh sách {name} không phải UTF-8."))
}

#[cfg(test)]
pub fn fetch_and_store(
    directory: PathBuf,
    proxy_port: Option<u16>,
) -> Result<UpdateSummary, String> {
    fetch_and_store_with_google_dns(directory, false, proxy_port)
}

pub fn fetch_and_store_with_google_dns(
    directory: PathBuf,
    google_dns_enabled: bool,
    proxy_port: Option<u16>,
) -> Result<UpdateSummary, String> {
    let builder = crate::doh_proxy::with_google_dns_proxy(
        reqwest::blocking::Client::builder(),
        google_dns_enabled,
        proxy_port,
    )?;
    let client = builder
        .timeout(std::time::Duration::from_secs(25))
        .user_agent(concat!("Pip-Browser/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|_| "Không tạo được kết nối tải danh sách quảng cáo.".to_string())?;
    let lists = UpdatedLists {
        version: 1,
        easylist: download(&client, EASYLIST_URL, "EasyList")?,
        ublock_filters: download(&client, UBLOCK_FILTERS_URL, "uBlock filters")?,
        ublock_quick_fixes: download(&client, UBLOCK_QUICK_FIXES_URL, "uBlock quick-fixes")?,
        ublock_unbreak: download(&client, UBLOCK_UNBREAK_URL, "uBlock unbreak")?,
    };
    let summary = lists.validate()?;
    let bytes = serde_json::to_vec(&lists)
        .map_err(|_| "Không thể chuẩn bị bộ lọc quảng cáo để lưu.".to_string())?;
    if bytes.len() > MAX_CACHE_BYTES {
        return Err("Bộ lọc quảng cáo vượt giới hạn dung lượng Pip.".into());
    }
    std::fs::create_dir_all(&directory)
        .map_err(|_| "Không thể tạo thư mục lưu bộ lọc quảng cáo.".to_string())?;
    let path = directory.join(CACHE_FILE);
    let staging = directory.join(format!("{CACHE_FILE}.{}.tmp", std::process::id()));
    let backup = directory.join(format!("{CACHE_FILE}.{}.bak", std::process::id()));
    std::fs::write(&staging, bytes)
        .map_err(|_| "Không thể lưu bộ lọc quảng cáo đã tải.".to_string())?;
    let had_previous = path.exists();
    if had_previous {
        std::fs::rename(&path, &backup)
            .map_err(|_| "Không thể thay bộ lọc quảng cáo đã lưu.".to_string())?;
    }
    if let Err(error) = std::fs::rename(&staging, &path) {
        if had_previous {
            let _ = std::fs::rename(&backup, &path);
        }
        let _ = std::fs::remove_file(&staging);
        return Err(format!("Không thể lưu bộ lọc quảng cáo đã tải: {error}"));
    }
    if had_previous {
        let _ = std::fs::remove_file(backup);
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_list() -> String {
        format!(
            "! Title: test\n{}",
            (0..120)
                .map(|i| format!("||ad{i}.example^"))
                .collect::<Vec<_>>()
                .join("\n")
        )
    }

    #[test]
    fn cached_lists_require_all_four_nontrivial_filter_sets() {
        let sample = sample_list();
        let lists = UpdatedLists {
            version: 1,
            easylist: sample.clone(),
            ublock_filters: sample.clone(),
            ublock_quick_fixes: sample.clone(),
            ublock_unbreak: sample,
        };
        let summary = lists.validate().unwrap();
        assert_eq!(summary.list_count, 4);
        assert_eq!(summary.rule_count, 480);
    }

    #[test]
    fn filter_update_rejects_error_pages_and_short_truncated_lists() {
        assert!(validate_list("<!doctype html>error", "EasyList").is_err());
        assert!(validate_list("! Title: short\n||one.example^", "EasyList").is_err());
        assert!(validate_list(&sample_list(), "EasyList").is_ok());
    }

    #[test]
    #[ignore = "downloads official filter lists from the internet"]
    fn live_official_filter_lists_download_validate_and_cache() {
        let directory =
            std::env::temp_dir().join(format!("pip-filter-update-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        let result = fetch_and_store(directory.clone(), None);
        assert!(result.is_ok(), "{result:?}");
        let cached = load_cached(&directory).expect("validated cache");
        assert_eq!(cached.validate().unwrap().list_count, 4);
        let _ = std::fs::remove_dir_all(directory);
    }
}
