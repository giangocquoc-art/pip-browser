//! Client-side authenticated encryption for the opaque Cloudflare sync vault.

use crate::sync_v2;
use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
#[cfg(test)]
use evergreen_core::library::Page;
use hkdf::Hkdf;
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

const AAD: &[u8] = b"pip-browser-sync-v1";
const SYNC_ENDPOINT: &str = "https://sync.psharehub.com/v1/vault";
const MAX_SYNC_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg(test)]
pub struct BookmarkSnapshot {
    pub version: u8,
    #[serde(default)]
    pub bookmarks: Vec<Page>,
    #[serde(default)]
    pub removed_bookmarks: Vec<String>,
}

#[derive(Debug)]
pub struct SyncOutcome {
    pub payload: sync_v2::SyncPayloadV2,
    /// Fingerprint of the local allowlisted settings sent when this request began.
    pub requested_settings_fingerprint: Option<String>,
    pub message: String,
}

/// Build the allowlisted sync payload from the local library. History and
/// start-page shortcuts are deliberately left out, even as those local schemas grow.
#[cfg(test)]
pub fn bookmark_snapshot_from_library(
    library: &evergreen_core::library::Library,
) -> BookmarkSnapshot {
    BookmarkSnapshot {
        version: 1,
        bookmarks: library.bookmarks.clone(),
        removed_bookmarks: library.removed_bookmarks.clone(),
    }
}

pub fn sync_payload_from_library(
    library: &evergreen_core::library::Library,
    include_history: bool,
) -> sync_v2::SyncPayloadV2 {
    let mut bookmark_updates = std::collections::HashMap::new();
    for update in &library.bookmark_updates {
        bookmark_updates
            .entry(update.url.as_str())
            .and_modify(|timestamp: &mut u64| {
                *timestamp = (*timestamp).max(update.updated_at_ms.max(0) as u64)
            })
            .or_insert(update.updated_at_ms.max(0) as u64);
    }
    let mut bookmark_tombstones = std::collections::HashMap::new();
    for url in &library.removed_bookmarks {
        bookmark_tombstones.entry(url.as_str()).or_insert(0u64);
    }
    for tombstone in &library.bookmark_tombstones {
        let timestamp = tombstone.deleted_at_ms.max(0) as u64;
        bookmark_tombstones
            .entry(tombstone.url.as_str())
            .and_modify(|existing: &mut u64| *existing = (*existing).max(timestamp))
            .or_insert(timestamp);
    }
    let mut bookmark_tombstones = bookmark_tombstones
        .into_iter()
        .map(|(url, deleted_at_ms)| sync_v2::UrlTombstone {
            url: url.to_owned(),
            deleted_at_ms,
        })
        .collect::<Vec<_>>();
    bookmark_tombstones.sort_by(|a, b| {
        b.deleted_at_ms
            .cmp(&a.deleted_at_ms)
            .then_with(|| a.url.cmp(&b.url))
    });
    bookmark_tombstones.truncate(sync_v2::MAX_BOOKMARK_TOMBSTONES);
    let mut payload = sync_v2::SyncPayloadV2 {
        schema_version: sync_v2::INNER_SCHEMA_VERSION,
        bookmarks: Some(sync_v2::BookmarkCategory {
            items: library
                .bookmarks
                .iter()
                .map(|page| sync_v2::TimedBookmark {
                    page: sync_v2::SyncPage {
                        url: page.url.clone(),
                        title: page.title.clone(),
                    },
                    updated_at_ms: bookmark_updates
                        .get(page.url.as_str())
                        .copied()
                        .unwrap_or(0),
                })
                .collect(),
            tombstones: bookmark_tombstones,
            folders: library
                .bookmark_folders
                .iter()
                .map(|folder| sync_v2::SyncBookmarkFolder {
                    id: folder.id.clone(),
                    name: folder.name.clone(),
                    parent_id: folder.parent_id.clone(),
                })
                .collect(),
            placements: library
                .bookmark_folder_assignments
                .iter()
                .map(|placement| sync_v2::BookmarkPlacement {
                    url: placement.url.clone(),
                    folder_id: placement.folder_id.clone(),
                    updated_at_ms: placement.updated_at_ms.max(0) as u64,
                })
                .collect(),
            folder_tombstones: library
                .bookmark_folder_tombstones
                .iter()
                .map(|folder| sync_v2::BookmarkFolderTombstone {
                    id: folder.id.clone(),
                    deleted_at_ms: folder.deleted_at_ms.max(0) as u64,
                })
                .collect(),
        }),
        history: None,
        open_tabs: None,
        settings: None,
        passwords: None,
    };
    if include_history {
        let mut history_items = library
            .history_visits
            .iter()
            .map(|visit| sync_v2::HistoryEntry {
                page: sync_v2::SyncPage {
                    url: visit.url.clone(),
                    title: visit.title.clone(),
                },
                visited_at_ms: visit.visited_at_ms.max(0) as u64,
            })
            .collect::<Vec<_>>();
        // Profiles created before timestamped history store only the latest
        // page per URL. Keep those URLs in Sync until load-time migration runs.
        for page in &library.history {
            if !history_items.iter().any(|visit| visit.page.url == page.url) {
                history_items.push(sync_v2::HistoryEntry {
                    page: sync_v2::SyncPage {
                        url: page.url.clone(),
                        title: page.title.clone(),
                    },
                    visited_at_ms: 0,
                });
            }
        }
        payload.history = Some(sync_v2::HistoryCategory {
            items: history_items,
            tombstones: library
                .removed_history
                .iter()
                .filter(|removed| removed.visit_at_ms.is_none())
                .map(|removed| sync_v2::UrlTombstone {
                    url: removed.url.clone(),
                    deleted_at_ms: removed.removed_at_ms.max(0) as u64,
                })
                .collect(),
            visit_tombstones: library
                .removed_history
                .iter()
                .filter_map(|removed| {
                    removed
                        .visit_at_ms
                        .map(|visited_at_ms| sync_v2::HistoryVisitTombstone {
                            url: removed.url.clone(),
                            visited_at_ms: visited_at_ms.max(0) as u64,
                            deleted_at_ms: removed.removed_at_ms.max(0) as u64,
                        })
                })
                .collect(),
        });
    }
    payload
}

pub fn generate_device_id() -> String {
    let mut id = [0u8; 16];
    OsRng.fill_bytes(&mut id);
    encode_hex(&id)
}

pub fn syncable_settings_from(
    settings: &evergreen_core::settings::Settings,
    updated_at_ms: u64,
) -> sync_v2::SyncableSettings {
    use sync_v2::{SearchEngine, SyncableSettings, Theme, UiDensity};
    SyncableSettings {
        updated_at_ms,
        search_engine: match settings.search_engine.as_str() {
            "bing" => SearchEngine::Bing,
            "brave" => SearchEngine::Brave,
            "ecosia" => SearchEngine::Ecosia,
            _ => SearchEngine::Google,
        },
        theme: match settings.appearance.theme.as_str() {
            "light" => Theme::Light,
            "dark" => Theme::Dark,
            _ => Theme::System,
        },
        density: match settings.appearance.density.as_str() {
            "compact" => UiDensity::Compact,
            _ => UiDensity::Comfortable,
        },
        default_zoom_percent: (settings.appearance.default_zoom_level * 100.0)
            .round()
            .clamp(50.0, 200.0) as u16,
        show_status_preview: settings.appearance.show_status_preview,
        show_zoom_badge: settings.appearance.show_zoom_badge,
        enable_tab_reordering: settings.tabs.enable_tab_reordering,
        enable_tab_tearoff: settings.tabs.enable_tab_tearoff,
        warn_on_close_tabs: settings.tabs.warn_on_close_tabs,
    }
}

