//! Client-side authenticated encryption for the opaque Cloudflare sync vault.

use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use evergreen_core::library::Page;
use hkdf::Hkdf;
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

const AAD: &[u8] = b"pip-browser-sync-v1";
const SYNC_ENDPOINT: &str = "https://sync.psharehub.com/v1/vault";
const MAX_SYNC_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookmarkSnapshot {
    pub version: u8,
    #[serde(default)]
    pub bookmarks: Vec<Page>,
    #[serde(default)]
    pub removed_bookmarks: Vec<String>,
}

#[derive(Debug)]
pub struct SyncOutcome {
    pub bookmarks: Vec<Page>,
    pub removed_bookmarks: Vec<String>,
    pub message: String,
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
            vault_id: parts[1].to_string(),
            access_token: parts[2].to_string(),
            recovery_key: parts[3].to_string(),
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
    let bytes = decode_hex(value).ok_or(SyncCryptoError::InvalidEnvelope)?;
    let plaintext = unprotect_local_secret(&bytes)?;
    serde_json::from_slice(&plaintext).map_err(|_| SyncCryptoError::InvalidEnvelope)
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
#[cfg(windows)]
pub fn sync_bookmarks(
    credentials: &SyncCredentials,
    local: BookmarkSnapshot,
) -> Result<SyncOutcome, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|_| "Không tạo được kết nối Pip Sync.".to_string())?;
    let url = format!("{SYNC_ENDPOINT}/{}", credentials.vault_id);
    let token = format!("Bearer {}", credentials.access_token);
    let mut local = normalize_snapshot(local)?;

    for attempt in 0..2 {
        let response = client
            .get(&url)
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
                let bytes = response
                    .bytes()
                    .map_err(|_| "Không đọc được dữ liệu đồng bộ.".to_string())?;
                if bytes.len() > MAX_SYNC_BYTES {
                    return Err("Dữ liệu đồng bộ vượt giới hạn 1 MB.".into());
                }
                let envelope: EncryptedEnvelope = serde_json::from_slice(&bytes)
                    .map_err(|_| "Gói đồng bộ trên máy chủ không hợp lệ.".to_string())?;
                let plain = decrypt(credentials, &envelope).map_err(|error| error.to_string())?;
                if plain.len() > MAX_SYNC_BYTES {
                    return Err("Dữ liệu dấu trang vượt giới hạn 1 MB.".into());
                }
                let snapshot: BookmarkSnapshot = serde_json::from_slice(&plain)
                    .map_err(|_| "Dữ liệu dấu trang đồng bộ không hợp lệ.".to_string())?;
                (Some(normalize_snapshot(snapshot)?), Some(revision))
            }
            404 => (None, None),
            401 => return Err("Mã đồng bộ không khớp hoặc đã bị thu hồi.".into()),
            410 => {
                return Err(
                    "Kho đồng bộ đã bị xóa. Tạo mã mới hoặc nhập mã khác để tiếp tục.".into(),
                )
            }
            _ => return Err("Máy chủ Pip Sync chưa sẵn sàng. Hãy thử lại sau.".into()),
        };

        if let Some(remote) = remote {
            local = merge_snapshots(local, remote);
        }
        let plain = serde_json::to_vec(&local)
            .map_err(|_| "Không thể chuẩn bị dấu trang để đồng bộ.".to_string())?;
        let envelope = encrypt(credentials, &plain).map_err(|error| error.to_string())?;
        let payload =
            serde_json::to_vec(&envelope).map_err(|_| "Không thể mã hóa dấu trang.".to_string())?;
        if payload.len() > MAX_SYNC_BYTES {
            return Err("Gói dấu trang vượt giới hạn 1 MB của Pip Sync.".into());
        }
        let mut request = client
            .put(&url)
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
                bookmarks: local.bookmarks,
                removed_bookmarks: local.removed_bookmarks,
                message: "Đã đồng bộ dấu trang an toàn.".into(),
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
pub fn delete_vault(credentials: &SyncCredentials) -> Result<(), String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|_| "Không tạo được kết nối Pip Sync.".to_string())?;
    let url = format!("{SYNC_ENDPOINT}/{}", credentials.vault_id);
    let authorization = format!("Bearer {}", credentials.access_token);
    let current = client
        .get(&url)
        .header(reqwest::header::AUTHORIZATION, &authorization)
        .send()
        .map_err(|_| "Không kết nối được Pip Sync để xóa kho.".to_string())?;
    if matches!(current.status().as_u16(), 404 | 410) {
        return Ok(());
    }
    if current.status().as_u16() == 401 {
        return Err("Mã Pip Sync không khớp; không thể xác nhận quyền xóa.".into());
    }
    if current.status().as_u16() != 200 {
        return Err("Máy chủ không xác nhận được kho đồng bộ.".into());
    }
    let revision = current
        .headers()
        .get(reqwest::header::ETAG)
        .ok_or_else(|| "Máy chủ thiếu phiên bản kho đồng bộ.".to_string())?
        .to_str()
        .map_err(|_| "Phiên bản kho đồng bộ không hợp lệ.".to_string())?;
    let deleted = client
        .delete(url)
        .header(reqwest::header::AUTHORIZATION, authorization)
        .header(reqwest::header::IF_MATCH, revision)
        .send()
        .map_err(|_| "Không xóa được kho đồng bộ trên máy chủ.".to_string())?;
    if deleted.status().as_u16() == 204 {
        Ok(())
    } else {
        Err("Kho đồng bộ đã thay đổi. Hãy thử tắt đồng bộ lại.".into())
    }
}

fn normalize_snapshot(mut snapshot: BookmarkSnapshot) -> Result<BookmarkSnapshot, String> {
    if snapshot.version != 1
        || snapshot.bookmarks.len() > 200
        || snapshot.removed_bookmarks.len() > 2000
    {
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
    fn recovery_code_roundtrips_and_secrets_are_high_entropy() {
        let credentials = SyncCredentials::generate();
        let restored = SyncCredentials::from_recovery_code(&credentials.recovery_code()).unwrap();
        assert_eq!(credentials, restored);
        assert_ne!(credentials.access_token, credentials.recovery_key);
        assert_eq!(credentials.vault_id.len(), 32);
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

    #[cfg(windows)]
    #[test]
    #[ignore = "requires the live Cloudflare Pip Sync endpoint"]
    fn sync_roundtrip_and_remote_deletion_use_live_worker() {
        let credentials = SyncCredentials::generate();
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
        )
        .unwrap();
        assert_eq!(first.bookmarks.len(), 1);
        let deletion = sync_bookmarks(
            &credentials,
            BookmarkSnapshot {
                version: 1,
                bookmarks: vec![],
                removed_bookmarks: vec![page.url.clone()],
            },
        )
        .unwrap();
        assert!(deletion.bookmarks.is_empty());
        let stale_device = sync_bookmarks(
            &credentials,
            BookmarkSnapshot {
                version: 1,
                bookmarks: vec![page],
                removed_bookmarks: vec![],
            },
        )
        .unwrap();
        assert!(stale_device.bookmarks.is_empty());

        delete_vault(&credentials).unwrap();
        let cannot_recreate = sync_bookmarks(
            &credentials,
            BookmarkSnapshot {
                version: 1,
                bookmarks: vec![],
                removed_bookmarks: vec![],
            },
        );
        assert!(cannot_recreate.unwrap_err().contains("đã bị xóa"));
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
