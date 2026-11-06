//! Versioned, allowlisted inner payload for cross-device Pip Sync.
//!
//! This module is deliberately separate from the V1 network client. Keep the
//! outer AES-GCM `EncryptedEnvelope` at version 1: the Cloudflare Worker stores
//! that envelope as opaque bytes and only validates its encoding and size.
//!
//! `decode_and_migrate` accepts bookmark-only V1, second-precision V2, and
//! millisecond-precision V3, folderless V4, and URL-deduplicated V5 plaintexts,
//! upgrading them to V6 in memory. Never serialize V6 fields under V1's `version` key: old clients ignore unknown
//! JSON keys and could then overwrite them. An older client receiving
//! `schema_version: 6` must fail
//! closed before PUT; callers should surface an update-required message and
//! retain both snapshots.
//!
//! Category absence means “not enabled / no data in this sync payload”, not a
//! deletion. The host must preserve local data for omitted categories. Only
//! construct snapshots from a regular profile window after explicit category
//! consent. This schema has no fields for credentials, cookies, payment data,
//! DPAPI material, profile IDs, or private-window state. Never serialize the
//! full application Settings, Profile, Library, or TabManager objects here.
//!
//! History can contain sensitive URLs (including query strings), so it must be
//! separately opt-in. Open-tab snapshots are keyed by a locally generated
//! pseudonymous device ID; the host must exclude private windows and internal
//! Pip pages before constructing them. Settings are closed enums and a small
//! allowlist; network, permission, download-path, privacy, and profile settings
//! intentionally remain device-local.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const INNER_SCHEMA_VERSION: u8 = 6;
/// Leaves room for AES-GCM overhead, hex encoding, and envelope JSON under the
/// Worker’s 1 MiB request-body ceiling.
pub const MAX_INNER_JSON_BYTES: usize = 450 * 1024;
pub const MAX_BOOKMARKS: usize = 200;
pub const MAX_BOOKMARK_FOLDERS: usize = 200;
pub const MAX_BOOKMARK_TOMBSTONES: usize = 2_000;
pub const MAX_HISTORY_ITEMS: usize = 500;
pub const MAX_HISTORY_TOMBSTONES: usize = 2_000;
pub const MAX_DEVICES: usize = 16;
pub const MAX_OPEN_TABS_PER_DEVICE: usize = 100;
pub const MAX_URL_BYTES: usize = 8_192;
pub const MAX_TITLE_CHARS: usize = 300;
pub const MAX_GROUP_NAME_CHARS: usize = 64;
pub const MAX_DEVICE_ID_BYTES: usize = 64;
pub const MAX_PASSWORDS: usize = 500;
pub const MAX_PASSWORD_TOMBSTONES: usize = 2_000;
pub const MAX_PASSWORD_ORIGIN_BYTES: usize = 4_096;
pub const MAX_PASSWORD_USERNAME_BYTES: usize = 1_024;
pub const MAX_PASSWORD_BYTES: usize = 4_096;
pub const MAX_PASSWORD_CATEGORY_BYTES: usize = 450 * 1024;
const MAX_LOCAL_TIMESTAMP_MS: u64 = i64::MAX as u64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncPage {
    pub url: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimedBookmark {
    pub page: SyncPage,
    /// Unix time in milliseconds. Millisecond precision prevents a delete and
    /// a later re-add in the same second from becoming an unresolved tie.
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UrlTombstone {
    pub url: String,
    /// Unix time in milliseconds.
    pub deleted_at_ms: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BookmarkCategory {
    pub items: Vec<TimedBookmark>,
    pub tombstones: Vec<UrlTombstone>,
    #[serde(default)]
    pub folders: Vec<SyncBookmarkFolder>,
    #[serde(default)]
    pub placements: Vec<BookmarkPlacement>,
    #[serde(default)]
    pub folder_tombstones: Vec<BookmarkFolderTombstone>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncBookmarkFolder {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BookmarkPlacement {
    pub url: String,
    pub folder_id: Option<String>,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BookmarkFolderTombstone {
    pub id: String,
    pub deleted_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryEntry {
    pub page: SyncPage,
    /// Unix time in milliseconds.
    pub visited_at_ms: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HistoryCategory {
    pub items: Vec<HistoryEntry>,
    pub tombstones: Vec<UrlTombstone>,
    #[serde(default)]
    pub visit_tombstones: Vec<HistoryVisitTombstone>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryVisitTombstone {
    pub url: String,
    pub visited_at_ms: u64,
    pub deleted_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenTab {
    pub url: String,
    pub title: String,
    pub pinned: bool,
    pub group: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceTabs {
    pub device_id: String,
    pub updated_at_ms: u64,
    pub tabs: Vec<OpenTab>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OpenTabsCategory {
    pub devices: Vec<DeviceTabs>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchEngine {
    Google,
    Bing,
    Brave,
    Ecosia,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiDensity {
    Comfortable,
    Compact,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncableSettings {
    pub updated_at_ms: u64,
    pub search_engine: SearchEngine,
    pub theme: Theme,
    pub density: UiDensity,
    /// Zoom percentage, clamped to 50–200 by validation.
    pub default_zoom_percent: u16,
    pub show_status_preview: bool,
    pub show_zoom_badge: bool,
    pub enable_tab_reordering: bool,
    pub enable_tab_tearoff: bool,
    pub warn_on_close_tabs: bool,
}

/// Optional end-to-end encrypted password vault projection. This intentionally
/// contains no local numeric IDs or machine-bound DPAPI data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PasswordsCategory {
    pub version: u8,
    pub entries: Vec<SyncPasswordEntry>,
    pub tombstones: Vec<SyncPasswordTombstone>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncPasswordEntry {
    pub origin: String,
    pub username: String,
    pub password: String,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncPasswordTombstone {
    pub origin: String,
    pub username: String,
    pub deleted_at_ms: u64,
}

/// A deliberately closed allowlist: adding fields requires a schema decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncPayloadV2 {
    pub schema_version: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bookmarks: Option<BookmarkCategory>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history: Option<HistoryCategory>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_tabs: Option<OpenTabsCategory>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<SyncableSettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passwords: Option<PasswordsCategory>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacySyncPayloadV3 {
    schema_version: u8,
    #[serde(default)]
    bookmarks: Option<BookmarkCategory>,
    #[serde(default)]
    history: Option<HistoryCategory>,
    #[serde(default)]
    open_tabs: Option<OpenTabsCategory>,
    #[serde(default)]
    settings: Option<SyncableSettings>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacySyncPayloadV4 {
    schema_version: u8,
    #[serde(default)]
    bookmarks: Option<LegacyBookmarkCategoryV4>,
    #[serde(default)]
    history: Option<HistoryCategory>,
    #[serde(default)]
    open_tabs: Option<OpenTabsCategory>,
    #[serde(default)]
    settings: Option<SyncableSettings>,
    #[serde(default)]
    passwords: Option<PasswordsCategory>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyBookmarkCategoryV4 {
    #[serde(default)]
    items: Vec<TimedBookmark>,
    #[serde(default)]
    tombstones: Vec<UrlTombstone>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyBookmarkSnapshotV1 {
    version: u8,
    #[serde(default)]
    bookmarks: Vec<SyncPage>,
    #[serde(default)]
    removed_bookmarks: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyTimedBookmarkV2 {
    page: SyncPage,
    updated_at_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyUrlTombstoneV2 {
    url: String,
    deleted_at_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyBookmarkCategoryV2 {
    #[serde(default)]
    items: Vec<LegacyTimedBookmarkV2>,
    #[serde(default)]
    tombstones: Vec<LegacyUrlTombstoneV2>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyHistoryEntryV2 {
    page: SyncPage,
    visited_at_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyHistoryCategoryV2 {
    #[serde(default)]
    items: Vec<LegacyHistoryEntryV2>,
    #[serde(default)]
    tombstones: Vec<LegacyUrlTombstoneV2>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyDeviceTabsV2 {
    device_id: String,
    updated_at_secs: u64,
    tabs: Vec<OpenTab>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyOpenTabsCategoryV2 {
    #[serde(default)]
    devices: Vec<LegacyDeviceTabsV2>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacySyncableSettingsV2 {
    updated_at_secs: u64,
    search_engine: SearchEngine,
    theme: Theme,
    density: UiDensity,
    default_zoom_percent: u16,
    show_status_preview: bool,
    show_zoom_badge: bool,
    enable_tab_reordering: bool,
    enable_tab_tearoff: bool,
    warn_on_close_tabs: bool,
}

impl From<LegacySyncableSettingsV2> for SyncableSettings {
    fn from(old: LegacySyncableSettingsV2) -> Self {
        Self {
            updated_at_ms: old.updated_at_secs.saturating_mul(1_000),
            search_engine: old.search_engine,
            theme: old.theme,
            density: old.density,
            default_zoom_percent: old.default_zoom_percent,
            show_status_preview: old.show_status_preview,
            show_zoom_badge: old.show_zoom_badge,
            enable_tab_reordering: old.enable_tab_reordering,
            enable_tab_tearoff: old.enable_tab_tearoff,
            warn_on_close_tabs: old.warn_on_close_tabs,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacySyncPayloadV2 {
    schema_version: u8,
    #[serde(default)]
    bookmarks: Option<LegacyBookmarkCategoryV2>,
    #[serde(default)]
    history: Option<LegacyHistoryCategoryV2>,
    #[serde(default)]
    open_tabs: Option<LegacyOpenTabsCategoryV2>,
    #[serde(default)]
    settings: Option<LegacySyncableSettingsV2>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncPayloadError {
    InvalidJson,
    PayloadTooLarge,
    UnsupportedVersion(u64),
    MissingVersion,
    InvalidData(&'static str),
}

impl std::fmt::Display for SyncPayloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidJson => f.write_str("Invalid encrypted sync payload JSON"),
            Self::PayloadTooLarge => f.write_str("Sync payload exceeds the safe size limit"),
            Self::UnsupportedVersion(version) => {
                write!(f, "Unsupported sync payload schema version {version}")
            }
            Self::MissingVersion => f.write_str("Sync payload has no schema version"),
            Self::InvalidData(message) => write!(f, "Invalid sync payload: {message}"),
        }
    }
}

impl std::error::Error for SyncPayloadError {}

impl SyncPayloadV2 {
    pub fn empty() -> Self {
        Self {
            schema_version: INNER_SCHEMA_VERSION,
            bookmarks: None,
            history: None,
            open_tabs: None,
            settings: None,
            passwords: None,
        }
    }

    pub fn decode_and_migrate(bytes: &[u8]) -> Result<Self, SyncPayloadError> {
        if bytes.len() > MAX_INNER_JSON_BYTES {
            return Err(SyncPayloadError::PayloadTooLarge);
        }
        let value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|_| SyncPayloadError::InvalidJson)?;
        let object = value.as_object().ok_or(SyncPayloadError::MissingVersion)?;

        if let Some(version) = object.get("schema_version") {
            let version = version.as_u64().ok_or(SyncPayloadError::MissingVersion)?;
            if version == 2 {
                let legacy: LegacySyncPayloadV2 =
                    serde_json::from_value(value).map_err(|_| SyncPayloadError::InvalidJson)?;
                return legacy.into_current().validate_and_normalize();
            }
            if version == 3 {
                let legacy: LegacySyncPayloadV3 =
                    serde_json::from_value(value).map_err(|_| SyncPayloadError::InvalidJson)?;
                return Self {
                    schema_version: INNER_SCHEMA_VERSION,
                    bookmarks: legacy.bookmarks,
                    history: legacy.history,
                    open_tabs: legacy.open_tabs,
                    settings: legacy.settings,
                    passwords: None,
                }
                .validate_and_normalize();
            }
            if version == 4 {
                let legacy: LegacySyncPayloadV4 =
                    serde_json::from_value(value).map_err(|_| SyncPayloadError::InvalidJson)?;
                let bookmarks = legacy.bookmarks.map(|category| BookmarkCategory {
                    items: category.items,
                    tombstones: category.tombstones,
                    folders: Vec::new(),
                    placements: Vec::new(),
                    folder_tombstones: Vec::new(),
                });
                return Self {
                    schema_version: INNER_SCHEMA_VERSION,
                    bookmarks,
                    history: legacy.history,
                    open_tabs: legacy.open_tabs,
                    settings: legacy.settings,
                    passwords: legacy.passwords,
                }
                .validate_and_normalize();
            }
            if version == 5 {
                // V5 has the same envelope fields, but its history merger kept
                // only one item per URL. Preserve those surviving entries as
                // individual legacy visits while upgrading the inner version.
                let mut legacy: Self =
                    serde_json::from_value(value).map_err(|_| SyncPayloadError::InvalidJson)?;
                legacy.schema_version = INNER_SCHEMA_VERSION;
                return legacy.validate_and_normalize();
            }
            if version != u64::from(INNER_SCHEMA_VERSION) {
                return Err(SyncPayloadError::UnsupportedVersion(version));
            }
            let payload: Self =
                serde_json::from_value(value).map_err(|_| SyncPayloadError::InvalidJson)?;
            return payload.validate_and_normalize();
        }

        let version = object
            .get("version")
            .and_then(serde_json::Value::as_u64)
            .ok_or(SyncPayloadError::MissingVersion)?;
        if version != 1 {
            return Err(SyncPayloadError::UnsupportedVersion(version));
        }
        let legacy: LegacyBookmarkSnapshotV1 =
            serde_json::from_value(value).map_err(|_| SyncPayloadError::InvalidJson)?;
        if legacy.version != 1 {
            return Err(SyncPayloadError::UnsupportedVersion(u64::from(
                legacy.version,
            )));
        }
        let mut payload = Self::empty();
        payload.bookmarks = Some(BookmarkCategory {
            items: legacy
                .bookmarks
                .into_iter()
                .map(|page| TimedBookmark {
                    page,
                    updated_at_ms: 0,
                })
                .collect(),
            tombstones: legacy
                .removed_bookmarks
                .into_iter()
                .map(|url| UrlTombstone {
                    url,
                    deleted_at_ms: 0,
                })
                .collect(),
            folders: Vec::new(),
            placements: Vec::new(),
            folder_tombstones: Vec::new(),
        });
        payload.validate_and_normalize()
    }

    pub fn validate_and_normalize(mut self) -> Result<Self, SyncPayloadError> {
        if self.schema_version != INNER_SCHEMA_VERSION {
            return Err(SyncPayloadError::UnsupportedVersion(u64::from(
                self.schema_version,
            )));
        }
        if let Some(category) = &mut self.bookmarks {
            normalize_bookmarks(category)?;
        }
        if let Some(category) = &mut self.history {
            normalize_history(category)?;
        }
        if let Some(category) = &mut self.open_tabs {
            normalize_tabs(category)?;
        }
        if let Some(settings) = &self.settings {
            if !(50..=200).contains(&settings.default_zoom_percent) {
                return Err(SyncPayloadError::InvalidData("zoom must be 50–200%"));
            }
        }
        if let Some(passwords) = &mut self.passwords {
            normalize_passwords(passwords)?;
        }
        Ok(self)
    }

    /// Validate and encode for the existing V1 AES-GCM envelope. Callers
    /// should use this instead of serializing this struct directly.
    pub fn encode(&self) -> Result<Vec<u8>, SyncPayloadError> {
        let normalized = self.clone().validate_and_normalize()?;
        let bytes = serde_json::to_vec(&normalized).map_err(|_| SyncPayloadError::InvalidJson)?;
        if bytes.len() > MAX_INNER_JSON_BYTES {
            return Err(SyncPayloadError::PayloadTooLarge);
        }
        Ok(bytes)
    }

    /// Merge enabled categories. Missing categories are preserved from either
    /// side rather than interpreted as empty/deleted data.
    pub fn merge(self, other: Self) -> Result<Self, SyncPayloadError> {
        let mut left = self.validate_and_normalize()?;
        let right = other.validate_and_normalize()?;

        left.bookmarks = match (left.bookmarks, right.bookmarks) {
            (Some(left), Some(right)) => Some(merge_bookmarks(left, right)?),
            (Some(value), None) | (None, Some(value)) => Some(value),
            (None, None) => None,
        };
        left.history = merge_optional(left.history, right.history, merge_history);
        left.open_tabs = merge_optional(left.open_tabs, right.open_tabs, merge_tabs);
        left.settings = merge_settings(left.settings, right.settings);
        left.passwords = match (left.passwords, right.passwords) {
            (Some(left), Some(right)) => Some(merge_passwords(left, right)?),
            (Some(value), None) | (None, Some(value)) => Some(value),
            (None, None) => None,
        };
        left.validate_and_normalize()
    }
}

impl LegacySyncPayloadV2 {
    fn into_current(self) -> SyncPayloadV2 {
        let to_ms = |seconds: u64| seconds.saturating_mul(1_000);
        SyncPayloadV2 {
            schema_version: INNER_SCHEMA_VERSION,
            bookmarks: self.bookmarks.map(|category| BookmarkCategory {
                items: category
                    .items
                    .into_iter()
                    .map(|entry| TimedBookmark {
                        page: entry.page,
                        updated_at_ms: to_ms(entry.updated_at_secs),
                    })
                    .collect(),
                tombstones: category
                    .tombstones
                    .into_iter()
                    .map(|entry| UrlTombstone {
                        url: entry.url,
                        deleted_at_ms: to_ms(entry.deleted_at_secs),
                    })
                    .collect(),
                folders: Vec::new(),
                placements: Vec::new(),
                folder_tombstones: Vec::new(),
            }),
            history: self.history.map(|category| HistoryCategory {
                items: category
                    .items
                    .into_iter()
                    .map(|entry| HistoryEntry {
                        page: entry.page,
                        visited_at_ms: to_ms(entry.visited_at_secs),
                    })
                    .collect(),
                tombstones: category
                    .tombstones
                    .into_iter()
                    .map(|entry| UrlTombstone {
                        url: entry.url,
                        deleted_at_ms: to_ms(entry.deleted_at_secs),
                    })
                    .collect(),
                visit_tombstones: Vec::new(),
            }),
            open_tabs: self.open_tabs.map(|category| OpenTabsCategory {
                devices: category
                    .devices
                    .into_iter()
                    .map(|device| DeviceTabs {
                        device_id: device.device_id,
                        updated_at_ms: to_ms(device.updated_at_secs),
                        tabs: device.tabs,
                    })
                    .collect(),
            }),
            settings: self.settings.map(Into::into),
            passwords: None,
        }
    }
}

fn merge_optional<T>(left: Option<T>, right: Option<T>, merge: fn(T, T) -> T) -> Option<T> {
    match (left, right) {
        (Some(left), Some(right)) => Some(merge(left, right)),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

fn merge_bookmarks(
    left: BookmarkCategory,
    right: BookmarkCategory,
) -> Result<BookmarkCategory, SyncPayloadError> {
    let mut entries = BTreeMap::<String, TimedBookmark>::new();
    for entry in left.items.into_iter().chain(right.items) {
        let key = entry.page.url.clone();
        entries
            .entry(key)
            .and_modify(|current| {
                if (entry.updated_at_ms, &entry.page.title)
                    > (current.updated_at_ms, &current.page.title)
                {
                    *current = entry.clone();
                }
            })
            .or_insert(entry);
    }
    let mut tombstones = merge_tombstones(left.tombstones, right.tombstones);
    entries.retain(|url, entry| {
        !tombstones
            .iter()
            .any(|deleted| deleted.url == *url && deleted.deleted_at_ms >= entry.updated_at_ms)
    });
    let mut items: Vec<_> = entries.into_values().collect();
    items.sort_by(|a, b| {
        b.updated_at_ms
            .cmp(&a.updated_at_ms)
            .then_with(|| a.page.url.cmp(&b.page.url))
    });
    tombstones.sort_by(|a, b| {
        b.deleted_at_ms
            .cmp(&a.deleted_at_ms)
            .then_with(|| a.url.cmp(&b.url))
    });
    let folder_tombstones =
        merge_folder_tombstones(left.folder_tombstones, right.folder_tombstones)?;
    let mut deleted_folders: std::collections::HashSet<String> = folder_tombstones
        .iter()
        .map(|item| item.id.clone())
        .collect();
    let mut folders = BTreeMap::<String, SyncBookmarkFolder>::new();
    for folder in left.folders.into_iter().chain(right.folders) {
        folders
            .entry(folder.id.clone())
            .and_modify(|current| {
                if (&folder.name, &folder.parent_id) < (&current.name, &current.parent_id) {
                    *current = folder.clone();
                }
            })
            .or_insert(folder);
    }
    loop {
        let before = deleted_folders.len();
        for folder in folders.values() {
            if folder
                .parent_id
                .as_ref()
                .is_some_and(|parent| deleted_folders.contains(parent))
            {
                deleted_folders.insert(folder.id.clone());
            }
        }
        if before == deleted_folders.len() {
            break;
        }
    }
    folders.retain(|id, _| !deleted_folders.contains(id));
    let mut placements = BTreeMap::<String, BookmarkPlacement>::new();
    for placement in left.placements.into_iter().chain(right.placements) {
        placements
            .entry(placement.url.clone())
            .and_modify(|current| {
                if (placement.updated_at_ms, &placement.folder_id)
                    > (current.updated_at_ms, &current.folder_id)
                {
                    *current = placement.clone();
                }
            })
            .or_insert(placement);
    }
    for placement in placements.values_mut() {
        if placement
            .folder_id
            .as_ref()
            .is_some_and(|id| deleted_folders.contains(id))
        {
            // Folder deletion moves its bookmarks to the root and dominates a
            // stale placement to the deleted subtree from another device.
            placement.folder_id = None;
        }
    }
    let folders = folders.into_values().collect::<Vec<_>>();
    let placements = placements.into_values().collect::<Vec<_>>();
    if folders.len() > MAX_BOOKMARK_FOLDERS || placements.len() > MAX_BOOKMARKS {
        return Err(SyncPayloadError::InvalidData(
            "bookmark folder merge exceeds safe limits",
        ));
    }
    Ok(BookmarkCategory {
        items,
        tombstones,
        folders,
        placements,
        folder_tombstones,
    })
}

fn merge_folder_tombstones(
    left: Vec<BookmarkFolderTombstone>,
    right: Vec<BookmarkFolderTombstone>,
) -> Result<Vec<BookmarkFolderTombstone>, SyncPayloadError> {
    let mut merged = BTreeMap::<String, BookmarkFolderTombstone>::new();
    for tombstone in left.into_iter().chain(right) {
        merged
            .entry(tombstone.id.clone())
            .and_modify(|current| {
                current.deleted_at_ms = current.deleted_at_ms.max(tombstone.deleted_at_ms)
            })
            .or_insert(tombstone);
    }
    let mut values: Vec<_> = merged.into_values().collect();
    if values.len() > MAX_BOOKMARK_TOMBSTONES {
        return Err(SyncPayloadError::InvalidData(
            "bookmark folder deletion history exceeds safe limits",
        ));
    }
    values.sort_by(|a, b| {
        b.deleted_at_ms
            .cmp(&a.deleted_at_ms)
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(values)
}

fn merge_history(left: HistoryCategory, right: HistoryCategory) -> HistoryCategory {
    let mut entries = BTreeMap::<(String, u64), HistoryEntry>::new();
    for entry in left.items.into_iter().chain(right.items) {
        let key = (entry.page.url.clone(), entry.visited_at_ms);
        entries
            .entry(key)
            .and_modify(|current| {
                if entry.page.title > current.page.title {
                    *current = entry.clone();
                }
            })
            .or_insert(entry);
    }
    let mut tombstones = merge_tombstones(left.tombstones, right.tombstones);
    let mut visit_tombstones = BTreeMap::<(String, u64), HistoryVisitTombstone>::new();
    for tombstone in left
        .visit_tombstones
        .into_iter()
        .chain(right.visit_tombstones)
    {
        let key = (tombstone.url.clone(), tombstone.visited_at_ms);
        visit_tombstones
            .entry(key)
            .and_modify(|current| {
                current.deleted_at_ms = current.deleted_at_ms.max(tombstone.deleted_at_ms);
            })
            .or_insert(tombstone);
    }
    entries.retain(|(url, _), entry| {
        !tombstones
            .iter()
            .any(|deleted| deleted.url == *url && deleted.deleted_at_ms >= entry.visited_at_ms)
            && !visit_tombstones.contains_key(&(url.clone(), entry.visited_at_ms))
    });
    let mut items: Vec<_> = entries.into_values().collect();
    items.sort_by(|a, b| {
        b.visited_at_ms
            .cmp(&a.visited_at_ms)
            .then_with(|| a.page.url.cmp(&b.page.url))
            .then_with(|| a.page.title.cmp(&b.page.title))
    });
    tombstones.sort_by(|a, b| {
        b.deleted_at_ms
            .cmp(&a.deleted_at_ms)
            .then_with(|| a.url.cmp(&b.url))
    });
    let mut visit_tombstones: Vec<_> = visit_tombstones.into_values().collect();
    visit_tombstones.sort_by(|a, b| {
        b.deleted_at_ms
            .cmp(&a.deleted_at_ms)
            .then_with(|| a.url.cmp(&b.url))
            .then_with(|| a.visited_at_ms.cmp(&b.visited_at_ms))
    });
    HistoryCategory {
        items,
        tombstones,
        visit_tombstones,
    }
}

fn merge_tabs(left: OpenTabsCategory, right: OpenTabsCategory) -> OpenTabsCategory {
    let mut devices = BTreeMap::<String, DeviceTabs>::new();
    for device in left.devices.into_iter().chain(right.devices) {
        devices
            .entry(device.device_id.clone())
            .and_modify(|current| {
                if (device.updated_at_ms, &device.tabs) > (current.updated_at_ms, &current.tabs) {
                    *current = device.clone();
                }
            })
            .or_insert(device);
    }
    let mut devices: Vec<_> = devices.into_values().collect();
    devices.sort_by(|a, b| {
        b.updated_at_ms
            .cmp(&a.updated_at_ms)
            .then_with(|| a.device_id.cmp(&b.device_id))
    });
    OpenTabsCategory { devices }
}

fn merge_settings(
    left: Option<SyncableSettings>,
    right: Option<SyncableSettings>,
) -> Option<SyncableSettings> {
    match (left, right) {
        (Some(left), Some(right)) => {
            if (right.updated_at_ms, &right) > (left.updated_at_ms, &left) {
                Some(right)
            } else {
                Some(left)
            }
        }
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

fn validate_password_identity(origin: &str, username: &str) -> Result<(), SyncPayloadError> {
    if origin.len() > MAX_PASSWORD_ORIGIN_BYTES || origin.chars().any(char::is_control) {
        return Err(SyncPayloadError::InvalidData("invalid password origin"));
    }
    let uri = origin
        .parse::<wry::http::Uri>()
        .map_err(|_| SyncPayloadError::InvalidData("invalid password origin"))?;
    let scheme = uri
        .scheme_str()
        .ok_or(SyncPayloadError::InvalidData("invalid password origin"))?
        .to_ascii_lowercase();
    if !matches!(scheme.as_str(), "http" | "https") {
        return Err(SyncPayloadError::InvalidData("invalid password origin"));
    }
    let authority = uri
        .authority()
        .ok_or(SyncPayloadError::InvalidData("invalid password origin"))?;
    if authority.as_str().contains('@') {
        return Err(SyncPayloadError::InvalidData(
            "password origin must not contain credentials",
        ));
    }
    let host = uri
        .host()
        .ok_or(SyncPayloadError::InvalidData("invalid password origin"))?
        .to_ascii_lowercase();
    let port = uri
        .port_u16()
        .filter(|port| !((*port == 80 && scheme == "http") || (*port == 443 && scheme == "https")));
    let canonical = match port {
        Some(port) => format!("{scheme}://{host}:{port}"),
        None => format!("{scheme}://{host}"),
    };
    if origin != canonical {
        return Err(SyncPayloadError::InvalidData(
            "password origin is not canonical",
        ));
    }
    if username.is_empty()
        || username.len() > MAX_PASSWORD_USERNAME_BYTES
        || username.chars().any(char::is_control)
        || username.trim() != username
    {
        return Err(SyncPayloadError::InvalidData("invalid password username"));
    }
    Ok(())
}

fn normalize_passwords(category: &mut PasswordsCategory) -> Result<(), SyncPayloadError> {
    if category.version != 1 {
        return Err(SyncPayloadError::InvalidData(
            "unsupported password category version",
        ));
    }
    if category.entries.len() > MAX_PASSWORDS || category.tombstones.len() > MAX_PASSWORD_TOMBSTONES
    {
        return Err(SyncPayloadError::InvalidData(
            "password record limits exceeded",
        ));
    }
    let mut entries = BTreeMap::<(String, String), SyncPasswordEntry>::new();
    for entry in category.entries.drain(..) {
        validate_password_identity(&entry.origin, &entry.username)?;
        if entry.password.is_empty()
            || entry.password.len() > MAX_PASSWORD_BYTES
            || entry.password.chars().any(char::is_control)
            || entry.updated_at_ms == u64::MAX
        {
            return Err(SyncPayloadError::InvalidData(
                "invalid password value or timestamp",
            ));
        }
        let key = (entry.origin.clone(), entry.username.clone());
        if entries.insert(key, entry).is_some() {
            return Err(SyncPayloadError::InvalidData("duplicate password identity"));
        }
    }
    let mut tombstones = BTreeMap::<(String, String), SyncPasswordTombstone>::new();
    for tombstone in category.tombstones.drain(..) {
        validate_password_identity(&tombstone.origin, &tombstone.username)?;
        if tombstone.deleted_at_ms == u64::MAX {
            return Err(SyncPayloadError::InvalidData(
                "invalid password deletion timestamp",
            ));
        }
        let key = (tombstone.origin.clone(), tombstone.username.clone());
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
            .is_some_and(|tombstone| tombstone.deleted_at_ms >= entry.updated_at_ms)
    });
    category.entries = entries.into_values().collect();
    category.tombstones = tombstones.into_values().collect();
    let encoded = serde_json::to_vec(category).map_err(|_| SyncPayloadError::InvalidJson)?;
    if encoded.len() > MAX_PASSWORD_CATEGORY_BYTES {
        return Err(SyncPayloadError::InvalidData(
            "password category exceeds safe size limit",
        ));
    }
    Ok(())
}

fn merge_passwords(
    left: PasswordsCategory,
    right: PasswordsCategory,
) -> Result<PasswordsCategory, SyncPayloadError> {
    let mut left = left;
    let mut right = right;
    normalize_passwords(&mut left)?;
    normalize_passwords(&mut right)?;
    let mut entries = BTreeMap::<(String, String), SyncPasswordEntry>::new();
    for entry in left.entries.into_iter().chain(right.entries) {
        let key = (entry.origin.clone(), entry.username.clone());
        if let Some(current) = entries.get_mut(&key) {
            if entry.updated_at_ms > current.updated_at_ms {
                *current = entry;
            } else if entry.updated_at_ms == current.updated_at_ms
                && entry.password != current.password
            {
                return Err(SyncPayloadError::InvalidData(
                    "conflicting password updates share a timestamp",
                ));
            }
        } else {
            entries.insert(key, entry);
        }
    }
    let mut tombstones = BTreeMap::<(String, String), SyncPasswordTombstone>::new();
    for tombstone in left.tombstones.into_iter().chain(right.tombstones) {
        let key = (tombstone.origin.clone(), tombstone.username.clone());
        tombstones
            .entry(key)
            .and_modify(|current| {
                current.deleted_at_ms = current.deleted_at_ms.max(tombstone.deleted_at_ms)
            })
            .or_insert(tombstone);
    }
    let mut merged = PasswordsCategory {
        version: 1,
        entries: entries.into_values().collect(),
        tombstones: tombstones.into_values().collect(),
    };
    normalize_passwords(&mut merged)?;
    Ok(merged)
}

fn merge_tombstones(left: Vec<UrlTombstone>, right: Vec<UrlTombstone>) -> Vec<UrlTombstone> {
    let mut merged = BTreeMap::<String, UrlTombstone>::new();
    for tombstone in left.into_iter().chain(right) {
        merged
            .entry(tombstone.url.clone())
            .and_modify(|current| {
                current.deleted_at_ms = current.deleted_at_ms.max(tombstone.deleted_at_ms)
            })
            .or_insert(tombstone);
    }
    merged.into_values().collect()
}

fn normalize_bookmarks(category: &mut BookmarkCategory) -> Result<(), SyncPayloadError> {
    if category.items.len() > MAX_BOOKMARKS
        || category.tombstones.len() > MAX_BOOKMARK_TOMBSTONES
        || category.folders.len() > MAX_BOOKMARK_FOLDERS
        || category.placements.len() > MAX_BOOKMARKS
        || category.folder_tombstones.len() > MAX_BOOKMARK_TOMBSTONES
    {
        return Err(SyncPayloadError::InvalidData("bookmark limits exceeded"));
    }
    if category
        .items
        .iter()
        .any(|item| item.updated_at_ms > MAX_LOCAL_TIMESTAMP_MS)
        || category
            .tombstones
            .iter()
            .any(|item| item.deleted_at_ms > MAX_LOCAL_TIMESTAMP_MS)
        || category
            .placements
            .iter()
            .any(|item| item.updated_at_ms > MAX_LOCAL_TIMESTAMP_MS)
        || category
            .folder_tombstones
            .iter()
            .any(|item| item.deleted_at_ms > MAX_LOCAL_TIMESTAMP_MS)
    {
        return Err(SyncPayloadError::InvalidData(
            "bookmark timestamp exceeds local storage range",
        ));
    }
    for entry in &category.items {
        validate_page(&entry.page)?;
    }
    for tombstone in &category.tombstones {
        validate_url(&tombstone.url)?;
    }
    let folder_ids: std::collections::HashSet<_> = category
        .folders
        .iter()
        .map(|folder| folder.id.as_str())
        .collect();
    let deleted_folder_ids: std::collections::HashSet<_> = category
        .folder_tombstones
        .iter()
        .map(|folder| folder.id.as_str())
        .collect();
    if category.folder_tombstones.iter().any(|folder| {
        folder.id.is_empty() || folder.id.len() > 128 || folder.id.chars().any(char::is_control)
    }) {
        return Err(SyncPayloadError::InvalidData(
            "invalid deleted bookmark folder",
        ));
    }
    if folder_ids.len() != category.folders.len()
        || category.folders.iter().any(|folder| {
            folder.id.is_empty()
                || folder.id.len() > 128
                || folder.name.trim().is_empty()
                || folder.name.chars().count() > 100
                || folder.name.chars().any(char::is_control)
                || folder
                    .parent_id
                    .as_deref()
                    .is_some_and(|id| !folder_ids.contains(id) && !deleted_folder_ids.contains(id))
        })
    {
        return Err(SyncPayloadError::InvalidData("invalid bookmark folder"));
    }
    for folder in &category.folders {
        let mut seen = std::collections::HashSet::new();
        let mut current = Some(folder.id.as_str());
        while let Some(id) = current {
            if !seen.insert(id) {
                return Err(SyncPayloadError::InvalidData("bookmark folder cycle"));
            }
            current = category
                .folders
                .iter()
                .find(|candidate| candidate.id == id)
                .and_then(|candidate| candidate.parent_id.as_deref());
        }
        if seen.len() > 16 {
            return Err(SyncPayloadError::InvalidData(
                "bookmark folder depth exceeded",
            ));
        }
    }
    for placement in &category.placements {
        validate_url(&placement.url)?;
        if placement
            .folder_id
            .as_deref()
            .is_some_and(|id| !folder_ids.contains(id) && !deleted_folder_ids.contains(id))
        {
            return Err(SyncPayloadError::InvalidData(
                "bookmark placement has unknown folder",
            ));
        }
    }
    *category = merge_bookmarks(BookmarkCategory::default(), category.clone())?;
    let urls: std::collections::HashSet<_> = category
        .items
        .iter()
        .map(|item| item.page.url.as_str())
        .collect();
    category
        .placements
        .retain(|placement| urls.contains(placement.url.as_str()));
    Ok(())
}

fn normalize_history(category: &mut HistoryCategory) -> Result<(), SyncPayloadError> {
    if category.items.len() > MAX_HISTORY_ITEMS
        || category.tombstones.len() > MAX_HISTORY_TOMBSTONES
        || category.visit_tombstones.len() > MAX_HISTORY_TOMBSTONES
    {
        return Err(SyncPayloadError::InvalidData("history limits exceeded"));
    }
    if category
        .items
        .iter()
        .any(|item| item.visited_at_ms > MAX_LOCAL_TIMESTAMP_MS)
        || category
            .tombstones
            .iter()
            .any(|item| item.deleted_at_ms > MAX_LOCAL_TIMESTAMP_MS)
        || category.visit_tombstones.iter().any(|item| {
            item.visited_at_ms > MAX_LOCAL_TIMESTAMP_MS
                || item.deleted_at_ms > MAX_LOCAL_TIMESTAMP_MS
        })
    {
        return Err(SyncPayloadError::InvalidData(
            "history timestamp exceeds local storage range",
        ));
    }
    for entry in &category.items {
        validate_page(&entry.page)?;
    }
    for tombstone in &category.tombstones {
        validate_url(&tombstone.url)?;
    }
    for tombstone in &category.visit_tombstones {
        validate_url(&tombstone.url)?;
    }
    *category = merge_history(HistoryCategory::default(), category.clone());
    Ok(())
}

fn normalize_tabs(category: &mut OpenTabsCategory) -> Result<(), SyncPayloadError> {
    if category.devices.len() > MAX_DEVICES {
        return Err(SyncPayloadError::InvalidData("device limit exceeded"));
    }
    let mut seen = std::collections::HashSet::new();
    for device in &category.devices {
        if !valid_device_id(&device.device_id) || !seen.insert(device.device_id.clone()) {
            return Err(SyncPayloadError::InvalidData(
                "invalid or duplicate device ID",
            ));
        }
        if device.tabs.len() > MAX_OPEN_TABS_PER_DEVICE {
            return Err(SyncPayloadError::InvalidData("open-tab limit exceeded"));
        }
        for tab in &device.tabs {
            validate_url(&tab.url)?;
            validate_title(&tab.title)?;
            if tab
                .group
                .as_ref()
                .is_some_and(|name| name.chars().count() > MAX_GROUP_NAME_CHARS)
            {
                return Err(SyncPayloadError::InvalidData("tab group name too long"));
            }
        }
    }
    *category = merge_tabs(OpenTabsCategory::default(), category.clone());
    Ok(())
}

fn validate_page(page: &SyncPage) -> Result<(), SyncPayloadError> {
    validate_url(&page.url)?;
    validate_title(&page.title)
}

fn validate_title(title: &str) -> Result<(), SyncPayloadError> {
    if title.chars().count() > MAX_TITLE_CHARS || title.chars().any(char::is_control) {
        return Err(SyncPayloadError::InvalidData(
            "title is too long or contains controls",
        ));
    }
    Ok(())
}

fn validate_url(url: &str) -> Result<(), SyncPayloadError> {
    if url.len() > MAX_URL_BYTES || url.chars().any(char::is_control) {
        return Err(SyncPayloadError::InvalidData(
            "URL is too long or contains controls",
        ));
    }
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .ok_or(SyncPayloadError::InvalidData("only HTTP(S) URLs may sync"))?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.is_empty() || authority.contains('@') || authority.chars().any(char::is_whitespace)
    {
        return Err(SyncPayloadError::InvalidData(
            "URL authority is invalid or contains credentials",
        ));
    }
    Ok(())
}

fn valid_device_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_DEVICE_ID_BYTES
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(url: &str, title: &str) -> SyncPage {
        SyncPage {
            url: url.into(),
            title: title.into(),
        }
    }

    #[test]
    fn rejects_bookmark_and_history_timestamps_that_local_profiles_cannot_represent() {
        let invalid_timestamp = MAX_LOCAL_TIMESTAMP_MS + 1;
        let invalid_bookmarks = |category| {
            let mut payload = SyncPayloadV2::empty();
            payload.bookmarks = Some(category);
            payload.validate_and_normalize()
        };

        let mut category = BookmarkCategory::default();
        category.items.push(TimedBookmark {
            page: page("https://saved.example/", "Saved"),
            updated_at_ms: invalid_timestamp,
        });
        assert_eq!(
            invalid_bookmarks(category),
            Err(SyncPayloadError::InvalidData(
                "bookmark timestamp exceeds local storage range"
            ))
        );

        let mut category = BookmarkCategory::default();
        category.tombstones.push(UrlTombstone {
            url: "https://removed.example/".into(),
            deleted_at_ms: invalid_timestamp,
        });
        assert_eq!(
            invalid_bookmarks(category),
            Err(SyncPayloadError::InvalidData(
                "bookmark timestamp exceeds local storage range"
            ))
        );

        let mut payload = SyncPayloadV2::empty();
        payload.history = Some(HistoryCategory {
            items: vec![HistoryEntry {
                page: page("https://visited.example/", "Visited"),
                visited_at_ms: invalid_timestamp,
            }],
            ..Default::default()
        });
        assert_eq!(
            payload.validate_and_normalize(),
            Err(SyncPayloadError::InvalidData(
                "history timestamp exceeds local storage range"
            ))
        );
    }

    #[test]
    fn migrates_bookmark_only_v1_and_keeps_new_categories_absent() {
        let payload = SyncPayloadV2::decode_and_migrate(
            br#"{"version":1,"bookmarks":[{"url":"https://a.example/","title":"A"}],"removed_bookmarks":["https://b.example/"]}"#,
        )
        .unwrap();
        assert_eq!(payload.schema_version, INNER_SCHEMA_VERSION);
        assert_eq!(payload.history, None);
        assert_eq!(payload.open_tabs, None);
        let bookmarks = payload.bookmarks.unwrap();
        assert_eq!(bookmarks.items[0].updated_at_ms, 0);
        assert_eq!(bookmarks.tombstones[0].deleted_at_ms, 0);
    }

    #[test]
    fn migrates_v2_second_timestamps_to_v3_milliseconds() {
        let payload = SyncPayloadV2::decode_and_migrate(
            br#"{"schema_version":2,"bookmarks":{"items":[{"page":{"url":"https://a.example/","title":"A"},"updated_at_secs":1700000000}],"tombstones":[{"url":"https://b.example/","deleted_at_secs":1700000001}]},"history":{"items":[{"page":{"url":"https://c.example/","title":"C"},"visited_at_secs":1700000002}],"tombstones":[]},"open_tabs":{"devices":[{"device_id":"old-device","updated_at_secs":1700000003,"tabs":[]}]},"settings":{"updated_at_secs":1700000004,"search_engine":"google","theme":"dark","density":"comfortable","default_zoom_percent":100,"show_status_preview":true,"show_zoom_badge":true,"enable_tab_reordering":true,"enable_tab_tearoff":false,"warn_on_close_tabs":true}}"#,
        )
        .unwrap();
        assert_eq!(payload.schema_version, INNER_SCHEMA_VERSION);
        assert_eq!(
            payload.bookmarks.unwrap().items[0].updated_at_ms,
            1_700_000_000_000
        );
        assert_eq!(
            payload.history.unwrap().items[0].visited_at_ms,
            1_700_000_002_000
        );
        assert_eq!(
            payload.open_tabs.as_ref().unwrap().devices[0].updated_at_ms,
            1_700_000_003_000
        );
        assert_eq!(
            payload.settings.as_ref().unwrap().updated_at_ms,
            1_700_000_004_000
        );
        assert!(payload.passwords.is_none());
    }

    #[test]
    fn migrates_v3_without_passwords_and_preserves_millisecond_metadata() {
        let payload = SyncPayloadV2::decode_and_migrate(
            br#"{"schema_version":3,"bookmarks":{"items":[{"page":{"url":"https://a.example/","title":"A"},"updated_at_ms":1700000000123}],"tombstones":[]},"history":null,"open_tabs":{"devices":[{"device_id":"desktop","updated_at_ms":1700000000456,"tabs":[]}]},"settings":{"updated_at_ms":1700000000789,"search_engine":"google","theme":"dark","density":"comfortable","default_zoom_percent":100,"show_status_preview":true,"show_zoom_badge":true,"enable_tab_reordering":true,"enable_tab_tearoff":false,"warn_on_close_tabs":true}}"#,
        )
        .unwrap();
        assert_eq!(payload.schema_version, INNER_SCHEMA_VERSION);
        assert!(payload.passwords.is_none());
        assert_eq!(
            payload.bookmarks.unwrap().items[0].updated_at_ms,
            1_700_000_000_123
        );
        assert_eq!(
            payload.open_tabs.unwrap().devices[0].updated_at_ms,
            1_700_000_000_456
        );
        assert_eq!(payload.settings.unwrap().updated_at_ms, 1_700_000_000_789);
    }

    #[test]
    fn migrates_folderless_v4_bookmarks_and_rejects_silent_folder_loss_versions() {
        let old_v4 = br#"{"schema_version":4,"bookmarks":{"items":[{"page":{"url":"https://old.example/","title":"Old"},"updated_at_ms":1234}],"tombstones":[]},"history":null,"open_tabs":null,"settings":null,"passwords":null}"#;
        let migrated = SyncPayloadV2::decode_and_migrate(old_v4).unwrap();
        assert_eq!(migrated.schema_version, INNER_SCHEMA_VERSION);
        let bookmarks = migrated.bookmarks.unwrap();
        assert_eq!(bookmarks.items.len(), 1);
        assert!(bookmarks.folders.is_empty());
        assert!(bookmarks.placements.is_empty());

        // This V6 reader rejects future schemas rather than ignoring fields it
        // cannot represent and silently overwriting after downloading the snapshot.
        assert_eq!(
            SyncPayloadV2::decode_and_migrate(br#"{"schema_version":7}"#),
            Err(SyncPayloadError::UnsupportedVersion(7))
        );
    }

    #[test]
    fn refuses_unknown_versions_instead_of_dropping_newer_fields() {
        assert_eq!(
            SyncPayloadV2::decode_and_migrate(br#"{"schema_version":7}"#),
            Err(SyncPayloadError::UnsupportedVersion(7))
        );
        assert_eq!(
            SyncPayloadV2::decode_and_migrate(
                br#"{"schema_version":4,"history":{"items":[],"tombstones":[],"future_field":true}}"#
            ),
            Err(SyncPayloadError::InvalidJson)
        );
    }

    #[test]
    fn bookmark_merge_is_deterministic_and_deletions_win_timestamp_ties() {
        let left = BookmarkCategory {
            items: vec![TimedBookmark {
                page: page("https://gone.example/", "Old"),
                updated_at_ms: 10,
            }],
            tombstones: vec![],
            folders: vec![],
            placements: vec![],
            folder_tombstones: vec![],
        };
        let right = BookmarkCategory {
            items: vec![TimedBookmark {
                page: page("https://keep.example/", "Keep"),
                updated_at_ms: 7,
            }],
            tombstones: vec![UrlTombstone {
                url: "https://gone.example/".into(),
                deleted_at_ms: 10,
            }],
            folders: vec![],
            placements: vec![],
            folder_tombstones: vec![],
        };
        let merged = merge_bookmarks(left, right).unwrap();
        assert_eq!(merged.items.len(), 1);
        assert_eq!(merged.items[0].page.url, "https://keep.example/");
        assert_eq!(merged.tombstones.len(), 1);
    }

    #[test]
    fn same_bookmark_conflicts_use_newest_timestamp_and_stable_title_tie_break() {
        let bookmark = |title: &str, updated_at_ms| BookmarkCategory {
            items: vec![TimedBookmark {
                page: page("https://same.example/", title),
                updated_at_ms,
            }],
            tombstones: vec![],
            folders: vec![],
            placements: vec![],
            folder_tombstones: vec![],
        };

        let older = bookmark("Device A title", 100);
        let newer = bookmark("Device B title", 101);
        let merged = merge_bookmarks(older.clone(), newer.clone()).unwrap();
        let reversed = merge_bookmarks(newer, older).unwrap();
        assert_eq!(merged, reversed);
        assert_eq!(merged.items[0].page.title, "Device B title");
        assert_eq!(merged.items[0].updated_at_ms, 101);

        // Concurrent edits at the same millisecond have no causal ordering.
        // Keep the existing deterministic lexicographic title tie-break so
        // devices converge regardless of which payload is the left operand.
        let left = bookmark("Alpha title", 200);
        let right = bookmark("Zulu title", 200);
        let merged = merge_bookmarks(left.clone(), right.clone()).unwrap();
        assert_eq!(merged, merge_bookmarks(right, left).unwrap());
        assert_eq!(merged.items[0].page.title, "Zulu title");
    }

    #[test]
    fn bookmark_delete_wins_equal_timestamp_but_a_later_readd_survives() {
        let deleted = BookmarkCategory {
            items: vec![],
            tombstones: vec![UrlTombstone {
                url: "https://same.example/".into(),
                deleted_at_ms: 300,
            }],
            folders: vec![],
            placements: vec![],
            folder_tombstones: vec![],
        };
        let concurrent_add = BookmarkCategory {
            items: vec![TimedBookmark {
                page: page("https://same.example/", "Concurrent add"),
                updated_at_ms: 300,
            }],
            tombstones: vec![],
            folders: vec![],
            placements: vec![],
            folder_tombstones: vec![],
        };
        let tied = merge_bookmarks(concurrent_add, deleted.clone()).unwrap();
        assert!(tied.items.is_empty());
        assert_eq!(tied.tombstones[0].deleted_at_ms, 300);
        let reversed_tie = merge_bookmarks(
            BookmarkCategory {
                items: vec![],
                tombstones: vec![UrlTombstone {
                    url: "https://same.example/".into(),
                    deleted_at_ms: 300,
                }],
                folders: vec![],
                placements: vec![],
                folder_tombstones: vec![],
            },
            BookmarkCategory {
                items: vec![TimedBookmark {
                    page: page("https://same.example/", "Concurrent add"),
                    updated_at_ms: 300,
                }],
                tombstones: vec![],
                folders: vec![],
                placements: vec![],
                folder_tombstones: vec![],
            },
        )
        .unwrap();
        assert_eq!(tied, reversed_tie);

        let later_readd = BookmarkCategory {
            items: vec![TimedBookmark {
                page: page("https://same.example/", "Re-added later"),
                updated_at_ms: 301,
            }],
            tombstones: vec![],
            folders: vec![],
            placements: vec![],
            folder_tombstones: vec![],
        };
        let readded = merge_bookmarks(deleted.clone(), later_readd.clone()).unwrap();
        assert_eq!(readded.items.len(), 1);
        assert_eq!(readded.items[0].page.title, "Re-added later");
        assert_eq!(readded, merge_bookmarks(later_readd, deleted).unwrap());
    }

    #[test]
    fn folder_deletion_tombstone_removes_stale_descendants_and_placements() {
        let live = BookmarkCategory {
            items: vec![TimedBookmark {
                page: page("https://saved.example/", "Saved"),
                updated_at_ms: 10,
            }],
            tombstones: vec![],
            folders: vec![
                SyncBookmarkFolder {
                    id: "parent".into(),
                    name: "Parent".into(),
                    parent_id: None,
                },
                SyncBookmarkFolder {
                    id: "child".into(),
                    name: "Child".into(),
                    parent_id: Some("parent".into()),
                },
            ],
            placements: vec![BookmarkPlacement {
                url: "https://saved.example/".into(),
                folder_id: Some("child".into()),
                updated_at_ms: 10,
            }],
            folder_tombstones: vec![],
        };
        let deleted = BookmarkCategory {
            items: vec![],
            tombstones: vec![],
            folders: vec![],
            placements: vec![],
            folder_tombstones: vec![BookmarkFolderTombstone {
                id: "parent".into(),
                deleted_at_ms: 11,
            }],
        };
        let merged = merge_bookmarks(live, deleted).unwrap();
        assert!(merged.folders.is_empty());
        assert_eq!(merged.folder_tombstones.len(), 1);
        assert_eq!(merged.placements[0].folder_id, None);
    }

    #[test]
    fn bookmark_folder_cycles_and_unknown_parents_are_rejected() {
        for folders in [
            vec![
                SyncBookmarkFolder {
                    id: "a".into(),
                    name: "A".into(),
                    parent_id: Some("b".into()),
                },
                SyncBookmarkFolder {
                    id: "b".into(),
                    name: "B".into(),
                    parent_id: Some("a".into()),
                },
            ],
            vec![SyncBookmarkFolder {
                id: "a".into(),
                name: "A".into(),
                parent_id: Some("missing".into()),
            }],
        ] {
            let mut payload = SyncPayloadV2::empty();
            payload.bookmarks = Some(BookmarkCategory {
                items: vec![],
                tombstones: vec![],
                folders,
                placements: vec![],
                folder_tombstones: vec![],
            });
            assert!(payload.validate_and_normalize().is_err());
        }
    }

    #[test]
    fn merging_over_capacity_fails_instead_of_silently_dropping_sync_data() {
        let mut left = SyncPayloadV2::empty();
        let mut right = SyncPayloadV2::empty();
        left.bookmarks = Some(BookmarkCategory {
            items: (0..MAX_BOOKMARKS)
                .map(|index| TimedBookmark {
                    page: page(&format!("https://left-{index}.example/"), "Left"),
                    updated_at_ms: index as u64,
                })
                .collect(),
            tombstones: vec![],
            folders: vec![],
            placements: vec![],
            folder_tombstones: vec![],
        });
        right.bookmarks = Some(BookmarkCategory {
            items: (0..MAX_BOOKMARKS)
                .map(|index| TimedBookmark {
                    page: page(&format!("https://right-{index}.example/"), "Right"),
                    updated_at_ms: index as u64,
                })
                .collect(),
            tombstones: vec![],
            folders: vec![],
            placements: vec![],
            folder_tombstones: vec![],
        });
        assert!(left.merge(right).is_err());

        let mut left = SyncPayloadV2::empty();
        let mut right = SyncPayloadV2::empty();
        left.history = Some(HistoryCategory {
            items: (0..MAX_HISTORY_ITEMS)
                .map(|index| HistoryEntry {
                    page: page(&format!("https://left-{index}.example/"), "Left"),
                    visited_at_ms: index as u64,
                })
                .collect(),
            tombstones: vec![],
            visit_tombstones: vec![],
        });
        right.history = Some(HistoryCategory {
            items: (0..MAX_HISTORY_ITEMS)
                .map(|index| HistoryEntry {
                    page: page(&format!("https://right-{index}.example/"), "Right"),
                    visited_at_ms: index as u64,
                })
                .collect(),
            tombstones: vec![],
            visit_tombstones: vec![],
        });
        assert!(left.merge(right).is_err());

        let mut left = SyncPayloadV2::empty();
        let mut right = SyncPayloadV2::empty();
        left.bookmarks = Some(BookmarkCategory {
            items: vec![],
            tombstones: (0..MAX_BOOKMARK_TOMBSTONES)
                .map(|index| UrlTombstone {
                    url: format!("https://left-{index}.example/"),
                    deleted_at_ms: index as u64,
                })
                .collect(),
            folders: vec![],
            placements: vec![],
            folder_tombstones: vec![],
        });
        right.bookmarks = Some(BookmarkCategory {
            items: vec![],
            tombstones: (0..MAX_BOOKMARK_TOMBSTONES)
                .map(|index| UrlTombstone {
                    url: format!("https://right-{index}.example/"),
                    deleted_at_ms: index as u64,
                })
                .collect(),
            folders: vec![],
            placements: vec![],
            folder_tombstones: vec![],
        });
        assert!(left.merge(right).is_err());

        let mut left = SyncPayloadV2::empty();
        let mut right = SyncPayloadV2::empty();
        left.history = Some(HistoryCategory {
            items: vec![],
            tombstones: (0..MAX_HISTORY_TOMBSTONES)
                .map(|index| UrlTombstone {
                    url: format!("https://left-{index}.example/"),
                    deleted_at_ms: index as u64,
                })
                .collect(),
            visit_tombstones: vec![],
        });
        right.history = Some(HistoryCategory {
            items: vec![],
            tombstones: (0..MAX_HISTORY_TOMBSTONES)
                .map(|index| UrlTombstone {
                    url: format!("https://right-{index}.example/"),
                    deleted_at_ms: index as u64,
                })
                .collect(),
            visit_tombstones: vec![],
        });
        assert!(left.merge(right).is_err());

        let mut left = SyncPayloadV2::empty();
        let mut right = SyncPayloadV2::empty();
        left.open_tabs = Some(OpenTabsCategory {
            devices: (0..MAX_DEVICES)
                .map(|index| DeviceTabs {
                    device_id: format!("left-{index}"),
                    updated_at_ms: index as u64,
                    tabs: vec![],
                })
                .collect(),
        });
        right.open_tabs = Some(OpenTabsCategory {
            devices: (0..MAX_DEVICES)
                .map(|index| DeviceTabs {
                    device_id: format!("right-{index}"),
                    updated_at_ms: index as u64,
                    tabs: vec![],
                })
                .collect(),
        });
        assert!(left.merge(right).is_err());
    }

    #[test]
    fn history_deletion_suppresses_old_visit_but_new_visit_restores_it() {
        let old = HistoryCategory {
            items: vec![HistoryEntry {
                page: page("https://visited.example/", "Visited"),
                visited_at_ms: 10,
            }],
            tombstones: vec![],
            visit_tombstones: vec![],
        };
        let deleted = HistoryCategory {
            items: vec![],
            tombstones: vec![UrlTombstone {
                url: "https://visited.example/".into(),
                deleted_at_ms: 10,
            }],
            visit_tombstones: vec![],
        };
        assert!(merge_history(old.clone(), deleted.clone()).items.is_empty());
        let mut new_visit = old;
        new_visit.items[0].visited_at_ms = 11;
        assert_eq!(merge_history(new_visit, deleted).items.len(), 1);
    }

    #[test]
    fn history_merge_keeps_repeated_visits_to_the_same_url() {
        let first = HistoryCategory {
            items: vec![
                HistoryEntry {
                    page: page("https://visited.example/", "First"),
                    visited_at_ms: 100,
                },
                HistoryEntry {
                    page: page("https://visited.example/", "Second"),
                    visited_at_ms: 200,
                },
            ],
            tombstones: vec![],
            visit_tombstones: vec![],
        };
        let second = HistoryCategory {
            items: vec![HistoryEntry {
                page: page("https://visited.example/", "First"),
                visited_at_ms: 100,
            }],
            tombstones: vec![],
            visit_tombstones: vec![],
        };
        let merged = merge_history(first, second);
        assert_eq!(merged.items.len(), 2);
        assert_eq!(merged.items[0].visited_at_ms, 200);
        assert_eq!(merged.items[1].visited_at_ms, 100);
    }

    #[test]
    fn event_tombstone_removes_one_visit_and_preserves_newer_revisit() {
        let url = "https://visited.example/";
        let category = HistoryCategory {
            items: vec![
                HistoryEntry {
                    page: page(url, "Earlier"),
                    visited_at_ms: 100,
                },
                HistoryEntry {
                    page: page(url, "Deleted visit"),
                    visited_at_ms: 200,
                },
            ],
            tombstones: vec![],
            visit_tombstones: vec![HistoryVisitTombstone {
                url: url.into(),
                visited_at_ms: 200,
                deleted_at_ms: 300,
            }],
        };
        let merged = merge_history(category, HistoryCategory::default());
        assert_eq!(merged.items.len(), 1);
        assert_eq!(merged.items[0].visited_at_ms, 100);
        assert_eq!(merged.visit_tombstones.len(), 1);

        let revisit = HistoryCategory {
            items: vec![HistoryEntry {
                page: page(url, "New revisit"),
                visited_at_ms: 400,
            }],
            tombstones: vec![],
            visit_tombstones: vec![],
        };
        let merged = merge_history(merged, revisit);
        assert_eq!(merged.items.len(), 2);
        assert!(merged.items.iter().any(|entry| entry.visited_at_ms == 400));
        assert!(!merged.items.iter().any(|entry| entry.visited_at_ms == 200));
    }

    #[test]
    fn v5_history_payload_migrates_without_dropping_legacy_visits() {
        let mut old = SyncPayloadV2::empty();
        old.schema_version = 5;
        old.history = Some(HistoryCategory {
            items: vec![HistoryEntry {
                page: page("https://legacy.example/", "Legacy"),
                visited_at_ms: 1234,
            }],
            tombstones: vec![],
            visit_tombstones: vec![],
        });
        let mut old_json = serde_json::to_value(&old).unwrap();
        old_json["history"]
            .as_object_mut()
            .unwrap()
            .remove("visit_tombstones");
        let bytes = serde_json::to_vec(&old_json).unwrap();
        let migrated = SyncPayloadV2::decode_and_migrate(&bytes).unwrap();
        assert_eq!(migrated.schema_version, INNER_SCHEMA_VERSION);
        let items = &migrated.history.unwrap().items;
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].visited_at_ms, 1234);
        assert_eq!(items[0].page.title, "Legacy");
    }

    #[test]
    fn v4_url_history_and_tombstone_migrate_to_v6_clear_all_semantics() {
        let old = br#"{"schema_version":4,"bookmarks":{"items":[],"tombstones":[]},"history":{"items":[{"page":{"url":"https://legacy.example/","title":"Legacy"},"visited_at_ms":1234}],"tombstones":[{"url":"https://cleared.example/","deleted_at_ms":2345}]},"open_tabs":null,"settings":null,"passwords":null}"#;
        let migrated = SyncPayloadV2::decode_and_migrate(old).unwrap();
        assert_eq!(migrated.schema_version, INNER_SCHEMA_VERSION);
        let history = migrated.history.unwrap();
        assert_eq!(history.items.len(), 1);
        assert_eq!(history.items[0].visited_at_ms, 1234);
        assert_eq!(history.visit_tombstones.len(), 0);
        assert_eq!(history.tombstones[0].deleted_at_ms, 2345);
    }

    #[test]
    fn millisecond_timestamps_keep_same_second_revisits_after_deletion() {
        let deleted = HistoryCategory {
            items: vec![],
            tombstones: vec![UrlTombstone {
                url: "https://visited.example/".into(),
                deleted_at_ms: 1_700_000_000_456,
            }],
            visit_tombstones: vec![],
        };
        let revisit = HistoryCategory {
            items: vec![HistoryEntry {
                page: page("https://visited.example/", "Visited again"),
                visited_at_ms: 1_700_000_000_789,
            }],
            tombstones: vec![],
            visit_tombstones: vec![],
        };
        let merged = merge_history(deleted, revisit);
        assert_eq!(merged.items.len(), 1);
    }

    #[test]
    fn tabs_merge_by_device_and_keep_each_devices_latest_snapshot() {
        let first = OpenTabsCategory {
            devices: vec![DeviceTabs {
                device_id: "desktop_a".into(),
                updated_at_ms: 1_700_000_000_100,
                tabs: vec![OpenTab {
                    url: "https://old.example/".into(),
                    title: "Old".into(),
                    pinned: false,
                    group: None,
                }],
            }],
        };
        let second = OpenTabsCategory {
            devices: vec![
                DeviceTabs {
                    device_id: "desktop_a".into(),
                    updated_at_ms: 1_700_000_000_900,
                    tabs: vec![],
                },
                DeviceTabs {
                    device_id: "laptop_b".into(),
                    updated_at_ms: 1,
                    tabs: vec![],
                },
            ],
        };
        let merged = merge_tabs(first, second);
        assert_eq!(merged.devices.len(), 2);
        assert!(merged
            .devices
            .iter()
            .find(|d| d.device_id == "desktop_a")
            .unwrap()
            .tabs
            .is_empty());
        assert!(merged.devices.iter().any(|d| d.device_id == "laptop_b"));
    }

    #[test]
    fn settings_merge_uses_timestamp_and_is_allowlisted_by_type() {
        let settings = |time, theme| SyncableSettings {
            updated_at_ms: 1_700_000_000_000 + time as u64,
            search_engine: SearchEngine::Google,
            theme,
            density: UiDensity::Comfortable,
            default_zoom_percent: 100,
            show_status_preview: true,
            show_zoom_badge: true,
            enable_tab_reordering: true,
            enable_tab_tearoff: false,
            warn_on_close_tabs: true,
        };
        let merged = merge_settings(
            Some(settings(1, Theme::Light)),
            Some(settings(2, Theme::Dark)),
        )
        .unwrap();
        assert_eq!(merged.theme, Theme::Dark);
        assert_eq!(merged.updated_at_ms, 1_700_000_000_002);
    }

    #[test]
    fn validation_rejects_limits_non_http_urls_and_url_credentials() {
        let mut payload = SyncPayloadV2::empty();
        payload.bookmarks = Some(BookmarkCategory {
            items: vec![TimedBookmark {
                page: page("file:///C:/secret", "local file"),
                updated_at_ms: 1,
            }],
            tombstones: vec![],
            folders: vec![],
            placements: vec![],
            folder_tombstones: vec![],
        });
        assert!(payload.validate_and_normalize().is_err());

        let mut payload = SyncPayloadV2::empty();
        payload.open_tabs = Some(OpenTabsCategory {
            devices: vec![DeviceTabs {
                device_id: "desktop".into(),
                updated_at_ms: 1,
                tabs: vec![OpenTab {
                    url: "https://user:pass@example.com/".into(),
                    title: "Secret URL credentials".into(),
                    pinned: false,
                    group: None,
                }],
            }],
        });
        assert!(payload.validate_and_normalize().is_err());
    }

    #[test]
    fn roundtrip_contains_only_explicit_sync_categories() {
        let mut payload = SyncPayloadV2::empty();
        payload.settings = Some(SyncableSettings {
            updated_at_ms: 4,
            search_engine: SearchEngine::Google,
            theme: Theme::System,
            density: UiDensity::Comfortable,
            default_zoom_percent: 100,
            show_status_preview: true,
            show_zoom_badge: true,
            enable_tab_reordering: true,
            enable_tab_tearoff: false,
            warn_on_close_tabs: true,
        });
        let bytes = payload.encode().unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(!text.contains("password"));
        assert!(!text.contains("cookie"));
        assert!(!text.contains("profile"));
        assert_eq!(SyncPayloadV2::decode_and_migrate(&bytes).unwrap(), payload);
    }

    fn password_entry(username: &str, password: &str, updated_at_ms: u64) -> SyncPasswordEntry {
        SyncPasswordEntry {
            origin: "https://example.com".into(),
            username: username.into(),
            password: password.into(),
            updated_at_ms,
        }
    }

    #[test]
    fn password_category_is_optional_versioned_and_roundtrips() {
        let mut payload = SyncPayloadV2::empty();
        assert!(payload
            .encode()
            .unwrap()
            .windows(10)
            .all(|part| part != b"passwords:"));
        payload.passwords = Some(PasswordsCategory {
            version: 1,
            entries: vec![password_entry(
                "giang@example.com",
                "secret",
                1_700_000_000_123,
            )],
            tombstones: vec![],
        });
        let encoded = payload.encode().unwrap();
        let decoded = SyncPayloadV2::decode_and_migrate(&encoded).unwrap();
        assert_eq!(decoded, payload);
        assert!(String::from_utf8(encoded)
            .unwrap()
            .contains("giang@example.com"));
    }

    #[test]
    fn password_merge_uses_milliseconds_and_keeps_absent_categories() {
        let local = PasswordsCategory {
            version: 1,
            entries: vec![password_entry("user", "old", 1_700_000_000_100)],
            tombstones: vec![],
        };
        let remote = PasswordsCategory {
            version: 1,
            entries: vec![password_entry("user", "new", 1_700_000_000_900)],
            tombstones: vec![],
        };
        let mut local_payload = SyncPayloadV2::empty();
        local_payload.passwords = Some(local.clone());
        let absent = SyncPayloadV2::empty();
        assert_eq!(
            local_payload.clone().merge(absent).unwrap().passwords,
            Some(local)
        );
        let mut remote_payload = SyncPayloadV2::empty();
        remote_payload.passwords = Some(remote);
        let merged = local_payload.merge(remote_payload).unwrap();
        let category = merged.passwords.unwrap();
        assert_eq!(category.entries.len(), 1);
        assert_eq!(category.entries[0].password, "new");
    }

    #[test]
    fn conflicting_password_updates_at_the_same_timestamp_fail_closed() {
        let timestamp = 1_700_000_000_500;
        let mut first = SyncPayloadV2::empty();
        first.passwords = Some(PasswordsCategory {
            version: 1,
            entries: vec![password_entry("user", "device-a-value", timestamp)],
            tombstones: vec![],
        });
        let mut second = SyncPayloadV2::empty();
        second.passwords = Some(PasswordsCategory {
            version: 1,
            entries: vec![password_entry("user", "device-b-value", timestamp)],
            tombstones: vec![],
        });

        // With identical identity and timestamp there is no safe winner. The
        // merge must reject rather than silently overwrite either password.
        assert_eq!(
            first.clone().merge(second.clone()).unwrap_err(),
            SyncPayloadError::InvalidData("conflicting password updates share a timestamp")
        );
        assert_eq!(
            second.merge(first).unwrap_err(),
            SyncPayloadError::InvalidData("conflicting password updates share a timestamp")
        );

        let mut same_value = SyncPayloadV2::empty();
        same_value.passwords = Some(PasswordsCategory {
            version: 1,
            entries: vec![password_entry("user", "same-value", timestamp)],
            tombstones: vec![],
        });
        let mut duplicate = SyncPayloadV2::empty();
        duplicate.passwords = Some(PasswordsCategory {
            version: 1,
            entries: vec![password_entry("user", "same-value", timestamp)],
            tombstones: vec![],
        });
        let merged = same_value.merge(duplicate).unwrap();
        let passwords = merged.passwords.unwrap();
        assert_eq!(passwords.entries.len(), 1);
        assert_eq!(passwords.entries[0].password, "same-value");
    }

    #[test]
    fn password_deletion_wins_timestamp_ties_and_newer_readd_survives() {
        let entry = password_entry("user", "secret", 1_700_000_000_100);
        let deleted = PasswordsCategory {
            version: 1,
            entries: vec![],
            tombstones: vec![SyncPasswordTombstone {
                origin: entry.origin.clone(),
                username: entry.username.clone(),
                deleted_at_ms: entry.updated_at_ms,
            }],
        };
        let mut payload = SyncPayloadV2::empty();
        payload.passwords = Some(PasswordsCategory {
            version: 1,
            entries: vec![entry.clone()],
            tombstones: vec![],
        });
        let mut deletion_payload = SyncPayloadV2::empty();
        deletion_payload.passwords = Some(deleted.clone());
        assert!(payload
            .clone()
            .merge(deletion_payload.clone())
            .unwrap()
            .passwords
            .unwrap()
            .entries
            .is_empty());
        assert!(deletion_payload
            .clone()
            .merge(payload.clone())
            .unwrap()
            .passwords
            .unwrap()
            .entries
            .is_empty());

        let mut readd = entry;
        readd.updated_at_ms += 1;
        let mut readd_payload = SyncPayloadV2::empty();
        readd_payload.passwords = Some(PasswordsCategory {
            version: 1,
            entries: vec![readd],
            tombstones: vec![],
        });
        for merged in [
            deletion_payload
                .clone()
                .merge(readd_payload.clone())
                .unwrap(),
            readd_payload.merge(deletion_payload).unwrap(),
        ] {
            let passwords = merged.passwords.unwrap();
            assert_eq!(passwords.entries.len(), 1);
            assert_eq!(passwords.entries[0].password, "secret");
        }
    }

    #[test]
    fn password_category_rejects_bad_identity_duplicates_and_oversize() {
        let mut payload = SyncPayloadV2::empty();
        payload.passwords = Some(PasswordsCategory {
            version: 2,
            entries: vec![],
            tombstones: vec![],
        });
        assert!(payload.validate_and_normalize().is_err());

        let mut payload = SyncPayloadV2::empty();
        payload.passwords = Some(PasswordsCategory {
            version: 1,
            entries: vec![
                password_entry("user", "secret", 1),
                password_entry("user", "other", 2),
            ],
            tombstones: vec![],
        });
        assert!(payload.validate_and_normalize().is_err());

        let mut payload = SyncPayloadV2::empty();
        let long_password = "x".repeat(MAX_PASSWORD_BYTES);
        payload.passwords = Some(PasswordsCategory {
            version: 1,
            entries: (0..120)
                .map(|index| password_entry(&format!("user-{index}"), &long_password, 1))
                .collect(),
            tombstones: vec![],
        });
        assert!(payload.validate_and_normalize().is_err());
    }

    #[test]
    fn refuses_inner_payloads_that_cannot_fit_the_v1_worker_envelope() {
        let oversized = vec![b' '; MAX_INNER_JSON_BYTES + 1];
        assert_eq!(
            SyncPayloadV2::decode_and_migrate(&oversized),
            Err(SyncPayloadError::PayloadTooLarge)
        );
    }
}
