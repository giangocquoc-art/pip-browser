//! Small per-profile password vault protected by the current Windows account.
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::{io, path::Path};

const MAX_PASSWORDS: usize = 500;
const MAX_VAULT_BYTES: usize = 2 * 1024 * 1024;
const MAX_SAFE_JS_ID: u64 = (1 << 53) - 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PasswordEntry {
    pub id: u64,
    pub origin: String,
    pub username: String,
    pub password: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PasswordSummary {
    pub id: u64,
    pub origin: String,
    pub username: String,
}

pub fn normalize_origin(value: &str) -> Option<String> {
    if value.len() > 4096 || value.chars().any(char::is_control) {
        return None;
    }
    let uri = value.trim().parse::<wry::http::Uri>().ok()?;
    let scheme = uri.scheme_str()?.to_ascii_lowercase();
    if !matches!(scheme.as_str(), "http" | "https") {
        return None;
    }
    let authority = uri.authority()?.as_str();
    if authority.contains('@') {
        return None;
    }
    let host = uri.host()?.to_ascii_lowercase();
    let port = uri
        .port_u16()
        .filter(|port| !((*port == 80 && scheme == "http") || (*port == 443 && scheme == "https")));
    Some(match port {
        Some(port) => format!("{scheme}://{host}:{port}"),
        None => format!("{scheme}://{host}"),
    })
}

pub fn summaries(entries: &[PasswordEntry]) -> Vec<PasswordSummary> {
    entries
        .iter()
        .map(|entry| PasswordSummary {
            id: entry.id,
            origin: entry.origin.clone(),
            username: entry.username.clone(),
        })
        .collect()
}

pub fn find(entries: &[PasswordEntry], id: u64) -> Option<&PasswordEntry> {
    entries.iter().find(|entry| entry.id == id)
}

pub fn matching_origin<'a>(entries: &'a [PasswordEntry], url: &str) -> Vec<&'a PasswordEntry> {
    let Some(origin) = normalize_origin(url) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter(|entry| entry.origin == origin)
        .collect()
}

pub fn upsert(
    entries: &mut Vec<PasswordEntry>,
    origin: &str,
    username: &str,
    password: &str,
) -> Result<u64, String> {
    let origin = normalize_origin(origin).ok_or("Nhập địa chỉ HTTP hoặc HTTPS hợp lệ.")?;
    let username = username.trim();
    if username.is_empty() || username.chars().any(char::is_control) || username.len() > 1024 {
        return Err(
            "Tên đăng nhập phải có từ 1 đến 1024 byte và không chứa ký tự điều khiển.".into(),
        );
    }
    if password.is_empty() || password.len() > 4096 || password.chars().any(char::is_control) {
        return Err("Mật khẩu phải có từ 1 đến 4096 byte và không chứa ký tự điều khiển.".into());
    }

    if let Some(existing) = entries
        .iter_mut()
        .find(|entry| entry.origin == origin && entry.username == username)
    {
        existing.password = password.to_string();
        return Ok(existing.id);
    }
    if entries.len() >= MAX_PASSWORDS {
        return Err("Kho Pip lưu tối đa 500 mật khẩu cho mỗi hồ sơ.".into());
    }

    let mut id = OsRng.next_u64() & MAX_SAFE_JS_ID;
    while id == 0 || entries.iter().any(|entry| entry.id == id) {
        id = OsRng.next_u64() & MAX_SAFE_JS_ID;
    }
    entries.push(PasswordEntry {
        id,
        origin,
        username: username.to_string(),
        password: password.to_string(),
    });
    Ok(id)
}

pub fn load(path: &Path) -> Result<Vec<PasswordEntry>, String> {
    let protected = match std::fs::read_to_string(path) {
        Ok(value) => value,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err("Không đọc được kho mật khẩu Pip.".into()),
    };
    if protected.len() > MAX_VAULT_BYTES * 2 {
        return Err("Kho mật khẩu Pip vượt giới hạn dung lượng.".into());
    }
    let plaintext = crate::sync::unprotect_local_data(&protected)
        .map_err(|_| "Không thể giải mã kho mật khẩu trên tài khoản Windows này.".to_string())?;
    if plaintext.len() > MAX_VAULT_BYTES {
        return Err("Kho mật khẩu Pip vượt giới hạn dung lượng.".into());
    }
    let entries: Vec<PasswordEntry> = serde_json::from_slice(&plaintext)
        .map_err(|_| "Kho mật khẩu Pip không hợp lệ.".to_string())?;
    if entries.len() > MAX_PASSWORDS
        || entries.iter().any(|entry| {
            entry.id == 0
                || entry.id > MAX_SAFE_JS_ID
                || normalize_origin(&entry.origin).as_deref() != Some(entry.origin.as_str())
                || entry.username.is_empty()
                || entry.username.len() > 1024
                || entry.password.is_empty()
                || entry.password.len() > 4096
        })
    {
        return Err("Kho mật khẩu Pip chứa dữ liệu không hợp lệ.".into());
    }
    Ok(entries)
}