pub fn apply_syncable_settings(
    settings: &mut evergreen_core::settings::Settings,
    synced: &sync_v2::SyncableSettings,
) {
    use sync_v2::{SearchEngine, Theme, UiDensity};
    settings.search_engine = match synced.search_engine {
        SearchEngine::Google => "google",
        SearchEngine::Bing => "bing",
        SearchEngine::Brave => "brave",
        SearchEngine::Ecosia => "ecosia",
    }
    .into();
    settings.appearance.theme = match synced.theme {
        Theme::System => "system",
        Theme::Light => "light",
        Theme::Dark => "dark",
    }
    .into();
    settings.appearance.density = match synced.density {
        UiDensity::Comfortable => "comfortable",
        UiDensity::Compact => "compact",
    }
    .into();
    settings.appearance.default_zoom_level = f64::from(synced.default_zoom_percent) / 100.0;
    settings.appearance.show_status_preview = synced.show_status_preview;
    settings.appearance.show_zoom_badge = synced.show_zoom_badge;
    settings.tabs.enable_tab_reordering = synced.enable_tab_reordering;
    settings.tabs.enable_tab_tearoff = synced.enable_tab_tearoff;
    settings.tabs.warn_on_close_tabs = synced.warn_on_close_tabs;
}

pub fn syncable_settings_fingerprint(settings: &sync_v2::SyncableSettings) -> String {
    let mut normalized = settings.clone();
    normalized.updated_at_ms = 0;
    serde_json::to_string(&normalized).unwrap_or_default()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncCredentials {
    pub vault_id: String,
    pub access_token: String,
    pub recovery_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EncryptedEnvelope {
    pub version: u8,
    pub salt: String,
    pub iv: String,
    pub ciphertext: String,
}

#[derive(Debug, thiserror::Error)]
pub enum SyncCryptoError {
    #[error("Mã khôi phục Pip Sync không hợp lệ")]
    InvalidRecoveryCode,
    #[error("Không thể tạo khóa mã hóa an toàn")]
    KeyDerivation,
    #[error("Gói đồng bộ không hợp lệ hoặc đã bị thay đổi")]
    InvalidEnvelope,
    #[error("Không thể mã hóa dữ liệu đồng bộ")]
    Encrypt,
    #[error("Không thể giải mã dữ liệu đồng bộ; kiểm tra mã khôi phục")]
    Decrypt,
}

impl SyncCredentials {
    pub fn generate() -> Self {
        let mut id = [0u8; 16];
        let mut access_token = [0u8; 32];
        let mut recovery_key = [0u8; 32];
        OsRng.fill_bytes(&mut id);
        OsRng.fill_bytes(&mut access_token);
        OsRng.fill_bytes(&mut recovery_key);
        Self {
            vault_id: encode_hex(&id),
            access_token: encode_hex(&access_token),
            recovery_key: encode_hex(&recovery_key),
        }
    }

    pub fn recovery_code(&self) -> String {
        format!(
            "PIPSYNC1:{}:{}:{}",
            self.vault_id, self.access_token, self.recovery_key
        )
    }

    pub fn from_recovery_code(value: &str) -> Result<Self, SyncCryptoError> {
        let parts: Vec<_> = value.trim().split(':').collect();
        if parts.len() != 4
            || parts[0] != "PIPSYNC1"
            || decode_hex(parts[1]).is_none_or(|bytes| bytes.len() != 16)
            || decode_hex(parts[2]).is_none_or(|bytes| bytes.len() != 32)
            || decode_hex(parts[3]).is_none_or(|bytes| bytes.len() != 32)
        {
            return Err(SyncCryptoError::InvalidRecoveryCode);
        }
        Ok(Self {
            // The Worker route and bearer-token validator use lowercase hex.
            // decode_hex accepts uppercase, so canonicalize imports before
            // those credentials reach the HTTP client.
            vault_id: parts[1].to_ascii_lowercase(),
            access_token: parts[2].to_ascii_lowercase(),
            recovery_key: parts[3].to_ascii_lowercase(),
        })
    }

    fn derived_key(&self, salt: &[u8]) -> Result<[u8; 32], SyncCryptoError> {
        let secret = decode_hex(&self.recovery_key).ok_or(SyncCryptoError::InvalidRecoveryCode)?;
        let hkdf = Hkdf::<Sha256>::new(Some(salt), &secret);
        let mut key = [0u8; 32];
        hkdf.expand(AAD, &mut key)
            .map_err(|_| SyncCryptoError::KeyDerivation)?;
        Ok(key)
    }
}

pub fn protect_credentials(credentials: &SyncCredentials) -> Result<String, SyncCryptoError> {
    let bytes =
        serde_json::to_vec(credentials).map_err(|_| SyncCryptoError::InvalidRecoveryCode)?;
    protect_local_secret(&bytes)
}

pub fn unprotect_credentials(value: &str) -> Result<SyncCredentials, SyncCryptoError> {
    let plaintext = unprotect_local_data(value)?;
    serde_json::from_slice(&plaintext).map_err(|_| SyncCryptoError::InvalidEnvelope)
}

pub(crate) fn protect_local_data(bytes: &[u8]) -> Result<String, SyncCryptoError> {
    protect_local_secret(bytes)
}

pub(crate) fn unprotect_local_data(value: &str) -> Result<Vec<u8>, SyncCryptoError> {
    let bytes = decode_hex(value).ok_or(SyncCryptoError::InvalidEnvelope)?;
    unprotect_local_secret(&bytes)
}

#[cfg(windows)]
fn protect_local_secret(bytes: &[u8]) -> Result<String, SyncCryptoError> {
    use windows::core::PCWSTR;
    use windows::Win32::{
        Foundation::{LocalFree, HLOCAL},
        Security::Cryptography::{CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB},
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes
            .len()
            .try_into()
            .map_err(|_| SyncCryptoError::InvalidEnvelope)?,
        pbData: bytes.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    // SAFETY: input points to a valid byte slice for the duration of the call;
    // Windows allocates output memory, which is released with LocalFree below.
    unsafe {
        CryptProtectData(
            &input,
            PCWSTR::null(),
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|_| SyncCryptoError::Encrypt)?;
        let protected = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(output.pbData.cast())));
        Ok(encode_hex(&protected))
    }
}

#[cfg(windows)]
fn unprotect_local_secret(bytes: &[u8]) -> Result<Vec<u8>, SyncCryptoError> {
    use windows::core::PWSTR;
    use windows::Win32::{
        Foundation::{LocalFree, HLOCAL},
        Security::Cryptography::{
            CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
        },
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes
            .len()
            .try_into()
            .map_err(|_| SyncCryptoError::InvalidEnvelope)?,
        pbData: bytes.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let mut description = PWSTR::null();
    // SAFETY: input points to a valid byte slice for the duration of the call;
    // Windows allocates output memory, which is released with LocalFree below.
    unsafe {
        CryptUnprotectData(
            &input,
            Some(&mut description),
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|_| SyncCryptoError::Decrypt)?;
        let plaintext = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(output.pbData.cast())));
        if !description.is_null() {
            let _ = LocalFree(Some(HLOCAL(description.0.cast())));
        }
        Ok(plaintext)
    }
}

#[cfg(not(windows))]
fn protect_local_secret(_bytes: &[u8]) -> Result<String, SyncCryptoError> {
    Err(SyncCryptoError::Encrypt)
}

#[cfg(not(windows))]
fn unprotect_local_secret(_bytes: &[u8]) -> Result<Vec<u8>, SyncCryptoError> {
    Err(SyncCryptoError::Decrypt)
}

