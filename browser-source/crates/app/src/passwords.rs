//! Small per-profile password vault protected by the current Windows account.
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_PASSWORDS: usize = 500;
const MAX_PASSWORD_TOMBSTONES: usize = 2_000;
const MAX_VAULT_BYTES: usize = 2 * 1024 * 1024;
/// The existing Worker accepts a 1 MiB hex envelope. Leave room for AES-GCM
/// overhead and JSON/hex expansion when projecting a password category.
const MAX_PASSWORD_SYNC_BYTES: usize = 450 * 1024;
const MAX_SAFE_JS_ID: u64 = (1 << 53) - 1;
const PASSWORD_VAULT_VERSION: u8 = 2;
const PASSWORD_SYNC_VERSION: u8 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PasswordEntry {
    pub id: u64,
    pub origin: String,
    pub username: String,
    pub password: String,
    /// Unix time in milliseconds, strictly above this identity's prior update
    /// or deletion when persisted through `save`.
    #[serde(default)]
    pub updated_at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PasswordTombstone {
    pub origin: String,
    pub username: String,
    pub deleted_at_ms: u64,
}

/// Inner password category projection. It contains no local UI IDs; identity
/// for conflict resolution is normalized origin + exact username.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PasswordSyncSnapshot {
    pub version: u8,
    pub entries: Vec<PasswordSyncEntry>,
    pub tombstones: Vec<PasswordTombstone>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PasswordSyncEntry {
    pub origin: String,
    pub username: String,
    pub password: String,
    pub updated_at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PasswordVaultDocument {
    version: u8,
    entries: Vec<PasswordEntry>,
    #[serde(default)]
    tombstones: Vec<PasswordTombstone>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct PasswordKey {
    origin: String,
    username: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PasswordSyncError {
    InvalidData(&'static str),
    ConcurrentUpdate,
}

impl std::fmt::Display for PasswordSyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidData(message) => write!(f, "Invalid password Sync data: {message}"),
            Self::ConcurrentUpdate => f.write_str(
                "Conflicting password changes have the same timestamp; resolve them locally and sync again",
            ),
        }
    }
}

impl std::error::Error for PasswordSyncError {}

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

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn next_timestamp(now: u64, previous: u64) -> u64 {
    now.max(previous.saturating_add(1))
}

fn password_key(origin: &str, username: &str) -> PasswordKey {
    PasswordKey {
        origin: origin.to_string(),
        username: username.to_string(),
    }
}

fn validate_identity(origin: &str, username: &str) -> Result<PasswordKey, PasswordSyncError> {
    let normalized =
        normalize_origin(origin).ok_or(PasswordSyncError::InvalidData("invalid HTTP(S) origin"))?;
    if normalized != origin {
        return Err(PasswordSyncError::InvalidData("origin is not normalized"));
    }
    if username.is_empty()
        || username.len() > 1024
        || username.chars().any(char::is_control)
        || username.trim() != username
    {
        return Err(PasswordSyncError::InvalidData("invalid username"));
    }
    Ok(password_key(origin, username))
}

fn validate_timestamp(timestamp: u64) -> Result<(), PasswordSyncError> {
    if timestamp == u64::MAX {
        return Err(PasswordSyncError::InvalidData("timestamp is out of range"));
    }
    Ok(())
}

fn validate_password(password: &str) -> Result<(), PasswordSyncError> {
    if password.is_empty() || password.len() > 4096 || password.chars().any(char::is_control) {
        return Err(PasswordSyncError::InvalidData(
            "invalid password length or controls",
        ));
    }
    Ok(())
}

fn validate_sync_snapshot(
    mut snapshot: PasswordSyncSnapshot,
) -> Result<PasswordSyncSnapshot, PasswordSyncError> {
    if snapshot.version != PASSWORD_SYNC_VERSION {
        return Err(PasswordSyncError::InvalidData(
            "unsupported category version",
        ));
    }
    if snapshot.entries.len() > MAX_PASSWORDS || snapshot.tombstones.len() > MAX_PASSWORD_TOMBSTONES
    {
        return Err(PasswordSyncError::InvalidData("record limits exceeded"));
    }

    let mut entries = BTreeMap::new();
    for entry in snapshot.entries.drain(..) {
        let key = validate_identity(&entry.origin, &entry.username)?;
        validate_password(&entry.password)?;
        validate_timestamp(entry.updated_at_ms)?;
        if entries.insert(key, entry).is_some() {
            return Err(PasswordSyncError::InvalidData(
                "duplicate password identity",
            ));
        }
    }
    snapshot.entries = entries.into_values().collect();

    let mut tombstones = BTreeMap::<PasswordKey, PasswordTombstone>::new();
    for tombstone in snapshot.tombstones.drain(..) {
        let key = validate_identity(&tombstone.origin, &tombstone.username)?;
        validate_timestamp(tombstone.deleted_at_ms)?;
        tombstones
            .entry(key)
            .and_modify(|current| {
                current.deleted_at_ms = current.deleted_at_ms.max(tombstone.deleted_at_ms)
            })
            .or_insert(tombstone);
    }
    snapshot.tombstones = tombstones.into_values().collect();

    let encoded = serde_json::to_vec(&snapshot)
        .map_err(|_| PasswordSyncError::InvalidData("cannot encode category"))?;
    if encoded.len() > MAX_PASSWORD_SYNC_BYTES {
        return Err(PasswordSyncError::InvalidData(
            "category exceeds safe size limit",
        ));
    }
    Ok(snapshot)
}

/// Build an allowlisted, ID-free password category from the locally DPAPI-
/// protected vault. Callers must encrypt the returned value before networking.
pub fn sync_projection(path: &Path) -> Result<PasswordSyncSnapshot, String> {
    let document = load_document(path)?;
    validate_sync_snapshot(PasswordSyncSnapshot {
        version: PASSWORD_SYNC_VERSION,
        entries: document
            .entries
            .iter()
            .map(|entry| PasswordSyncEntry {
                origin: entry.origin.clone(),
                username: entry.username.clone(),
                password: entry.password.clone(),
                updated_at_ms: entry.updated_at_ms,
            })
            .collect(),
        tombstones: document.tombstones,
    })
    .map_err(|error| error.to_string())
}

/// Merge two validated password projections by canonical origin + exact
/// username. A strictly newer re-add beats a tombstone; deletion wins ties.
/// Conflicting live values with identical timestamps fail rather than silently
/// choosing and discarding one password.
pub fn merge_sync_snapshots(
    local: PasswordSyncSnapshot,
    remote: PasswordSyncSnapshot,
) -> Result<PasswordSyncSnapshot, PasswordSyncError> {
    let local = validate_sync_snapshot(local)?;
    let remote = validate_sync_snapshot(remote)?;
    let mut entries = BTreeMap::<PasswordKey, PasswordSyncEntry>::new();
    for entry in local.entries.into_iter().chain(remote.entries) {
        let key = password_key(&entry.origin, &entry.username);
        if let Some(current) = entries.get_mut(&key) {
            if entry.updated_at_ms > current.updated_at_ms {
                *current = entry;
            } else if entry.updated_at_ms == current.updated_at_ms
                && (entry.password != current.password || entry.origin != current.origin)
            {
                return Err(PasswordSyncError::ConcurrentUpdate);
            }
        } else {
            entries.insert(key, entry);
        }
    }
    let mut tombstones = BTreeMap::<PasswordKey, PasswordTombstone>::new();
    for tombstone in local.tombstones.into_iter().chain(remote.tombstones) {
        let key = password_key(&tombstone.origin, &tombstone.username);
        tombstones
            .entry(key)
            .and_modify(|current| {
                current.deleted_at_ms = current.deleted_at_ms.max(tombstone.deleted_at_ms)
            })
            .or_insert(tombstone);
    }
    entries.retain(|key, entry| {
        !tombstones
            .get(key)
            .is_some_and(|deleted| deleted.deleted_at_ms >= entry.updated_at_ms)
    });
    // A newer re-add makes an older deletion obsolete. Removing it also keeps
    // the bounded tombstone set from growing forever after legitimate restores.
    tombstones.retain(|key, deleted| {
        !entries
            .get(key)
            .is_some_and(|entry| entry.updated_at_ms > deleted.deleted_at_ms)
    });
    validate_sync_snapshot(PasswordSyncSnapshot {
        version: PASSWORD_SYNC_VERSION,
        entries: entries.into_values().collect(),
        tombstones: tombstones.into_values().collect(),
    })
}

/// Merge an encrypted-sync result with the latest local vault and persist it
/// using this Windows account's DPAPI protection. Existing local IDs are kept;
/// remote-only records receive fresh local IDs.
pub fn apply_sync_snapshot(
    path: &Path,
    incoming: PasswordSyncSnapshot,
) -> Result<Vec<PasswordEntry>, String> {
    let mut document = load_document(path)?;
    let local = validate_sync_snapshot(PasswordSyncSnapshot {
        version: PASSWORD_SYNC_VERSION,
        entries: document
            .entries
            .iter()
            .map(|entry| PasswordSyncEntry {
                origin: entry.origin.clone(),
                username: entry.username.clone(),
                password: entry.password.clone(),
                updated_at_ms: entry.updated_at_ms,
            })
            .collect(),
        tombstones: document.tombstones.clone(),
    })
    .map_err(|error| error.to_string())?;
    let merged = merge_sync_snapshots(local, incoming).map_err(|error| error.to_string())?;
    let mut local_ids: BTreeMap<PasswordKey, u64> = document
        .entries
        .drain(..)
        .map(|entry| (password_key(&entry.origin, &entry.username), entry.id))
        .collect();
    let mut next_entries = Vec::with_capacity(merged.entries.len());
    for entry in merged.entries {
        let id = local_ids
            .remove(&password_key(&entry.origin, &entry.username))
            .unwrap_or_else(|| new_local_id(&next_entries));
        next_entries.push(PasswordEntry {
            id,
            origin: entry.origin,
            username: entry.username,
            password: entry.password,
            updated_at_ms: entry.updated_at_ms,
        });
    }
    document.entries = next_entries;
    document.tombstones = merged.tombstones;
    save_document(path, &document)?;
    Ok(document.entries)
}

fn new_local_id(entries: &[PasswordEntry]) -> u64 {
    let mut id = OsRng.next_u64() & MAX_SAFE_JS_ID;
    while id == 0 || entries.iter().any(|entry| entry.id == id) {
        id = OsRng.next_u64() & MAX_SAFE_JS_ID;
    }
    id
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
        existing.updated_at_ms = next_timestamp(now_ms(), existing.updated_at_ms);
        return Ok(existing.id);
    }
    if entries.len() >= MAX_PASSWORDS {
        return Err("Kho Pip lưu tối đa 500 mật khẩu cho mỗi hồ sơ.".into());
    }

    let id = new_local_id(entries);
    entries.push(PasswordEntry {
        id,
        origin,
        username: username.to_string(),
        password: password.to_string(),
        updated_at_ms: now_ms(),
    });
    Ok(id)
}

pub fn load(path: &Path) -> Result<Vec<PasswordEntry>, String> {
    Ok(load_document(path)?.entries)
}

pub fn save(path: &Path, entries: &[PasswordEntry]) -> Result<(), String> {
    if entries.len() > MAX_PASSWORDS {
        return Err("Kho Pip lưu tối đa 500 mật khẩu cho mỗi hồ sơ.".into());
    }
    let previous = load_document(path)?;
    let now = now_ms();
    let mut previous_entries: BTreeMap<PasswordKey, PasswordEntry> = previous
        .entries
        .into_iter()
        .map(|entry| (password_key(&entry.origin, &entry.username), entry))
        .collect();
    let mut tombstones: BTreeMap<PasswordKey, PasswordTombstone> = previous
        .tombstones
        .into_iter()
        .map(|tombstone| {
            (
                password_key(&tombstone.origin, &tombstone.username),
                tombstone,
            )
        })
        .collect();
    let mut seen_keys = std::collections::HashSet::new();
    let mut seen_ids = std::collections::HashSet::new();
    let mut next_entries = Vec::with_capacity(entries.len());

    for entry in entries {
        validate_entry(entry)?;
        if !seen_keys.insert(password_key(&entry.origin, &entry.username)) {
            return Err("Kho mật khẩu Pip chứa tài khoản trùng lặp.".into());
        }
        if !seen_ids.insert(entry.id) {
            return Err("Kho mật khẩu Pip chứa mã bản ghi trùng lặp.".into());
        }
        let key = password_key(&entry.origin, &entry.username);
        let old = previous_entries.remove(&key);
        let deleted_at = tombstones
            .get(&key)
            .map(|value| value.deleted_at_ms)
            .unwrap_or(0);
        let mut saved = entry.clone();
        saved.updated_at_ms = match old {
            Some(old) if old.password == entry.password => {
                old.updated_at_ms.max(entry.updated_at_ms)
            }
            Some(old) => next_timestamp(now, old.updated_at_ms.max(deleted_at)),
            None => entry.updated_at_ms.max(next_timestamp(now, deleted_at)),
        };
        if let Some(tombstone) = tombstones.get(&key) {
            if saved.updated_at_ms > tombstone.deleted_at_ms {
                tombstones.remove(&key);
            }
        }
        next_entries.push(saved);
    }

    for (key, old) in previous_entries {
        let deleted_at_ms = next_timestamp(
            now,
            old.updated_at_ms.max(
                tombstones
                    .get(&key)
                    .map(|value| value.deleted_at_ms)
                    .unwrap_or(0),
            ),
        );
        tombstones.insert(
            key,
            PasswordTombstone {
                origin: old.origin,
                username: old.username,
                deleted_at_ms,
            },
        );
    }
    if tombstones.len() > MAX_PASSWORD_TOMBSTONES {
        return Err("Kho Pip có quá nhiều mục đã xóa; đồng bộ trước khi xóa thêm.".into());
    }

    let document = PasswordVaultDocument {
        version: PASSWORD_VAULT_VERSION,
        entries: next_entries,
        tombstones: tombstones.into_values().collect(),
    };
    save_document(path, &document)
}

fn validate_entry(entry: &PasswordEntry) -> Result<(), String> {
    if entry.id == 0
        || entry.id > MAX_SAFE_JS_ID
        || normalize_origin(&entry.origin).as_deref() != Some(entry.origin.as_str())
        || entry.username.is_empty()
        || entry.username.trim() != entry.username
        || entry.username.len() > 1024
        || entry.username.chars().any(char::is_control)
        || entry.password.is_empty()
        || entry.password.len() > 4096
        || entry.password.chars().any(char::is_control)
        || entry.updated_at_ms == u64::MAX
    {
        return Err("Kho mật khẩu Pip chứa dữ liệu không hợp lệ.".into());
    }
    Ok(())
}

fn empty_document() -> PasswordVaultDocument {
    PasswordVaultDocument {
        version: PASSWORD_VAULT_VERSION,
        entries: Vec::new(),
        tombstones: Vec::new(),
    }
}

fn load_document(path: &Path) -> Result<PasswordVaultDocument, String> {
    let protected = match std::fs::read_to_string(path) {
        Ok(value) => value,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(empty_document()),
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
    let value: serde_json::Value = serde_json::from_slice(&plaintext)
        .map_err(|_| "Kho mật khẩu Pip không hợp lệ.".to_string())?;
    let document = if value.is_array() {
        // Legacy local vaults were a DPAPI-protected Vec<PasswordEntry>.
        let entries: Vec<PasswordEntry> = serde_json::from_value(value)
            .map_err(|_| "Kho mật khẩu Pip không hợp lệ.".to_string())?;
        PasswordVaultDocument {
            version: PASSWORD_VAULT_VERSION,
            entries,
            tombstones: Vec::new(),
        }
    } else {
        let document: PasswordVaultDocument = serde_json::from_value(value)
            .map_err(|_| "Kho mật khẩu Pip không hợp lệ.".to_string())?;
        if document.version != PASSWORD_VAULT_VERSION {
            return Err("Phiên bản kho mật khẩu Pip chưa được hỗ trợ.".into());
        }
        document
    };
    validate_document(document)
}

fn validate_document(mut document: PasswordVaultDocument) -> Result<PasswordVaultDocument, String> {
    if document.version != PASSWORD_VAULT_VERSION
        || document.entries.len() > MAX_PASSWORDS
        || document.tombstones.len() > MAX_PASSWORD_TOMBSTONES
    {
        return Err("Kho mật khẩu Pip chứa dữ liệu không hợp lệ hoặc vượt giới hạn.".into());
    }
    let mut keys = std::collections::HashSet::new();
    let mut ids = std::collections::HashSet::new();
    for entry in &document.entries {
        validate_entry(entry)?;
        if !keys.insert(password_key(&entry.origin, &entry.username)) || !ids.insert(entry.id) {
            return Err("Kho mật khẩu Pip chứa bản ghi trùng lặp.".into());
        }
    }
    let mut tombstones = BTreeMap::new();
    for tombstone in document.tombstones {
        if tombstone.deleted_at_ms == u64::MAX {
            return Err("Kho mật khẩu Pip chứa thời điểm xóa không hợp lệ.".into());
        }
        let key = validate_identity(&tombstone.origin, &tombstone.username)
            .map_err(|_| "Kho mật khẩu Pip chứa mục xóa không hợp lệ.")?;
        tombstones
            .entry(key)
            .and_modify(|current: &mut PasswordTombstone| {
                current.deleted_at_ms = current.deleted_at_ms.max(tombstone.deleted_at_ms)
            })
            .or_insert(tombstone);
    }
    if tombstones.len() > MAX_PASSWORD_TOMBSTONES {
        return Err("Kho mật khẩu Pip vượt giới hạn mục đã xóa.".into());
    }
    document.tombstones = tombstones.into_values().collect();
    Ok(document)
}

fn save_document(path: &Path, document: &PasswordVaultDocument) -> Result<(), String> {
    let document = validate_document(document.clone())?;
    let plaintext =
        serde_json::to_vec(&document).map_err(|_| "Không mã hóa được kho mật khẩu Pip.")?;
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

/// Remove Pip's password vault for a deleted profile. Validate the identifier
/// before joining it to the profiles directory so cleanup cannot escape it.
pub fn remove_profile_vault(profiles_directory: &Path, profile_id: &str) -> Result<(), String> {
    if !evergreen_core::settings::valid_profile_id(profile_id) {
        return Err("Mã hồ sơ không hợp lệ; không xóa kho mật khẩu Pip.".into());
    }
    let vault = profiles_directory.join(profile_id).join("passwords.bin");
    match std::fs::remove_file(vault) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("Không thể xóa kho mật khẩu Pip của hồ sơ.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sync_entry(password: &str, updated_at_ms: u64) -> PasswordSyncEntry {
        PasswordSyncEntry {
            origin: "https://example.com".into(),
            username: "account".into(),
            password: password.into(),
            updated_at_ms,
        }
    }

    fn sync_snapshot(
        entries: Vec<PasswordSyncEntry>,
        tombstones: Vec<PasswordTombstone>,
    ) -> PasswordSyncSnapshot {
        PasswordSyncSnapshot {
            version: PASSWORD_SYNC_VERSION,
            entries,
            tombstones,
        }
    }

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
        assert!(entries[0].updated_at_ms > 0);
        let previous_timestamp = entries[0].updated_at_ms;
        upsert(&mut entries, "https://example.com", "giang", "newer").unwrap();
        assert!(entries[0].updated_at_ms > previous_timestamp);
    }

    #[test]
    fn sync_projection_is_id_free_and_merge_resolves_deletions_and_readds() {
        let entry = sync_entry("restored", 12);
        let deletion = PasswordTombstone {
            origin: "https://example.com".into(),
            username: "account".into(),
            deleted_at_ms: 11,
        };
        let merged = merge_sync_snapshots(
            sync_snapshot(vec![entry.clone()], vec![]),
            sync_snapshot(vec![], vec![deletion.clone()]),
        )
        .unwrap();
        assert_eq!(merged.entries, vec![entry]);
        assert!(merged.tombstones.is_empty());

        let deleted = merge_sync_snapshots(
            sync_snapshot(vec![sync_entry("old", 10)], vec![]),
            sync_snapshot(vec![], vec![deletion.clone()]),
        )
        .unwrap();
        assert!(deleted.entries.is_empty());
        assert_eq!(deleted.tombstones, vec![deletion]);

        let serialized = serde_json::to_string(&deleted).unwrap();
        assert!(!serialized.contains("\"id\""));
    }

    #[test]
    fn password_sync_rejects_same_timestamp_conflicts_and_oversize_payloads() {
        let conflict = merge_sync_snapshots(
            sync_snapshot(vec![sync_entry("local", 7)], vec![]),
            sync_snapshot(vec![sync_entry("remote", 7)], vec![]),
        );
        assert_eq!(conflict, Err(PasswordSyncError::ConcurrentUpdate));

        let oversized = sync_snapshot(
            (0..MAX_PASSWORDS)
                .map(|index| PasswordSyncEntry {
                    origin: format!("https://site{index}.example.com"),
                    username: format!("account-{index}"),
                    password: "x".repeat(4096),
                    updated_at_ms: index as u64,
                })
                .collect(),
            vec![],
        );
        assert!(matches!(
            validate_sync_snapshot(oversized),
            Err(PasswordSyncError::InvalidData(
                "category exceeds safe size limit"
            ))
        ));
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

    #[test]
    fn profile_vault_removal_is_idempotent_and_rejects_path_traversal() {
        let root = std::env::temp_dir().join(format!(
            "pip-password-remove-test-{}-{}",
            std::process::id(),
            OsRng.next_u64()
        ));
        let profile_dir = root.join("profile-1");
        let other_dir = root.join("other");
        std::fs::create_dir_all(&profile_dir).unwrap();
        std::fs::create_dir_all(&other_dir).unwrap();
        std::fs::write(profile_dir.join("passwords.bin"), b"protected vault").unwrap();
        std::fs::write(other_dir.join("passwords.bin"), b"other profile").unwrap();

        remove_profile_vault(&root, "profile-1").unwrap();
        remove_profile_vault(&root, "profile-1").unwrap();
        assert!(!profile_dir.join("passwords.bin").exists());
        assert_eq!(
            std::fs::read(other_dir.join("passwords.bin")).unwrap(),
            b"other profile"
        );

        assert!(remove_profile_vault(&root, "../other").is_err());
        assert!(other_dir.join("passwords.bin").exists());
        let _ = std::fs::remove_dir_all(root);
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
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, entries[0].id);
        assert_eq!(loaded[0].password, "first-secret");
        assert!(loaded[0].updated_at_ms >= entries[0].updated_at_ms);
        // Production edits always reload before the next update. Saving may
        // monotonically advance metadata without mutating the caller's slice.
        entries = loaded;

        upsert(
            &mut entries,
            "https://example.com",
            "account",
            "second-secret",
        )
        .unwrap();
        save(&path, &entries).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, entries[0].id);
        assert_eq!(loaded[0].password, "second-secret");
        assert!(loaded[0].updated_at_ms >= entries[0].updated_at_ms);
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn legacy_vault_migrates_and_save_tracks_deletion_then_readd() {
        let root = std::env::temp_dir().join(format!(
            "pip-password-migration-test-{}-{}",
            std::process::id(),
            OsRng.next_u64()
        ));
        let path = root.join("passwords.bin");
        let mut legacy = Vec::new();
        upsert(&mut legacy, "https://example.com", "account", "secret").unwrap();
        let legacy_plaintext = serde_json::to_vec(&legacy).unwrap();
        let legacy_protected = crate::sync::protect_local_data(&legacy_plaintext).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&path, legacy_protected).unwrap();

        let loaded = load(&path).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].password, "secret");
        save(&path, &loaded).unwrap();
        let encoded =
            crate::sync::unprotect_local_data(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let document: PasswordVaultDocument = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(document.version, PASSWORD_VAULT_VERSION);

        save(&path, &[]).unwrap();
        let deleted = sync_projection(&path).unwrap();
        assert!(deleted.entries.is_empty());
        assert_eq!(deleted.tombstones.len(), 1);
        let deletion_time = deleted.tombstones[0].deleted_at_ms;

        let mut readded = Vec::new();
        upsert(&mut readded, "https://example.com", "account", "new-secret").unwrap();
        save(&path, &readded).unwrap();
        let restored = sync_projection(&path).unwrap();
        assert_eq!(restored.entries.len(), 1);
        assert!(restored.entries[0].updated_at_ms > deletion_time);
        assert!(restored.tombstones.is_empty());
        let _ = std::fs::remove_dir_all(root);
    }
}