pub fn save(path: &Path, entries: &[PasswordEntry]) -> Result<(), String> {
    if entries.len() > MAX_PASSWORDS {
        return Err("Kho Pip lưu tối đa 500 mật khẩu cho mỗi hồ sơ.".into());
    }
    let plaintext =
        serde_json::to_vec(entries).map_err(|_| "Không mã hóa được kho mật khẩu Pip.")?;
    if plaintext.len() > MAX_VAULT_BYTES {
        return Err("Kho mật khẩu Pip vượt giới hạn dung lượng.".into());
    }
    let protected = crate::sync::protect_local_data(&plaintext)
        .map_err(|_| "Không thể bảo vệ kho mật khẩu bằng tài khoản Windows này.".to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| "Không thể tạo thư mục hồ sơ Pip.")?;
    }
    let staging = path.with_file_name(format!("passwords-{}.tmp", std::process::id()));
    std::fs::write(&staging, protected)
        .and_then(|_| std::fs::rename(staging, path))
        .map_err(|_| "Không thể lưu kho mật khẩu Pip.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_origins_are_normalized_and_never_accept_active_schemes() {
        assert_eq!(
            normalize_origin("https://Example.com/login?q=1"),
            Some("https://example.com".into())
        );
        assert_eq!(
            normalize_origin("HTTPS://Example.com:443/login"),
            Some("https://example.com".into())
        );
        assert_eq!(
            normalize_origin("http://localhost:8080/path"),
            Some("http://localhost:8080".into())
        );
        assert_eq!(
            normalize_origin("http://[::1]:8080/path"),
            Some("http://[::1]:8080".into())
        );
        assert!(normalize_origin("javascript:alert(1)").is_none());
        assert!(normalize_origin("https://user:pass@example.com").is_none());
        assert!(normalize_origin("https://example.com\n").is_none());
    }

    #[test]
    fn password_vault_summaries_never_include_passwords() {
        let mut entries = Vec::new();
        let id = upsert(
            &mut entries,
            "https://example.com/login",
            "giang",
            "secret-value",
        )
        .unwrap();
        let summaries = summaries(&entries);
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].id, id);
        assert_eq!(summaries[0].origin, "https://example.com");
        let json = serde_json::to_string(&summaries).unwrap();
        assert!(json.contains("giang"));
        assert!(!json.contains("secret-value"));
    }

    #[test]
    fn same_site_and_username_update_in_place() {
        let mut entries = Vec::new();
        let first = upsert(&mut entries, "https://example.com/a", "giang", "old").unwrap();
        let second = upsert(&mut entries, "https://example.com/b", "giang", "new").unwrap();
        assert_eq!(first, second);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].password, "new");
    }

    #[test]
    fn password_picker_matches_only_the_exact_origin() {
        let mut entries = Vec::new();
        upsert(&mut entries, "https://example.com", "one", "secret-one").unwrap();
        upsert(&mut entries, "https://example.com", "two", "secret-two").unwrap();
        upsert(&mut entries, "http://example.com", "http", "secret-http").unwrap();
        upsert(
            &mut entries,
            "https://example.com:8443",
            "port",
            "secret-port",
        )
        .unwrap();

        let matches = matching_origin(&entries, "https://EXAMPLE.com/login");
        assert_eq!(matches.len(), 2);
        assert!(matches
            .iter()
            .all(|entry| entry.origin == "https://example.com"));
        assert!(matching_origin(&entries, "https://sub.example.com").is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn encrypted_profile_vault_roundtrips_and_replaces_existing_file() {
        let root = std::env::temp_dir().join(format!(
            "pip-password-vault-test-{}-{}",
            std::process::id(),
            OsRng.next_u64()
        ));
        let path = root.join("passwords.bin");
        let mut entries = Vec::new();
        upsert(
            &mut entries,
            "https://example.com",
            "account",
            "first-secret",
        )
        .unwrap();
        save(&path, &entries).unwrap();
        let protected = std::fs::read_to_string(&path).unwrap();
        assert!(!protected.contains("first-secret"));
        assert_eq!(load(&path).unwrap(), entries);

        upsert(
            &mut entries,
            "https://example.com",
            "account",
            "second-secret",
        )
        .unwrap();
        save(&path, &entries).unwrap();
        assert_eq!(load(&path).unwrap(), entries);
        let _ = std::fs::remove_dir_all(root);
    }
}