pub fn encrypt(
    credentials: &SyncCredentials,
    plaintext: &[u8],
) -> Result<EncryptedEnvelope, SyncCryptoError> {
    let mut salt = [0u8; 16];
    let mut iv = [0u8; 12];
    OsRng.fill_bytes(&mut salt);
    OsRng.fill_bytes(&mut iv);
    let key = credentials.derived_key(&salt)?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| SyncCryptoError::KeyDerivation)?;
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&iv),
            Payload {
                msg: plaintext,
                aad: AAD,
            },
        )
        .map_err(|_| SyncCryptoError::Encrypt)?;
    Ok(EncryptedEnvelope {
        version: 1,
        salt: encode_hex(&salt),
        iv: encode_hex(&iv),
        ciphertext: encode_hex(&ciphertext),
    })
}

pub fn decrypt(
    credentials: &SyncCredentials,
    envelope: &EncryptedEnvelope,
) -> Result<Vec<u8>, SyncCryptoError> {
    if envelope.version != 1 {
        return Err(SyncCryptoError::InvalidEnvelope);
    }
    let salt = decode_hex(&envelope.salt).ok_or(SyncCryptoError::InvalidEnvelope)?;
    let iv = decode_hex(&envelope.iv).ok_or(SyncCryptoError::InvalidEnvelope)?;
    let ciphertext = decode_hex(&envelope.ciphertext).ok_or(SyncCryptoError::InvalidEnvelope)?;
    if salt.len() != 16 || iv.len() != 12 || ciphertext.len() < 16 {
        return Err(SyncCryptoError::InvalidEnvelope);
    }
    let key = credentials.derived_key(&salt)?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| SyncCryptoError::KeyDerivation)?;
    cipher
        .decrypt(
            Nonce::from_slice(&iv),
            Payload {
                msg: &ciphertext,
                aad: AAD,
            },
        )
        .map_err(|_| SyncCryptoError::Decrypt)
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[allow(clippy::chunks_exact_to_as_chunks)]
fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if !value.len().is_multiple_of(2) {
        return None;
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let digit = |byte: u8| match byte {
                b'0'..=b'9' => Some(byte - b'0'),
                b'a'..=b'f' => Some(byte - b'a' + 10),
                b'A'..=b'F' => Some(byte - b'A' + 10),
                _ => None,
            };
            Some((digit(pair[0])? << 4) | digit(pair[1])?)
        })
        .collect()
}

/// Synchronizes bookmarks using a revision check. The worker sees only the encrypted envelope.
#[cfg(all(windows, test))]
pub fn sync_bookmarks(
    credentials: &SyncCredentials,
    local: BookmarkSnapshot,
    proxy_port: Option<u16>,
) -> Result<SyncOutcome, String> {
    let local = normalize_snapshot(local)?;
    let payload = sync_v2::SyncPayloadV2 {
        schema_version: sync_v2::INNER_SCHEMA_VERSION,
        bookmarks: Some(sync_v2::BookmarkCategory {
            items: local
                .bookmarks
                .into_iter()
                .map(|page| sync_v2::TimedBookmark {
                    page: sync_v2::SyncPage {
                        url: page.url,
                        title: page.title,
                    },
                    updated_at_ms: 0,
                })
                .collect(),
            tombstones: local
                .removed_bookmarks
                .into_iter()
                .map(|url| sync_v2::UrlTombstone {
                    url,
                    deleted_at_ms: 0,
                })
                .collect(),
            folders: Vec::new(),
            placements: Vec::new(),
            folder_tombstones: Vec::new(),
        }),
        history: None,
        open_tabs: None,
        settings: None,
        passwords: None,
    };
    sync_payload(credentials, payload, proxy_port)
}

/// Synchronizes a bounded V4 inner payload while preserving the opaque V1 HTTP envelope.
#[cfg(windows)]
pub fn sync_payload(
    credentials: &SyncCredentials,
    local: sync_v2::SyncPayloadV2,
    proxy_port: Option<u16>,
) -> Result<SyncOutcome, String> {
    sync_payload_with_google_dns(credentials, local, false, proxy_port)
}

#[cfg(windows)]
pub fn sync_payload_with_google_dns(
    credentials: &SyncCredentials,
    local: sync_v2::SyncPayloadV2,
    google_dns_enabled: bool,
    proxy_port: Option<u16>,
) -> Result<SyncOutcome, String> {
    let builder = crate::doh_proxy::with_google_dns_proxy(
        reqwest::blocking::Client::builder(),
        google_dns_enabled,
        proxy_port,
    )?;
    let client = builder
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|_| "Không tạo được kết nối Pip Sync.".to_string())?;
    let url = format!("{SYNC_ENDPOINT}/{}", credentials.vault_id);
    sync_payload_with_client(credentials, local, &client, &url)
}

#[cfg(any(windows, test))]
fn sync_payload_with_client(
    credentials: &SyncCredentials,
    local: sync_v2::SyncPayloadV2,
    client: &reqwest::blocking::Client,
    url: &str,
) -> Result<SyncOutcome, String> {
    let token = format!("Bearer {}", credentials.access_token);
    let requested_settings_fingerprint = local.settings.as_ref().map(syncable_settings_fingerprint);
    let mut local = local
        .validate_and_normalize()
        .map_err(|error| format!("Dữ liệu Sync trên thiết bị không hợp lệ: {error}"))?;

    for attempt in 0..2 {
        let response = client
            .get(url)
            .header(reqwest::header::AUTHORIZATION, &token)
            .send()
            .map_err(|_| "Không kết nối được Pip Sync. Kiểm tra mạng rồi thử lại.".to_string())?;
        let (remote, revision) = match response.status().as_u16() {
            200 => {
                let revision = response
                    .headers()
                    .get(reqwest::header::ETAG)
                    .and_then(|value| value.to_str().ok())
                    .ok_or_else(|| "Máy chủ thiếu mã phiên bản đồng bộ.".to_string())?
                    .to_string();
                let bytes = read_limited(response, MAX_SYNC_BYTES).map_err(|_| {
                    "Không đọc được dữ liệu đồng bộ hoặc vượt giới hạn 1 MB.".to_string()
                })?;
                if bytes.len() > MAX_SYNC_BYTES {
                    return Err("Dữ liệu đồng bộ vượt giới hạn 1 MB.".into());
                }
                let envelope: EncryptedEnvelope = serde_json::from_slice(&bytes)
                    .map_err(|_| "Gói đồng bộ trên máy chủ không hợp lệ.".to_string())?;
                let plain = decrypt(credentials, &envelope).map_err(|error| error.to_string())?;
                if plain.len() > MAX_SYNC_BYTES {
                    return Err("Dữ liệu Pip Sync vượt giới hạn 1 MB.".into());
                }
                let snapshot = sync_v2::SyncPayloadV2::decode_and_migrate(&plain).map_err(
                    |error| match error {
                        sync_v2::SyncPayloadError::UnsupportedVersion(version) => format!(
                            "Kho Pip Sync dùng phiên bản dữ liệu {version} không tương thích. Hãy cập nhật Pip trên thiết bị này rồi thử lại."
                        ),
                        sync_v2::SyncPayloadError::PayloadTooLarge => {
                            "Dữ liệu Pip Sync vượt giới hạn an toàn.".to_string()
                        }
                        _ => "Dữ liệu Pip Sync không hợp lệ hoặc chưa được hỗ trợ.".to_string(),
                    },
                )?;
                (Some(snapshot), Some(revision))
            }
            404 => (None, None),
            401 => return Err("Mã đồng bộ không khớp hoặc đã bị thu hồi.".into()),
            410 => {
                return Err(
                    "Kho đồng bộ đã bị xóa. Tạo mã mới hoặc nhập mã khác để tiếp tục.".into(),
                );
            }
            _ => return Err("Máy chủ Pip Sync chưa sẵn sàng. Hãy thử lại sau.".into()),
        };

        if let Some(remote) = remote {
            local = local
                .merge(remote)
                .map_err(|_| "Không thể gộp dữ liệu Pip Sync an toàn.".to_string())?;
        }
        let plain = local
            .encode()
            .map_err(|_| "Dữ liệu Pip Sync vượt giới hạn an toàn hoặc không hợp lệ.".to_string())?;
        let envelope = encrypt(credentials, &plain).map_err(|error| error.to_string())?;
        let payload = serde_json::to_vec(&envelope)
            .map_err(|_| "Không thể mã hóa dữ liệu Pip Sync.".to_string())?;
        if payload.len() > MAX_SYNC_BYTES {
            return Err("Gói Pip Sync vượt giới hạn 1 MB.".into());
        }
        let mut request = client
            .put(url)
            .header(reqwest::header::AUTHORIZATION, &token)
            .header(reqwest::header::CONTENT_TYPE, "application/json");
        request = if let Some(revision) = revision {
            request.header(reqwest::header::IF_MATCH, revision)
        } else {
            request.header(reqwest::header::IF_NONE_MATCH, "*")
        };
        let result = request
            .body(payload)
            .send()
            .map_err(|_| "Không tải lên được dữ liệu đồng bộ.".to_string())?;
        if result.status().as_u16() == 204 {
            return Ok(SyncOutcome {
                payload: local,
                requested_settings_fingerprint,
                message: "Đã đồng bộ dữ liệu Pip Sync an toàn.".into(),
            });
        }
        if result.status().as_u16() == 412 && attempt == 0 {
            continue;
        }
        if result.status().as_u16() == 412 {
            return Err("Một thiết bị khác vừa cập nhật. Hãy bấm đồng bộ lại.".into());
        }
        return Err("Máy chủ từ chối dữ liệu đồng bộ.".into());
    }
    Err("Không thể đồng bộ sau khi dữ liệu thay đổi trên thiết bị khác.".into())
}

#[cfg(windows)]
pub fn delete_vault(credentials: &SyncCredentials, proxy_port: Option<u16>) -> Result<(), String> {
    delete_vault_with_google_dns(credentials, false, proxy_port)
}

#[cfg(windows)]
pub fn delete_vault_with_google_dns(
    credentials: &SyncCredentials,
    google_dns_enabled: bool,
    proxy_port: Option<u16>,
) -> Result<(), String> {
    let builder = crate::doh_proxy::with_google_dns_proxy(
        reqwest::blocking::Client::builder(),
        google_dns_enabled,
        proxy_port,
    )?;
    let client = builder
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|_| "Không tạo được kết nối Pip Sync.".to_string())?;
    let url = format!("{SYNC_ENDPOINT}/{}", credentials.vault_id);
    delete_vault_with_client(&client, credentials, &url)
}

#[cfg(any(windows, test))]
fn delete_vault_with_client(
    client: &reqwest::blocking::Client,
    credentials: &SyncCredentials,
    url: &str,
) -> Result<(), String> {
    let authorization = format!("Bearer {}", credentials.access_token);
    let current = client
        .get(url)
        .header(reqwest::header::AUTHORIZATION, &authorization)
        .send()
        .map_err(|_| "Không kết nối được Pip Sync để xóa kho.".to_string())?;
    if current.status().as_u16() == 410 {
        return Ok(());
    }
    if current.status().as_u16() == 401 {
        return Err("Mã Pip Sync không khớp; không thể xác nhận quyền xóa.".into());
    }
    let revision = if current.status().as_u16() == 200 {
        Some(
            current
                .headers()
                .get(reqwest::header::ETAG)
                .ok_or_else(|| "Máy chủ thiếu phiên bản kho đồng bộ.".to_string())?
                .to_str()
                .map_err(|_| "Phiên bản kho đồng bộ không hợp lệ.".to_string())?
                .to_string(),
        )
    } else if current.status().as_u16() == 404 {
        // Revoke even a currently absent vault. Another device may be racing
        // its first If-None-Match create; the Worker's tombstone makes delete
        // win regardless of transaction order.
        None
    } else {
        return Err("Máy chủ không xác nhận được kho đồng bộ.".into());
    };
    let mut request = client
        .delete(url)
        .header(reqwest::header::AUTHORIZATION, authorization);
    if let Some(revision) = revision {
        request = request.header(reqwest::header::IF_MATCH, revision);
    }
    let deleted = request
        .send()
        .map_err(|_| "Không xóa được kho đồng bộ trên máy chủ.".to_string())?;
    if matches!(deleted.status().as_u16(), 204 | 410) {
        Ok(())
    } else {
        Err("Kho đồng bộ đã thay đổi. Hãy thử tắt đồng bộ lại.".into())
    }
}

#[cfg(test)]
fn vault_is_deleted_status(status: u16) -> bool {
    matches!(status, 404 | 410)
}

fn read_limited(mut reader: impl std::io::Read, max_bytes: usize) -> std::io::Result<Vec<u8>> {
    use std::io::Read as _;

    let limit = max_bytes.saturating_add(1);
    let mut bytes = Vec::with_capacity(limit.min(8192));
    reader.by_ref().take(limit as u64).read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
fn normalize_snapshot(mut snapshot: BookmarkSnapshot) -> Result<BookmarkSnapshot, String> {
    if snapshot.version != 1 {
        return Err(format!(
            "Kho Pip Sync dùng phiên bản dữ liệu {} không tương thích. Hãy cập nhật Pip trên thiết bị này rồi thử lại.",
            snapshot.version
        ));
    }
    if snapshot.bookmarks.len() > 200 || snapshot.removed_bookmarks.len() > 2000 {
        return Err("Dữ liệu dấu trang không hợp lệ hoặc vượt giới hạn.".into());
    }
    snapshot.removed_bookmarks.retain(|url| {
        url.len() <= 8192 && (url.starts_with("https://") || url.starts_with("http://"))
    });
    snapshot.bookmarks.retain(|page| {
        page.url.len() <= 8192
            && (page.url.starts_with("https://") || page.url.starts_with("http://"))
            && page.title.chars().count() <= 300
    });
    let mut seen = std::collections::HashSet::new();
    snapshot
        .bookmarks
        .retain(|page| seen.insert(page.url.clone()));
    snapshot
        .bookmarks
        .retain(|page| !snapshot.removed_bookmarks.contains(&page.url));
    Ok(snapshot)
}

#[cfg(test)]
pub fn merge_snapshots(local: BookmarkSnapshot, remote: BookmarkSnapshot) -> BookmarkSnapshot {
    let mut removed = local.removed_bookmarks;
    for url in remote.removed_bookmarks {
        if !removed.contains(&url) {
            removed.push(url);
        }
    }
    if removed.len() > 2000 {
        removed.drain(..removed.len() - 2000);
    }
    let mut bookmarks = local.bookmarks;
    for page in remote.bookmarks {
        if !removed.contains(&page.url)
            && !bookmarks.iter().any(|existing| existing.url == page.url)
        {
            bookmarks.push(page);
        }
    }
    bookmarks.retain(|page| !removed.contains(&page.url));
    bookmarks.truncate(200);
    BookmarkSnapshot {
        version: 1,
        bookmarks,
        removed_bookmarks: removed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_v4_settings_snapshot_reads_the_selected_profiles_override() {
        let mut settings = evergreen_core::settings::Settings::default();
        let mut work = evergreen_core::settings::ProfileSyncSettings::from_global(&settings);
        work.search_engine = "brave".into();
        work.default_zoom_level = 1.25;
        work.show_status_preview = false;
        settings
            .profiles
            .push(evergreen_core::settings::BrowserProfile {
                id: "Work".into(),
                name: "Work".into(),
                sync_settings_values: Some(work),
                ..settings.profiles[0].clone()
            });

        let personal = syncable_settings_from(&settings.for_profile("Default"), 10);
        let work = syncable_settings_from(&settings.for_profile("Work"), 10);
        assert_eq!(personal.search_engine, sync_v2::SearchEngine::Google);
        assert_eq!(work.search_engine, sync_v2::SearchEngine::Brave);
        assert_eq!(personal.default_zoom_percent, 100);
        assert_eq!(work.default_zoom_percent, 125);
        assert!(personal.show_status_preview);
        assert!(!work.show_status_preview);
    }

    #[test]
    fn sync_response_reader_stops_after_limit_plus_one() {
        let bytes = vec![b'x'; MAX_SYNC_BYTES + 4096];
        let read = read_limited(bytes.as_slice(), MAX_SYNC_BYTES).unwrap();
        assert_eq!(read.len(), MAX_SYNC_BYTES + 1);
    }

    #[test]
    fn vault_deletion_statuses_are_idempotent() {
        assert!(vault_is_deleted_status(404));
        assert!(vault_is_deleted_status(410));
        assert!(!vault_is_deleted_status(401));
        assert!(!vault_is_deleted_status(412));
        assert!(!vault_is_deleted_status(500));
    }

    #[test]
    fn delete_of_missing_vault_sends_authenticated_tombstone_request() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for status in ["404 Not Found", "204 No Content"] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut bytes = Vec::new();
                let mut chunk = [0_u8; 1024];
                while !bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                    let read = stream.read(&mut chunk).unwrap();
                    assert!(read > 0, "request ended before headers completed");
                    bytes.extend_from_slice(&chunk[..read]);
                }
                requests.push(String::from_utf8(bytes).unwrap());
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )
                .unwrap();
            }
            requests
        });

        let credentials = SyncCredentials::generate();
        let access_token = credentials.access_token.clone();
        let url = format!("http://{address}/v1/vault/{}", credentials.vault_id);
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(3))
            .build()
            .unwrap();

        delete_vault_with_client(&client, &credentials, &url).unwrap();

        let requests = server.join().unwrap();
        assert!(requests[0].starts_with("GET /v1/vault/"));
        assert!(requests[1].starts_with("DELETE /v1/vault/"));
        let delete_request = requests[1].to_ascii_lowercase();
        assert!(delete_request.contains(&format!("authorization: bearer {access_token}")));
        assert!(!delete_request.contains("if-match:"));
    }

    #[cfg(windows)]
    #[test]
    fn local_http_sync_roundtrip_merges_two_device_clients_without_plaintext_leakage() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
        };

        fn payload(marker: &str) -> sync_v2::SyncPayloadV2 {
            sync_v2::SyncPayloadV2 {
                schema_version: sync_v2::INNER_SCHEMA_VERSION,
                bookmarks: Some(sync_v2::BookmarkCategory {
                    items: vec![sync_v2::TimedBookmark {
                        page: sync_v2::SyncPage {
                            url: format!("https://{marker}.sync-test.invalid/"),
                            title: marker.into(),
                        },
                        updated_at_ms: 1_700_000_000_000,
                    }],
                    tombstones: Vec::new(),
                    folders: Vec::new(),
                    placements: Vec::new(),
                    folder_tombstones: Vec::new(),
                }),
                history: None,
                open_tabs: None,
                settings: None,
                passwords: None,
            }
        }

        let device_a = SyncCredentials::generate();
        let device_b = SyncCredentials::from_recovery_code(&device_a.recovery_code()).unwrap();
        let expected_auth = format!("Bearer {}", device_a.access_token);
        let expected_path = format!("/v1/vault/{}", device_a.vault_id);
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let mut envelope: Option<Vec<u8>> = None;
            let mut etag: Option<String> = None;
            let mut stored_bodies = Vec::new();
            for _ in 0..6 {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0_u8; 1024];
                let (header_end, content_length) = loop {
                    let read = stream.read(&mut buffer).unwrap();
                    assert!(read > 0, "client ended the sync request early");
                    request.extend_from_slice(&buffer[..read]);
                    if let Some(index) = request.windows(4).position(|window| window == b"\r\n\r\n")
                    {
                        let header_end = index + 4;
                        let headers =
                            String::from_utf8_lossy(&request[..index]).to_ascii_lowercase();
                        let length = headers
                            .lines()
                            .find_map(|line| line.strip_prefix("content-length:"))
                            .map(str::trim)
                            .and_then(|value| value.parse::<usize>().ok())
                            .unwrap_or(0);
                        if request.len() >= header_end + length {
                            break (header_end, length);
                        }
                        // Continue reading after the header terminator until the full envelope arrives.
                        while request.len() < header_end + length {
                            let read = stream.read(&mut buffer).unwrap();
                            assert!(read > 0, "client ended the sync body early");
                            request.extend_from_slice(&buffer[..read]);
                        }
                        break (header_end, length);
                    }
                };
                let head = String::from_utf8_lossy(&request[..header_end - 4]);
                let mut lines = head.lines();
                let method = lines.next().unwrap().split_whitespace().next().unwrap();
                let headers = lines
                    .filter_map(|line| line.split_once(':'))
                    .map(|(name, value)| (name.to_ascii_lowercase(), value.trim().to_owned()))
                    .collect::<std::collections::HashMap<_, _>>();
                assert_eq!(headers.get("authorization"), Some(&expected_auth));
                assert!(head.contains(&expected_path));
                let body = request[header_end..header_end + content_length].to_vec();
                let (status, etag_header, response_body) = match method {
                    "GET" => match &envelope {
                        Some(value) => ("200 OK", etag.clone(), value.clone()),
                        None => ("404 Not Found", None, Vec::new()),
                    },
                    "PUT" => {
                        stored_bodies.push(body.clone());
                        let condition = headers.get("if-none-match").map(String::as_str)
                            == Some("*")
                            && envelope.is_none()
                            || etag.as_ref().is_some_and(|current| {
                                headers
                                    .get("if-match")
                                    .is_some_and(|value| value == current)
                            });
                        if condition {
                            use sha2::Digest;
                            let hash = sha2::Sha256::digest(&body);
                            etag = Some(format!("\"{}\"", encode_hex(&hash)));
                            envelope = Some(body);
                            ("204 No Content", etag.clone(), Vec::new())
                        } else {
                            ("412 Precondition Failed", etag.clone(), Vec::new())
                        }
                    }
                    _ => panic!("unexpected local sync method: {method}"),
                };
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\n{}Content-Length: {}\r\nConnection: close\r\n\r\n",
                    etag_header
                        .map(|value| format!("ETag: {value}\r\n"))
                        .unwrap_or_default(),
                    response_body.len(),
                )
                .unwrap();
                stream.write_all(&response_body).unwrap();
            }
            stored_bodies
        });

        let url = format!("http://{address}/v1/vault/{}", device_a.vault_id);
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(3))
            .build()
            .unwrap();

        let first =
            sync_payload_with_client(&device_a, payload("client-a"), &client, &url).unwrap();
        assert_eq!(first.payload.bookmarks.as_ref().unwrap().items.len(), 1);
        let second =
            sync_payload_with_client(&device_b, payload("client-b"), &client, &url).unwrap();
        let second_bookmarks = &second.payload.bookmarks.as_ref().unwrap().items;
        assert!(second_bookmarks
            .iter()
            .any(|item| item.page.title == "client-a"));
        assert!(second_bookmarks
            .iter()
            .any(|item| item.page.title == "client-b"));
        let third =
            sync_payload_with_client(&device_a, payload("client-a"), &client, &url).unwrap();
        assert_eq!(third.payload.bookmarks.as_ref().unwrap().items.len(), 2);

        let stored_bodies = server.join().unwrap();
        assert_eq!(stored_bodies.len(), 3);
        assert!(stored_bodies.iter().all(|body| {
            !body
                .windows(b"client-a".len())
                .any(|window| window == b"client-a")
                && !body
                    .windows(b"client-b".len())
                    .any(|window| window == b"client-b")
        }));
    }

    #[test]
    fn recovery_code_roundtrips_and_secrets_are_high_entropy() {
        let credentials = SyncCredentials::generate();
        let restored = SyncCredentials::from_recovery_code(&credentials.recovery_code()).unwrap();
        assert_eq!(credentials, restored);
        assert_ne!(credentials.access_token, credentials.recovery_key);
        assert_eq!(credentials.vault_id.len(), 32);
    }

    #[test]
    fn recovery_code_normalizes_uppercase_hex_for_worker_contract() {
        let credentials = SyncCredentials::generate();
        let upper = credentials.recovery_code().to_ascii_uppercase();
        let restored = SyncCredentials::from_recovery_code(&upper).unwrap();
        assert_eq!(restored, credentials);
        assert!(restored
            .vault_id
            .bytes()
            .all(|byte| !byte.is_ascii_uppercase()));
        assert!(restored
            .access_token
            .bytes()
            .all(|byte| !byte.is_ascii_uppercase()));
        assert!(restored
            .recovery_key
            .bytes()
            .all(|byte| !byte.is_ascii_uppercase()));
    }

    #[test]
    fn encrypted_bookmark_snapshot_roundtrips_and_rejects_tampering() {
        let credentials = SyncCredentials::generate();
        let data =
            br#"{"version":1,"bookmarks":[{"url":"https://example.com","title":"Example"}]}"#;
        let mut envelope = encrypt(&credentials, data).unwrap();
        assert_eq!(decrypt(&credentials, &envelope).unwrap(), data);
        envelope.ciphertext.replace_range(0..2, "00");
        assert!(matches!(
            decrypt(&credentials, &envelope),
            Err(SyncCryptoError::Decrypt)
        ));
        let other = SyncCredentials::generate();
        assert!(decrypt(&other, &envelope).is_err());
    }

    #[test]
    fn unsupported_sync_schema_fails_with_update_guidance() {
        let error = normalize_snapshot(BookmarkSnapshot {
            version: 2,
            bookmarks: vec![],
            removed_bookmarks: vec![],
        })
        .unwrap_err();
        assert!(error.contains("phiên bản dữ liệu 2"));
        assert!(error.contains("cập nhật Pip"));
    }

    #[test]
    fn recovery_code_validation_rejects_short_and_malformed_values() {
        assert!(matches!(
            SyncCredentials::from_recovery_code("PIPSYNC1:a:b:c"),
            Err(SyncCryptoError::InvalidRecoveryCode)
        ));
    }

    #[test]
    fn deleted_bookmarks_win_during_cross_device_merge() {
        let local = BookmarkSnapshot {
            version: 1,
            bookmarks: vec![Page {
                url: "https://keep.example".into(),
                title: "Keep".into(),
            }],
            removed_bookmarks: vec!["https://gone.example".into()],
        };
        let remote = BookmarkSnapshot {
            version: 1,
            bookmarks: vec![
                Page {
                    url: "https://keep.example".into(),
                    title: "Old title".into(),
                },
                Page {
                    url: "https://gone.example".into(),
                    title: "Deleted elsewhere".into(),
                },
                Page {
                    url: "https://new.example".into(),
                    title: "New".into(),
                },
            ],
            removed_bookmarks: vec![],
        };
        let merged = merge_snapshots(local, remote);
        assert_eq!(
            merged
                .bookmarks
                .iter()
                .map(|page| page.url.as_str())
                .collect::<Vec<_>>(),
            vec!["https://keep.example", "https://new.example"]
        );
        assert_eq!(merged.removed_bookmarks, vec!["https://gone.example"]);
    }

    #[test]
    fn library_sync_projection_contains_only_bookmarks_and_deletions() {
        let library = evergreen_core::library::Library {
            bookmarks: vec![Page {
                url: "https://saved.example/".into(),
                title: "Saved".into(),
            }],
            bookmark_updates: vec![],
            bookmark_tombstones: vec![],
            bookmark_folders: vec![],
            bookmark_folder_assignments: vec![],
            bookmark_folder_tombstones: vec![],
            history: vec![Page {
                url: "https://private.example/?access_token=history-secret".into(),
                title: "History secret".into(),
            }],
            history_visits: vec![],
            removed_history: vec![],
            removed_bookmarks: vec!["https://removed.example/".into()],
            home_shortcuts: vec![Page {
                url: "https://private.example/?access_token=shortcut-secret".into(),
                title: "Shortcut secret".into(),
            }],
            home_shortcuts_configured: true,
        };

        let payload = serde_json::to_value(bookmark_snapshot_from_library(&library)).unwrap();
        let keys = payload
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            keys,
            ["bookmarks", "removed_bookmarks", "version"]
                .into_iter()
                .collect()
        );
        let serialized = payload.to_string();
        assert!(serialized.contains("saved.example"));
        assert!(serialized.contains("removed.example"));
        assert!(!serialized.contains("history-secret"));
        assert!(!serialized.contains("shortcut-secret"));
    }

    #[test]
    fn history_projection_is_opt_in_and_includes_visit_and_delete_metadata() {
        let library = evergreen_core::library::Library {
            history: vec![Page {
                url: "https://visited.example/?query=private".into(),
                title: "Visited".into(),
            }],
            history_visits: vec![
                evergreen_core::library::HistoryVisit {
                    url: "https://visited.example/?query=private".into(),
                    title: "Visited again".into(),
                    visited_at_ms: 1_700_000_000_124,
                },
                evergreen_core::library::HistoryVisit {
                    url: "https://visited.example/?query=private".into(),
                    title: "Visited".into(),
                    visited_at_ms: 1_700_000_000_123,
                },
            ],
            removed_history: vec![
                evergreen_core::library::HistoryTombstone {
                    url: "https://removed.example/".into(),
                    removed_at_ms: 1_700_000_001_456,
                    visit_at_ms: None,
                },
                evergreen_core::library::HistoryTombstone {
                    url: "https://visited.example/?query=private".into(),
                    removed_at_ms: 1_700_000_001_789,
                    visit_at_ms: Some(1_700_000_000_123),
                },
            ],
            home_shortcuts: vec![Page {
                url: "https://shortcut.example/".into(),
                title: "Shortcut".into(),
            }],
            ..Default::default()
        };
        assert!(sync_payload_from_library(&library, false).history.is_none());
        let payload = sync_payload_from_library(&library, true);
        let history = payload.history.unwrap();
        assert_eq!(history.items.len(), 2);
        assert_eq!(history.items[0].visited_at_ms, 1_700_000_000_124);
        assert_eq!(history.items[0].page.title, "Visited again");
        assert_eq!(history.tombstones[0].deleted_at_ms, 1_700_000_001_456);
        assert_eq!(history.visit_tombstones.len(), 1);
        assert_eq!(history.visit_tombstones[0].visited_at_ms, 1_700_000_000_123);
        assert!(payload.bookmarks.as_ref().unwrap().items.is_empty());
    }

    #[test]
    fn history_projection_accepts_more_than_one_thousand_tombstones() {
        let library = evergreen_core::library::Library {
            removed_history: (0..1_500)
                .map(|index| evergreen_core::library::HistoryTombstone {
                    url: format!("https://removed-{index}.example/"),
                    removed_at_ms: 1_700_000_000_000 + index,
                    visit_at_ms: None,
                })
                .collect(),
            ..Default::default()
        };
        let payload = sync_payload_from_library(&library, true);
        assert_eq!(payload.history.as_ref().unwrap().tombstones.len(), 1_500);
        assert!(payload.encode().is_ok());
    }

    #[test]
    fn bookmark_projection_uses_persisted_update_and_deletion_timestamps() {
        let library = evergreen_core::library::Library {
            bookmarks: vec![Page {
                url: "https://saved.example/".into(),
                title: "Saved".into(),
            }],
            bookmark_updates: vec![evergreen_core::library::BookmarkUpdate {
                url: "https://saved.example/".into(),
                updated_at_ms: 1_700_000_000_456,
            }],
            bookmark_tombstones: vec![evergreen_core::library::BookmarkTombstone {
                url: "https://deleted.example/".into(),
                deleted_at_ms: 1_700_000_001_789,
            }],
            ..Default::default()
        };

        let bookmarks = sync_payload_from_library(&library, false)
            .bookmarks
            .unwrap();
        assert_eq!(bookmarks.items[0].updated_at_ms, 1_700_000_000_456);
        assert_eq!(bookmarks.tombstones.len(), 1);
        assert_eq!(bookmarks.tombstones[0].url, "https://deleted.example/");
        assert_eq!(bookmarks.tombstones[0].deleted_at_ms, 1_700_000_001_789);
    }

    #[cfg(windows)]
    struct LiveVaultCleanup {
        credentials: SyncCredentials,
        revoked: bool,
    }

    #[cfg(windows)]
    impl LiveVaultCleanup {
        fn revoke(&mut self) -> Result<(), String> {
            delete_vault(&self.credentials, None)?;
            self.revoked = true;
            Ok(())
        }
    }

    #[cfg(windows)]
    impl Drop for LiveVaultCleanup {
        fn drop(&mut self) {
            if !self.revoked {
                if let Err(error) = delete_vault(&self.credentials, None) {
                    eprintln!("Could not revoke temporary Pip Sync test vault: {error}");
                }
            }
        }
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "writes a generated test vault to the live Cloudflare Pip Sync endpoint"]
    fn live_two_device_v4_merge_preserves_categories_and_rejects_stale_write() {
        let original = SyncCredentials::generate();
        let mut cleanup = LiveVaultCleanup {
            credentials: original.clone(),
            revoked: false,
        };
        let recovery_code = original.recovery_code();
        let device_a = SyncCredentials::from_recovery_code(&recovery_code).unwrap();
        let device_b = SyncCredentials::from_recovery_code(&recovery_code).unwrap();
        assert_eq!(device_a, original);
        assert_eq!(device_b, original);

        let payload_for = |device_id: &str, marker: &str| {
            let mut payload = sync_v2::SyncPayloadV2::empty();
            payload.bookmarks = Some(sync_v2::BookmarkCategory {
                items: vec![sync_v2::TimedBookmark {
                    page: sync_v2::SyncPage {
                        url: format!("https://{marker}.sync-test.invalid/bookmark"),
                        title: format!("{marker} bookmark"),
                    },
                    updated_at_ms: 1_700_000_000_000,
                }],
                tombstones: vec![],
                folders: vec![],
                placements: vec![],
                folder_tombstones: vec![],
            });
            payload.history = Some(sync_v2::HistoryCategory {
                items: vec![sync_v2::HistoryEntry {
                    page: sync_v2::SyncPage {
                        url: format!("https://{marker}.sync-test.invalid/history"),
                        title: format!("{marker} history"),
                    },
                    visited_at_ms: 1_700_000_000_100,
                }],
                tombstones: vec![],
                visit_tombstones: vec![],
            });
            payload.open_tabs = Some(sync_v2::OpenTabsCategory {
                devices: vec![sync_v2::DeviceTabs {
                    device_id: device_id.into(),
                    updated_at_ms: 1_700_000_000_200,
                    tabs: vec![sync_v2::OpenTab {
                        url: format!("https://{marker}.sync-test.invalid/tab"),
                        title: format!("{marker} tab"),
                        pinned: false,
                        group: None,
                    }],
                }],
            });
            payload
        };
        let payload_a = payload_for("device_a", "device-a");
        let payload_b = payload_for("device_b", "device-b");

        let first = sync_payload(&device_a, payload_a.clone(), None).unwrap();
        assert_eq!(first.payload.bookmarks.as_ref().unwrap().items.len(), 1);
        let endpoint = format!("{SYNC_ENDPOINT}/{}", device_a.vault_id);
        let token = format!("Bearer {}", device_a.access_token);
        let http = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .unwrap();
        let before_second_device = http
            .get(&endpoint)
            .header(reqwest::header::AUTHORIZATION, &token)
            .send()
            .unwrap();
        assert_eq!(before_second_device.status().as_u16(), 200);
        let stale_revision = before_second_device
            .headers()
            .get(reqwest::header::ETAG)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();

        let second = sync_payload(&device_b, payload_b, None).unwrap();
        let second_bookmarks = &second.payload.bookmarks.as_ref().unwrap().items;
        assert!(second_bookmarks
            .iter()
            .any(|entry| entry.page.url.contains("device-a")));
        assert!(second_bookmarks
            .iter()
            .any(|entry| entry.page.url.contains("device-b")));
        let second_history = &second.payload.history.as_ref().unwrap().items;
        assert!(second_history
            .iter()
            .any(|entry| entry.page.url.contains("device-a")));
        assert!(second_history
            .iter()
            .any(|entry| entry.page.url.contains("device-b")));
        let second_tabs = &second.payload.open_tabs.as_ref().unwrap().devices;
        assert!(second_tabs
            .iter()
            .any(|device| device.device_id == "device_a"));
        assert!(second_tabs
            .iter()
            .any(|device| device.device_id == "device_b"));

        // Simulate device A attempting to write its old snapshot with the
        // revision captured before device B's successful update.
        let stale_plaintext = payload_a.encode().unwrap();
        let stale_envelope = encrypt(&device_a, &stale_plaintext).unwrap();
        let stale_body = serde_json::to_vec(&stale_envelope).unwrap();
        let stale_write = http
            .put(&endpoint)
            .header(reqwest::header::AUTHORIZATION, &token)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(reqwest::header::IF_MATCH, stale_revision)
            .body(stale_body)
            .send()
            .unwrap();
        assert_eq!(stale_write.status().as_u16(), 412);

        // The normal client path then rereads the newer revision and merges
        // A's local state back without erasing B's data.
        let after_stale_retry = sync_payload(&device_a, payload_a, None).unwrap();
        assert_eq!(
            after_stale_retry
                .payload
                .bookmarks
                .as_ref()
                .unwrap()
                .items
                .len(),
            2
        );
        assert_eq!(
            after_stale_retry
                .payload
                .history
                .as_ref()
                .unwrap()
                .items
                .len(),
            2
        );
        assert_eq!(
            after_stale_retry
                .payload
                .open_tabs
                .as_ref()
                .unwrap()
                .devices
                .len(),
            2
        );

        let persisted = http
            .get(&endpoint)
            .header(reqwest::header::AUTHORIZATION, &token)
            .send()
            .unwrap();
        assert_eq!(persisted.status().as_u16(), 200);
        let encrypted: EncryptedEnvelope =
            serde_json::from_slice(&persisted.bytes().unwrap()).unwrap();
        let plaintext = decrypt(&device_a, &encrypted).unwrap();
        let stored = sync_v2::SyncPayloadV2::decode_and_migrate(&plaintext).unwrap();
        assert_eq!(stored.bookmarks.unwrap().items.len(), 2);
        assert_eq!(stored.history.unwrap().items.len(), 2);
        assert_eq!(stored.open_tabs.unwrap().devices.len(), 2);

        cleanup.revoke().unwrap();
        let revoked = http
            .get(&endpoint)
            .header(reqwest::header::AUTHORIZATION, &token)
            .send()
            .unwrap();
        assert_eq!(revoked.status().as_u16(), 410);
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires the live Cloudflare Pip Sync endpoint"]
    fn sync_roundtrip_and_remote_deletion_use_live_worker() {
        let credentials = SyncCredentials::generate();
        struct RemoveTestVaultOnDrop<'a>(&'a SyncCredentials);
        impl Drop for RemoveTestVaultOnDrop<'_> {
            fn drop(&mut self) {
                let _ = delete_vault(self.0, None);
            }
        }
        let cleanup = RemoveTestVaultOnDrop(&credentials);
        let page = Page {
            url: "https://sync-test.invalid/".into(),
            title: "Temporary test".into(),
        };
        let first = sync_bookmarks(
            &credentials,
            BookmarkSnapshot {
                version: 1,
                bookmarks: vec![page.clone()],
                removed_bookmarks: vec![],
            },
            None,
        )
        .unwrap();
        assert_eq!(first.payload.bookmarks.as_ref().unwrap().items.len(), 1);
        let deletion = sync_bookmarks(
            &credentials,
            BookmarkSnapshot {
                version: 1,
                bookmarks: vec![],
                removed_bookmarks: vec![page.url.clone()],
            },
            None,
        )
        .unwrap();
        assert!(deletion
            .payload
            .bookmarks
            .as_ref()
            .unwrap()
            .items
            .is_empty());
        let stale_device = sync_bookmarks(
            &credentials,
            BookmarkSnapshot {
                version: 1,
                bookmarks: vec![page],
                removed_bookmarks: vec![],
            },
            None,
        )
        .unwrap();
        assert!(stale_device
            .payload
            .bookmarks
            .as_ref()
            .unwrap()
            .items
            .is_empty());

        delete_vault(&credentials, None).unwrap();
        drop(cleanup);
        let cannot_recreate = sync_bookmarks(
            &credentials,
            BookmarkSnapshot {
                version: 1,
                bookmarks: vec![],
                removed_bookmarks: vec![],
            },
            None,
        );
        assert!(cannot_recreate.unwrap_err().contains("đã bị xóa"));
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires the live Cloudflare Pip Sync endpoint"]
    fn sync_v4_categories_roundtrip_live_worker() {
        use sync_v2::{
            BookmarkCategory, DeviceTabs, HistoryCategory, HistoryEntry, OpenTab, OpenTabsCategory,
            PasswordsCategory, SearchEngine, SyncPage, SyncPasswordEntry, SyncPayloadV2,
            SyncableSettings, Theme, TimedBookmark, UiDensity, UrlTombstone,
        };

        let credentials = SyncCredentials::generate();
        struct RemoveTestVaultOnDrop<'a>(&'a SyncCredentials);
        impl Drop for RemoveTestVaultOnDrop<'_> {
            fn drop(&mut self) {
                let _ = delete_vault(self.0, None);
            }
        }
        let cleanup = RemoveTestVaultOnDrop(&credentials);
        let test_password = "pip-sync-live-test-password";
        let initial = SyncPayloadV2 {
            schema_version: sync_v2::INNER_SCHEMA_VERSION,
            bookmarks: Some(BookmarkCategory {
                items: vec![TimedBookmark {
                    page: SyncPage {
                        url: "https://sync-test.invalid/bookmark".into(),
                        title: "Disposable bookmark".into(),
                    },
                    updated_at_ms: 10,
                }],
                tombstones: vec![UrlTombstone {
                    url: "https://sync-test.invalid/deleted".into(),
                    deleted_at_ms: 11,
                }],
                folders: vec![],
                placements: vec![],
                folder_tombstones: vec![],
            }),
            history: Some(HistoryCategory {
                items: vec![HistoryEntry {
                    page: SyncPage {
                        url: "https://sync-test.invalid/history".into(),
                        title: "Disposable history".into(),
                    },
                    visited_at_ms: 12,
                }],
                tombstones: vec![],
                visit_tombstones: vec![],
            }),
            open_tabs: Some(OpenTabsCategory {
                devices: vec![DeviceTabs {
                    device_id: "live-test-device".into(),
                    updated_at_ms: 13,
                    tabs: vec![OpenTab {
                        url: "https://sync-test.invalid/tab".into(),
                        title: "Disposable tab".into(),
                        pinned: false,
                        group: Some("work".into()),
                    }],
                }],
            }),
            settings: Some(SyncableSettings {
                updated_at_ms: 14,
                search_engine: SearchEngine::Google,
                theme: Theme::System,
                density: UiDensity::Comfortable,
                default_zoom_percent: 100,
                show_status_preview: true,
                show_zoom_badge: true,
                enable_tab_reordering: true,
                enable_tab_tearoff: true,
                warn_on_close_tabs: true,
            }),
            passwords: Some(PasswordsCategory {
                version: 1,
                entries: vec![SyncPasswordEntry {
                    origin: "https://sync-test.invalid".into(),
                    username: "live-test-user".into(),
                    password: test_password.into(),
                    updated_at_ms: 15,
                }],
                tombstones: vec![],
            }),
        };

        let uploaded = sync_payload(&credentials, initial, None).unwrap();
        assert_eq!(uploaded.payload.bookmarks.as_ref().unwrap().items.len(), 1);
        assert_eq!(uploaded.payload.history.as_ref().unwrap().items.len(), 1);
        assert_eq!(
            uploaded.payload.open_tabs.as_ref().unwrap().devices.len(),
            1
        );
        assert_eq!(
            uploaded.payload.settings.as_ref().unwrap().updated_at_ms,
            14
        );
        assert_eq!(
            uploaded.payload.passwords.as_ref().unwrap().entries[0].password,
            test_password
        );

        let downloaded = sync_payload(
            &credentials,
            SyncPayloadV2 {
                schema_version: sync_v2::INNER_SCHEMA_VERSION,
                bookmarks: None,
                history: None,
                open_tabs: None,
                settings: None,
                passwords: None,
            },
            None,
        )
        .unwrap();
        assert_eq!(downloaded.payload, uploaded.payload);

        delete_vault(&credentials, None).unwrap();
        drop(cleanup);
        let after_revoke = sync_payload(
            &credentials,
            SyncPayloadV2 {
                schema_version: sync_v2::INNER_SCHEMA_VERSION,
                bookmarks: None,
                history: None,
                open_tabs: None,
                settings: None,
                passwords: None,
            },
            None,
        );
        assert!(after_revoke.unwrap_err().contains("đã bị xóa"));
    }

    #[cfg(windows)]
    #[test]
    fn local_sync_credentials_are_protected_by_windows_dpapi() {
        let credentials = SyncCredentials::generate();
        let protected = protect_credentials(&credentials).unwrap();
        assert!(!protected.contains(&credentials.recovery_key));
        assert_eq!(unprotect_credentials(&protected).unwrap(), credentials);
    }
}
