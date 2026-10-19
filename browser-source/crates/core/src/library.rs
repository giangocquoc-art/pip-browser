//! Small, bounded local library. The host never records private-window visits.
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{io, path::Path};

const MAX_BOOKMARKS: usize = 200;
const MAX_BOOKMARK_TOMBSTONES: usize = 2000;
const MAX_HISTORY_ITEMS: usize = 500;
const MAX_HISTORY_TOMBSTONES: usize = 2000;
const MAX_BOOKMARK_IMPORT_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_BOOKMARK_FOLDERS: usize = 200;
const MAX_HOME_SHORTCUTS: usize = 12;
static NEXT_BOOKMARK_FOLDER_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Library {
    pub bookmarks: Vec<Page>,
    /// Per-URL bookmark update times used for cross-device conflict resolution.
    #[serde(default)]
    pub bookmark_updates: Vec<BookmarkUpdate>,
    /// Timestamped deletions used to keep removed bookmarks from reappearing on sync.
    #[serde(default)]
    pub bookmark_tombstones: Vec<BookmarkTombstone>,
    /// Folder tree and URL placement. Both fields were absent in flat-bookmark profiles.
    #[serde(default)]
    pub bookmark_folders: Vec<BookmarkFolder>,
    #[serde(default)]
    pub bookmark_folder_assignments: Vec<BookmarkFolderAssignment>,
    #[serde(default)]
    pub bookmark_folder_tombstones: Vec<BookmarkFolderTombstone>,
    pub history: Vec<Page>,
    /// Per-URL visit times used when merging history across devices.
    #[serde(default)]
    pub history_visits: Vec<HistoryVisit>,
    /// URLs removed from history; retained so sync can propagate deletions.
    #[serde(default)]
    pub removed_history: Vec<HistoryTombstone>,
    /// URLs removed while sync is enabled; bounded so deletions propagate without unbounded growth.
    pub removed_bookmarks: Vec<String>,
    /// User-managed start-page shortcuts. These stay local to the profile.
    #[serde(default)]
    pub home_shortcuts: Vec<Page>,
    /// Distinguishes a user-cleared shortcut list from an old library without this setting.
    #[serde(default)]
    pub home_shortcuts_configured: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page {
    pub url: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryVisit {
    pub url: String,
    /// Page title at the time of the visit. Older profiles omit this field.
    #[serde(default)]
    pub title: String,
    /// Unix time in milliseconds.
    pub visited_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookmarkUpdate {
    pub url: String,
    /// Unix time in milliseconds.
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookmarkTombstone {
    pub url: String,
    /// Unix time in milliseconds.
    pub deleted_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookmarkFolder {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BookmarkImportResult {
    pub bookmarks: usize,
    pub folders: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookmarkFolderAssignment {
    pub url: String,
    pub folder_id: Option<String>,
    #[serde(default)]
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookmarkFolderTombstone {
    pub id: String,
    pub deleted_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryTombstone {
    pub url: String,
    /// Unix time in milliseconds.
    pub removed_at_ms: i64,
    /// When present, removes only this visit event. Missing means clear all
    /// visits to the URL at or before `removed_at_ms` (legacy behavior).
    #[serde(default)]
    pub visit_at_ms: Option<i64>,
}

impl Library {
    pub fn load(path: &Path) -> io::Result<Self> {
        match std::fs::read(path) {
            Ok(bytes) if bytes.len() <= 8 * 1024 * 1024 => {
                let mut library: Self = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
                // Older versions could persist HTTP(S) URLs containing userinfo.
                // Remove those records from every local collection and rewrite
                // the file so the credentials are no longer retained at rest.
                let migrated_history = library.migrate_history_events();
                if library.remove_credentialed_urls() || migrated_history {
                    library.save(path)?;
                }
                Ok(library)
            }
            Ok(_) => Err(io::Error::other("Thư viện vượt giới hạn dung lượng")),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error),
        }
    }
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let mut sanitized = self.clone();
        sanitized.remove_credentialed_urls();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let staging = path.with_extension("json.tmp");
        std::fs::write(
            &staging,
            serde_json::to_vec(&sanitized).map_err(io::Error::other)?,
        )?;
        std::fs::rename(staging, path)
    }

    fn remove_credentialed_urls(&mut self) -> bool {
        let mut changed = false;
        let previous_len = self.bookmarks.len();
        self.bookmarks.retain(|page| !url_has_userinfo(&page.url));
        changed |= self.bookmarks.len() != previous_len;

        let previous_len = self.bookmark_updates.len();
        self.bookmark_updates
            .retain(|item| !url_has_userinfo(&item.url));
        changed |= self.bookmark_updates.len() != previous_len;

        let previous_len = self.bookmark_tombstones.len();
        self.bookmark_tombstones
            .retain(|item| !url_has_userinfo(&item.url));
        changed |= self.bookmark_tombstones.len() != previous_len;

        let previous_len = self.bookmark_folder_assignments.len();
        self.bookmark_folder_assignments
            .retain(|item| !url_has_userinfo(&item.url));
        changed |= self.bookmark_folder_assignments.len() != previous_len;

        let previous_len = self.history.len();
        self.history.retain(|page| !url_has_userinfo(&page.url));
        changed |= self.history.len() != previous_len;

        let previous_len = self.history_visits.len();
        self.history_visits
            .retain(|item| !url_has_userinfo(&item.url));
        changed |= self.history_visits.len() != previous_len;

        let previous_len = self.removed_history.len();
        self.removed_history
            .retain(|item| !url_has_userinfo(&item.url));
        changed |= self.removed_history.len() != previous_len;

        let previous_len = self.removed_bookmarks.len();
        self.removed_bookmarks.retain(|url| !url_has_userinfo(url));
        changed |= self.removed_bookmarks.len() != previous_len;

        let previous_len = self.home_shortcuts.len();
        self.home_shortcuts
            .retain(|page| !url_has_userinfo(&page.url));
        changed |= self.home_shortcuts.len() != previous_len;
        changed
    }

    fn page(url: &str, title: &str) -> Option<Page> {
        if !(url.starts_with("https://") || url.starts_with("http://"))
            || url.len() > 8192
            || url_has_userinfo(url)
            || url
                .chars()
                .any(|ch| ch.is_control() || matches!(ch, '"' | '<' | '>'))
        {
            return None;
        }
        Some(Page {
            url: url.into(),
            title: title.chars().take(300).collect(),
        })
    }
    pub fn visit(&mut self, url: &str, title: &str, private: bool) {
        if private {
            return;
        }
        if let Some(page) = Self::page(url, title) {
            let visited_at_ms = self.next_history_timestamp(url);
            self.history_visits.insert(
                0,
                HistoryVisit {
                    url: url.into(),
                    title: page.title.clone(),
                    visited_at_ms,
                },
            );
            self.history_visits.truncate(MAX_HISTORY_ITEMS);
            self.history.retain(|p| p.url != url);
            self.history.insert(0, page);
            self.history.truncate(MAX_HISTORY_ITEMS);
            self.rebuild_history_projection();
        }
    }

    /// Upgrade old per-URL timestamp metadata into visit events on load.
    /// Legacy profiles did not retain repeated visits, so they migrate as one
    /// event per saved URL; their original visit time is unavailable.
    fn migrate_history_events(&mut self) -> bool {
        let old_visits = self.history_visits.clone();
        let old_history = self.history.clone();
        for visit in &mut self.history_visits {
            if visit.title.is_empty() {
                if let Some(page) = self.history.iter().find(|page| page.url == visit.url) {
                    visit.title = page.title.clone();
                }
            }
        }
        for page in &self.history {
            if !self
                .history_visits
                .iter()
                .any(|visit| visit.url == page.url)
            {
                self.history_visits.push(HistoryVisit {
                    url: page.url.clone(),
                    title: page.title.clone(),
                    visited_at_ms: 0,
                });
            }
        }
        self.history_visits.sort_by(|left, right| {
            right
                .visited_at_ms
                .cmp(&left.visited_at_ms)
                .then_with(|| left.url.cmp(&right.url))
        });
        self.history_visits.truncate(MAX_HISTORY_ITEMS);
        self.rebuild_history_projection();
        old_visits != self.history_visits || old_history != self.history
    }

    fn rebuild_history_projection(&mut self) {
        let mut seen = std::collections::HashSet::new();
        self.history = self
            .history_visits
            .iter()
            .filter(|visit| seen.insert(visit.url.as_str()))
            .map(|visit| Page {
                url: visit.url.clone(),
                title: visit.title.clone(),
            })
            .collect();
        self.history.truncate(MAX_HISTORY_ITEMS);
    }
    pub fn toggle_bookmark(&mut self, url: &str, title: &str) -> bool {
        if self.bookmarks.iter().any(|p| p.url == url) {
            self.remove_bookmark(url);
            true
        } else if let Some(page) = Self::page(url, title) {
            // Never silently evict an existing bookmark when the local library is full.
            if self.bookmarks.len() >= MAX_BOOKMARKS {
                return false;
            }
            let updated_at_ms = self.next_bookmark_timestamp(url);
            self.removed_bookmarks.retain(|removed| removed != url);
            self.bookmark_tombstones
                .retain(|tombstone| tombstone.url != url);
            self.bookmark_updates.retain(|update| update.url != url);
            self.bookmark_updates.insert(
                0,
                BookmarkUpdate {
                    url: url.into(),
                    updated_at_ms,
                },
            );
            self.bookmark_updates.truncate(MAX_BOOKMARKS);
            self.bookmarks.insert(0, page);
            true
        } else {
            false
        }
    }

    pub fn remove_bookmark(&mut self, url: &str) {
        if self.bookmarks.iter().any(|page| page.url == url) {
            let deleted_at_ms = self.next_bookmark_timestamp(url);
            self.bookmarks.retain(|page| page.url != url);
            self.bookmark_folder_assignments
                .retain(|item| item.url != url);
            self.bookmark_updates.retain(|update| update.url != url);
            self.removed_bookmarks.retain(|removed| removed != url);
            self.removed_bookmarks.push(url.to_string());
            if self.removed_bookmarks.len() > MAX_BOOKMARK_TOMBSTONES {
                let excess = self.removed_bookmarks.len() - MAX_BOOKMARK_TOMBSTONES;
                self.removed_bookmarks.drain(..excess);
            }
            self.bookmark_tombstones
                .retain(|tombstone| tombstone.url != url);
            self.bookmark_tombstones.insert(
                0,
                BookmarkTombstone {
                    url: url.to_string(),
                    deleted_at_ms,
                },
            );
            self.bookmark_tombstones.truncate(MAX_BOOKMARK_TOMBSTONES);
        }
    }

    pub fn bookmark_folder_for(&self, url: &str) -> Option<&str> {
        self.bookmark_folder_assignments
            .iter()
            .find(|item| item.url == url)
            .and_then(|item| item.folder_id.as_deref())
    }

    pub fn create_bookmark_folder(
        &mut self,
        name: &str,
        parent_id: Option<&str>,
    ) -> Option<String> {
        let name = name.trim();
        if name.is_empty()
            || name.chars().count() > 100
            || name.chars().any(char::is_control)
            || self.bookmark_folders.len() >= MAX_BOOKMARK_FOLDERS
            || parent_id
                .is_some_and(|id| !self.bookmark_folders.iter().any(|folder| folder.id == id))
            || parent_id.is_some_and(|id| self.bookmark_folder_depth(id) >= 16)
        {
            return None;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let mut id = format!("pip-folder-{now}-{}", std::process::id());
        while self.bookmark_folders.iter().any(|folder| folder.id == id)
            || self
                .bookmark_folder_tombstones
                .iter()
                .any(|removed| removed.id == id)
        {
            id = format!(
                "pip-folder-{now}-{}-{}",
                std::process::id(),
                NEXT_BOOKMARK_FOLDER_ID.fetch_add(1, Ordering::Relaxed)
            );
        }
        self.bookmark_folders.push(BookmarkFolder {
            id: id.clone(),
            name: name.to_string(),
            parent_id: parent_id.map(str::to_string),
        });
        Some(id)
    }

    fn bookmark_folder_depth(&self, folder_id: &str) -> usize {
        let mut depth = 0;
        let mut current = Some(folder_id);
        let mut seen = std::collections::HashSet::new();
        while let Some(id) = current {
            if !seen.insert(id) {
                return usize::MAX;
            }
            let Some(folder) = self.bookmark_folders.iter().find(|folder| folder.id == id) else {
                return usize::MAX;
            };
            current = folder.parent_id.as_deref();
            if current.is_some() {
                depth += 1;
            }
        }
        depth
    }

    /// Deletes a folder subtree; saved bookmarks in it remain at the root.
    pub fn delete_bookmark_folder(&mut self, folder_id: &str) -> bool {
        if !self
            .bookmark_folders
            .iter()
            .any(|folder| folder.id == folder_id)
        {
            return false;
        }
        let mut removed = std::collections::HashSet::from([folder_id.to_string()]);
        loop {
            let before = removed.len();
            for folder in &self.bookmark_folders {
                if folder
                    .parent_id
                    .as_ref()
                    .is_some_and(|parent| removed.contains(parent))
                {
                    removed.insert(folder.id.clone());
                }
            }
            if before == removed.len() {
                break;
            }
        }
        let additional_tombstones = removed
            .iter()
            .filter(|id| {
                !self
                    .bookmark_folder_tombstones
                    .iter()
                    .any(|item| item.id == **id)
            })
            .count();
        if self.bookmark_folder_tombstones.len() + additional_tombstones > 2_000 {
            return false;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(i64::MAX as u128) as i64;
        for id in &removed {
            self.bookmark_folder_tombstones
                .retain(|item| item.id != *id);
            self.bookmark_folder_tombstones
                .push(BookmarkFolderTombstone {
                    id: id.clone(),
                    deleted_at_ms: now,
                });
        }
        self.bookmark_folder_tombstones.sort_by(|a, b| {
            b.deleted_at_ms
                .cmp(&a.deleted_at_ms)
                .then_with(|| a.id.cmp(&b.id))
        });
        self.bookmark_folder_tombstones.truncate(2_000);
        self.bookmark_folders
            .retain(|folder| !removed.contains(&folder.id));
        let moved_urls: Vec<_> = self
            .bookmark_folder_assignments
            .iter()
            .filter(|placement| {
                placement
                    .folder_id
                    .as_ref()
                    .is_some_and(|id| removed.contains(id))
            })
            .map(|placement| placement.url.clone())
            .collect();
        for url in moved_urls {
            let updated_at_ms = self.next_bookmark_timestamp(&url);
            if let Some(placement) = self
                .bookmark_folder_assignments
                .iter_mut()
                .find(|placement| placement.url == url)
            {
                placement.folder_id = None;
                placement.updated_at_ms = updated_at_ms;
            }
            self.bookmark_updates.retain(|item| item.url != url);
            self.bookmark_updates
                .insert(0, BookmarkUpdate { url, updated_at_ms });
        }
        self.bookmark_updates.truncate(MAX_BOOKMARKS);
        true
    }

    pub fn move_bookmark_to_folder(&mut self, url: &str, folder_id: Option<&str>) -> bool {
        if !self.bookmarks.iter().any(|page| page.url == url)
            || folder_id
                .is_some_and(|id| !self.bookmark_folders.iter().any(|folder| folder.id == id))
        {
            return false;
        }
        self.bookmark_folder_assignments
            .retain(|item| item.url != url);
        let updated_at_ms = self.next_bookmark_timestamp(url);
        self.bookmark_folder_assignments
            .push(BookmarkFolderAssignment {
                url: url.to_string(),
                folder_id: folder_id.map(str::to_string),
                updated_at_ms,
            });
        self.bookmark_updates.retain(|update| update.url != url);
        self.bookmark_updates.insert(
            0,
            BookmarkUpdate {
                url: url.to_string(),
                updated_at_ms,
            },
        );
        self.bookmark_updates.truncate(MAX_BOOKMARKS);
        true
    }

    fn next_bookmark_timestamp(&self, url: &str) -> i64 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(i64::MAX as u128) as i64;
        let previous = self
            .bookmark_updates
            .iter()
            .filter(|update| update.url == url)
            .map(|update| update.updated_at_ms)
            .chain(
                self.bookmark_tombstones
                    .iter()
                    .filter(|tombstone| tombstone.url == url)
                    .map(|tombstone| tombstone.deleted_at_ms),
            )
            .max();
        previous.map_or(now, |previous| now.max(previous.saturating_add(1)))
    }

    pub fn remove_history(&mut self, url: &str) {
        if Self::page(url, "").is_none() {
            return;
        }
        let removed_at_ms = self.next_history_timestamp(url);
        self.history.retain(|page| page.url != url);
        self.history_visits.retain(|visit| visit.url != url);
        self.removed_history.retain(|entry| entry.url != url);
        self.removed_history.insert(
            0,
            HistoryTombstone {
                url: url.into(),
                removed_at_ms,
                visit_at_ms: None,
            },
        );
        self.removed_history.truncate(MAX_HISTORY_TOMBSTONES);
    }

    pub fn remove_history_visit(&mut self, url: &str, visited_at_ms: i64) {
        if Self::page(url, "").is_none() {
            return;
        }
        let Some(index) = self
            .history_visits
            .iter()
            .position(|visit| visit.url == url && visit.visited_at_ms == visited_at_ms)
        else {
            return;
        };
        let visit = self.history_visits.remove(index);
        let removed_at_ms = self.next_history_timestamp(url);
        self.removed_history
            .retain(|entry| entry.url != url || entry.visit_at_ms != Some(visited_at_ms));
        self.removed_history.insert(
            0,
            HistoryTombstone {
                url: url.into(),
                removed_at_ms: removed_at_ms.max(visit.visited_at_ms),
                visit_at_ms: Some(visited_at_ms),
            },
        );
        self.removed_history.truncate(MAX_HISTORY_TOMBSTONES);
        self.rebuild_history_projection();
    }

    /// Clear visible history and record bounded deletion metadata for sync.
    pub fn clear_history(&mut self) {
        let mut urls = std::collections::HashSet::new();
        urls.extend(self.history.iter().map(|page| page.url.clone()));
        urls.extend(self.history_visits.iter().map(|visit| visit.url.clone()));
        for url in urls {
            self.remove_history(&url);
        }
        self.history.clear();
        self.history_visits.clear();
    }

    fn next_history_timestamp(&self, url: &str) -> i64 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(i64::MAX as u128) as i64;
        let previous = self
            .history_visits
            .iter()
            .filter(|visit| visit.url == url)
            .map(|visit| visit.visited_at_ms)
            .chain(
                self.removed_history
                    .iter()
                    .filter(|entry| entry.url == url)
                    .map(|entry| entry.removed_at_ms),
            )
            .max();
        previous.map_or(now, |previous| now.max(previous.saturating_add(1)))
    }

    pub fn home_shortcuts(&self) -> Vec<Page> {
        if self.home_shortcuts_configured {
            self.home_shortcuts
                .iter()
                .take(MAX_HOME_SHORTCUTS)
                .cloned()
                .collect()
        } else {
            default_home_shortcuts()
        }
    }

    pub fn add_home_shortcut(&mut self, url: &str, title: &str) -> bool {
        let Some(page) = Self::page(url.trim(), title.trim()) else {
            return false;
        };
        if page.title.is_empty() {
            return false;
        }
        self.initialize_home_shortcuts();
        if self.home_shortcuts.len() >= MAX_HOME_SHORTCUTS
            || self
                .home_shortcuts
                .iter()
                .any(|existing| existing.url == page.url)
        {
            return false;
        }
        self.home_shortcuts.push(page);
        true
    }

    pub fn remove_home_shortcut(&mut self, url: &str) -> bool {
        self.initialize_home_shortcuts();
        let old_len = self.home_shortcuts.len();
        self.home_shortcuts.retain(|page| page.url != url);
        old_len != self.home_shortcuts.len()
    }

    /// Move one quick link by a single place. Direction is restricted to -1 or 1.
    pub fn move_home_shortcut(&mut self, url: &str, direction: i8) -> bool {
        if !matches!(direction, -1 | 1) {
            return false;
        }
        self.initialize_home_shortcuts();
        let Some(index) = self.home_shortcuts.iter().position(|page| page.url == url) else {
            return false;
        };
        let destination = if direction < 0 {
            index.checked_sub(1)
        } else {
            (index + 1 < self.home_shortcuts.len()).then_some(index + 1)
        };
        let Some(destination) = destination else {
            return false;
        };
        self.home_shortcuts.swap(index, destination);
        true
    }

    fn initialize_home_shortcuts(&mut self) {
        if !self.home_shortcuts_configured {
            self.home_shortcuts = default_home_shortcuts();
            self.home_shortcuts_configured = true;
        }
    }

    /// Return a bounded, local-only address-bar suggestion list. Recent
    /// history ranks first; bookmarks fill remaining slots. Private windows
    /// can disable history matching while retaining saved bookmark matches.
    pub fn suggestions(&self, query: &str, include_history: bool, limit: usize) -> Vec<Page> {
        let query = query.trim();
        if query.is_empty() || limit == 0 {
            return Vec::new();
        }
        let query = query.to_lowercase();
        let matches = |page: &Page| {
            page.title.to_lowercase().contains(&query) || page.url.to_lowercase().contains(&query)
        };
        let mut seen = std::collections::HashSet::new();
        let mut suggestions = Vec::with_capacity(limit.min(8));
        if include_history {
            for page in self.history.iter().filter(|page| matches(page)) {
                if seen.insert(page.url.as_str()) {
                    suggestions.push(page.clone());
                    if suggestions.len() >= limit {
                        return suggestions;
                    }
                }
            }
        }
        for page in self.bookmarks.iter().filter(|page| matches(page)) {
            if seen.insert(page.url.as_str()) {
                suggestions.push(page.clone());
                if suggestions.len() >= limit {
                    break;
                }
            }
        }
        suggestions
    }

    pub fn import_netscape_bookmarks(&mut self, html: &str) -> io::Result<BookmarkImportResult> {
        if html.len() > MAX_BOOKMARK_IMPORT_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Tệp dấu trang vượt giới hạn dung lượng",
            ));
        }
        let lower = html.to_ascii_lowercase();
        if !lower.contains("<!doctype netscape-bookmark-file-1") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Tệp không có định dạng dấu trang HTML của trình duyệt",
            ));
        }

        #[derive(Clone)]
        struct ParsedFolder {
            name: String,
            parent: Option<usize>,
        }

        let mut parsed_folders = Vec::<ParsedFolder>::new();
        let mut parsed = Vec::<(Page, Option<usize>)>::new();
        let available = MAX_BOOKMARKS.saturating_sub(self.bookmarks.len());
        let existing_urls: std::collections::HashSet<&str> = self
            .bookmarks
            .iter()
            .map(|page| page.url.as_str())
            .collect();
        let mut parsed_urls = std::collections::HashSet::<String>::new();
        let mut folder_stack = Vec::<Option<usize>>::new();
        let mut pending_folder = None;
        let mut cursor = 0;
        while let Some(relative) = html[cursor..].find('<') {
            let start = cursor + relative;
            let Some(tag_end) = find_tag_end(html, start + 1) else {
                break;
            };
            let raw_tag = html[start + 1..tag_end].trim();
            let closing = raw_tag.starts_with('/');
            let tag_body = raw_tag.trim_start_matches('/').trim_start();
            let tag_name = tag_body
                .split(|ch: char| ch.is_ascii_whitespace() || ch == '/')
                .next()
                .unwrap_or_default();
            if tag_name.eq_ignore_ascii_case("DL") {
                if closing {
                    folder_stack.pop();
                } else {
                    let folder = if let Some(name) = pending_folder.take() {
                        if parsed_folders.len() >= MAX_BOOKMARK_FOLDERS {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "Tệp có quá nhiều thư mục dấu trang; không nhập mục nào.",
                            ));
                        }
                        let parent = folder_stack.iter().rev().find_map(|item| *item);
                        let index = parsed_folders.len();
                        parsed_folders.push(ParsedFolder { name, parent });
                        Some(index)
                    } else {
                        folder_stack.iter().rev().find_map(|item| *item)
                    };
                    folder_stack.push(folder);
                }
            } else if tag_name.eq_ignore_ascii_case("H3") && !closing {
                if let Some(close) = lower[tag_end + 1..].find("</h3") {
                    pending_folder = Some(
                        decode_html_entities(&strip_tags(&html[tag_end + 1..tag_end + 1 + close]))
                            .trim()
                            .chars()
                            .take(100)
                            .collect(),
                    );
                    cursor = tag_end + 1 + close + 5;
                    continue;
                }
            } else if tag_name.eq_ignore_ascii_case("A") && !closing {
                if let Some(href) = anchor_href(tag_body) {
                    if let Some(close) = lower[tag_end + 1..].find("</a") {
                        let content_end = tag_end + 1 + close;
                        let title =
                            decode_html_entities(&strip_tags(&html[tag_end + 1..content_end]));
                        let url = decode_html_entities(href.trim());
                        if let Some(page) = Self::page(&url, &title) {
                            if !existing_urls.contains(page.url.as_str())
                                && parsed_urls.insert(page.url.clone())
                            {
                                if parsed.len() >= available {
                                    return Err(io::Error::new(
                                        io::ErrorKind::InvalidData,
                                        "Tệp có nhiều dấu trang mới hơn số mục Pip còn hỗ trợ; không nhập mục nào.",
                                    ));
                                }
                                let folder = folder_stack.iter().rev().find_map(|item| *item);
                                parsed.push((page, folder));
                            }
                        }
                        cursor = content_end + 4;
                        continue;
                    }
                }
            }
            cursor = tag_end + 1;
        }

        let mut next = self.clone();
        let mut folder_ids: Vec<String> = Vec::with_capacity(parsed_folders.len());
        let mut claimed_folder_ids = std::collections::HashSet::new();
        let original_folder_count = next.bookmark_folders.len();
        for (index, folder) in parsed_folders.iter().enumerate() {
            let parent_id = folder
                .parent
                .and_then(|parent| folder_ids.get(parent).cloned());
            if folder.parent.is_some() && parent_id.is_none() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Tệp có cấu trúc thư mục không hợp lệ; không nhập mục nào.",
                ));
            }
            let existing = next
                .bookmark_folders
                .iter()
                .find(|candidate| {
                    candidate.name == folder.name
                        && candidate.parent_id == parent_id
                        && !claimed_folder_ids.contains(&candidate.id)
                })
                .map(|candidate| candidate.id.clone());
            let id = match existing {
                Some(id) => id,
                None => next
                    .create_bookmark_folder(&folder.name, parent_id.as_deref())
                    .ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            "Tệp có quá nhiều thư mục hoặc cấu trúc thư mục không hợp lệ; không nhập mục nào.",
                        )
                    })?,
            };
            claimed_folder_ids.insert(id.clone());
            folder_ids.push(id);
            debug_assert_eq!(folder_ids.len(), index + 1);
        }
        let mut imported_pages = Vec::with_capacity(parsed.len());
        for (page, folder_index) in &parsed {
            next.removed_bookmarks
                .retain(|removed| removed != &page.url);
            let updated_at_ms = next.next_bookmark_timestamp(&page.url);
            next.bookmark_tombstones
                .retain(|tombstone| tombstone.url != page.url);
            next.bookmark_updates
                .retain(|update| update.url != page.url);
            next.bookmark_updates.insert(
                0,
                BookmarkUpdate {
                    url: page.url.clone(),
                    updated_at_ms,
                },
            );
            if let Some(folder_id) = folder_index.and_then(|index| folder_ids.get(index)) {
                next.bookmark_folder_assignments
                    .push(BookmarkFolderAssignment {
                        url: page.url.clone(),
                        folder_id: Some(folder_id.clone()),
                        updated_at_ms,
                    });
            }
            imported_pages.push(page.clone());
        }
        next.bookmarks.splice(0..0, imported_pages);
        next.bookmark_updates.truncate(MAX_BOOKMARKS);
        *self = next;
        Ok(BookmarkImportResult {
            bookmarks: parsed.len(),
            folders: self.bookmark_folders.len() - original_folder_count,
        })
    }

    pub fn export_netscape_bookmarks(&self) -> String {
        let mut html = String::from(
            "<!DOCTYPE NETSCAPE-Bookmark-file-1>\n<META HTTP-EQUIV=\"Content-Type\" CONTENT=\"text/html; charset=UTF-8\">\n<TITLE>Pip Bookmarks</TITLE>\n<H1>Bookmarks</H1>\n<DL><p>\n",
        );
        self.export_bookmark_level(None, 1, &mut html, &mut std::collections::HashSet::new());
        html.push_str("</DL><p>\n");
        html
    }

    fn export_bookmark_level(
        &self,
        parent_id: Option<&str>,
        depth: usize,
        html: &mut String,
        visited: &mut std::collections::HashSet<String>,
    ) {
        if depth > 16 {
            return;
        }
        for page in self
            .bookmarks
            .iter()
            .filter(|page| self.bookmark_folder_for(&page.url) == parent_id)
        {
            html.push_str("<DT><A HREF=\"");
            html.push_str(&escape_html(&page.url));
            html.push_str("\">");
            html.push_str(&escape_html(&page.title));
            html.push_str("</A>\n");
        }
        let child_folders: Vec<_> = self
            .bookmark_folders
            .iter()
            .filter(|folder| folder.parent_id.as_deref() == parent_id)
            .cloned()
            .collect();
        for folder in child_folders {
            if visited.contains(&folder.id) {
                continue;
            }
            visited.insert(folder.id.clone());
            html.push_str("<DT><H3>");
            html.push_str(&escape_html(&folder.name));
            html.push_str("</H3>\n<DL><p>\n");
            self.export_bookmark_level(Some(&folder.id), depth + 1, html, visited);
            html.push_str("</DL><p>\n");
        }
    }
}

fn url_has_userinfo(url: &str) -> bool {
    let Some((scheme, rest)) = url.split_once("://") else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return false;
    }
    rest.split(['/', '?', '#'])
        .next()
        .is_some_and(|authority| authority.contains('@'))
}

fn default_home_shortcuts() -> Vec<Page> {
    [
        ("https://www.google.com/", "Google"),
        ("https://www.youtube.com/", "YouTube"),
        ("https://github.com/", "GitHub"),
        ("https://psharehub.com/", "Psharehub"),
    ]
    .into_iter()
    .map(|(url, title)| Page {
        url: url.into(),
        title: title.into(),
    })
    .collect()
}

fn find_tag_end(html: &str, start: usize) -> Option<usize> {
    let mut quote = None;
    for (offset, ch) in html[start..].char_indices() {
        match (quote, ch) {
            (Some(current), value) if current == value => quote = None,
            (None, '"' | '\'') => quote = Some(ch),
            (None, '>') => return Some(start + offset),
            _ => {}
        }
    }
    None
}

fn anchor_href(attributes: &str) -> Option<&str> {
    let bytes = attributes.as_bytes();
    let mut cursor = 0;
    while cursor < bytes.len() {
        while cursor < bytes.len() && (bytes[cursor].is_ascii_whitespace() || bytes[cursor] == b'/')
        {
            cursor += 1;
        }
        let name_start = cursor;
        while cursor < bytes.len()
            && !bytes[cursor].is_ascii_whitespace()
            && !matches!(bytes[cursor], b'=' | b'/')
        {
            cursor += 1;
        }
        if name_start == cursor {
            cursor += 1;
            continue;
        }
        let name = &attributes[name_start..cursor];
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor == bytes.len() || bytes[cursor] != b'=' {
            continue;
        }
        cursor += 1;
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor == bytes.len() {
            break;
        }
        let (value_start, value_end);
        if matches!(bytes[cursor], b'"' | b'\'') {
            let quote = bytes[cursor];
            cursor += 1;
            value_start = cursor;
            while cursor < bytes.len() && bytes[cursor] != quote {
                cursor += 1;
            }
            value_end = cursor;
            cursor = (cursor + 1).min(bytes.len());
        } else {
            value_start = cursor;
            while cursor < bytes.len() && !bytes[cursor].is_ascii_whitespace() {
                cursor += 1;
            }
            value_end = cursor;
        }
        if name.eq_ignore_ascii_case("href") {
            return attributes.get(value_start..value_end);
        }
    }
    None
}

fn strip_tags(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut in_tag = false;
    for ch in value.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => output.push(ch),
            _ => {}
        }
    }
    output
}

fn decode_html_entities(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(ampersand) = rest.find('&') {
        output.push_str(&rest[..ampersand]);
        let entity_start = ampersand + 1;
        let Some(relative_end) = rest[entity_start..].find(';') else {
            output.push_str(&rest[ampersand..]);
            return output;
        };
        let entity_end = entity_start + relative_end;
        let entity = &rest[entity_start..entity_end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            _ if entity.starts_with("#x") || entity.starts_with("#X") => {
                u32::from_str_radix(&entity[2..], 16)
                    .ok()
                    .and_then(char::from_u32)
            }
            _ if entity.starts_with('#') => entity[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        if let Some(ch) = decoded {
            output.push(ch);
        } else {
            output.push_str(&rest[ampersand..=entity_end]);
        }
        rest = &rest[entity_end + 1..];
    }
    output.push_str(rest);
    output
}

fn escape_html(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#39;"),
            _ => output.push(ch),
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn library_bounds_private_visits_and_roundtrip() {
        let mut library = Library::default();
        library.visit("https://private.example/", "Secret", true);
        library.visit("javascript:alert(1)", "Invalid", false);
        assert!(library.history.is_empty());
        assert!(library.history_visits.is_empty());
        for n in 0..510 {
            library.visit(&format!("https://example.com/{n}"), "Page", false);
        }
        assert_eq!(library.history_visits.len(), 500);
        library.visit("https://example.com/509", "New title", false);
        assert_eq!(library.history_visits.len(), 500);
        assert_eq!(library.history.len(), 499);
        assert_eq!(library.history[0].title, "New title");
        library.toggle_bookmark("https://example.com/", "<untrusted>");
        let first_bookmark_update = library.bookmark_updates[0].updated_at_ms;
        let path = std::env::temp_dir().join(format!("pip-library-{}.json", std::process::id()));
        library.save(&path).unwrap();
        let mut restored = Library::load(&path).unwrap();
        assert_eq!(restored.bookmarks[0].title, "<untrusted>");
        restored.toggle_bookmark("https://example.com/", "Page");
        assert!(restored.bookmarks.is_empty());
        assert_eq!(restored.removed_bookmarks, vec!["https://example.com/"]);
        let deletion_time = restored.bookmark_tombstones[0].deleted_at_ms;
        assert!(deletion_time >= first_bookmark_update);
        assert!(restored.bookmark_updates.is_empty());
        restored.toggle_bookmark("https://example.com/", "Added again");
        assert!(restored.removed_bookmarks.is_empty());
        assert!(restored.bookmark_tombstones.is_empty());
        assert!(restored.bookmark_updates[0].updated_at_ms > deletion_time);
        restored.toggle_bookmark("https://example.com/", "Page");
        assert_eq!(restored.removed_bookmarks, vec!["https://example.com/"]);
        assert_eq!(restored.bookmark_tombstones.len(), 1);
        restored.visit("https://example.com/history", "History", false);
        let first_visit = restored.history_visits[0].visited_at_ms;
        restored.remove_history("https://example.com/history");
        assert!(!restored
            .history
            .iter()
            .any(|page| page.url == "https://example.com/history"));
        assert!(!restored
            .history_visits
            .iter()
            .any(|visit| visit.url == "https://example.com/history"));
        assert_eq!(restored.removed_history.len(), 1);
        assert_eq!(
            restored.removed_history[0].url,
            "https://example.com/history"
        );
        assert!(restored.removed_history[0].removed_at_ms >= first_visit);
        restored.visit("https://example.com/history", "Visited again", false);
        assert_eq!(restored.removed_history.len(), 1);
        assert!(restored.history_visits[0].visited_at_ms > first_visit);
        assert_eq!(
            restored.history_visits[0].url,
            "https://example.com/history"
        );
        assert_eq!(restored.history_visits[0].title, "Visited again");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn repeated_history_visits_are_distinct_and_row_removal_keeps_other_events() {
        let mut library = Library::default();
        library.visit("https://example.com/repeat", "First title", false);
        library.visit("https://example.com/repeat", "Second title", false);
        library.visit("https://example.com/other", "Other", false);
        assert_eq!(library.history.len(), 2);
        assert_eq!(library.history_visits.len(), 3);
        let repeat_visits = library
            .history_visits
            .iter()
            .filter(|visit| visit.url == "https://example.com/repeat")
            .collect::<Vec<_>>();
        assert_eq!(repeat_visits[0].title, "Second title");
        assert_eq!(repeat_visits[1].title, "First title");
        let second_visit_time = repeat_visits[0].visited_at_ms;
        let first_visit_time = repeat_visits[1].visited_at_ms;
        assert!(second_visit_time > first_visit_time);

        library.remove_history_visit("https://example.com/repeat", second_visit_time);
        assert_eq!(library.history_visits.len(), 2);
        assert!(library.history_visits.iter().any(|visit| {
            visit.url == "https://example.com/repeat" && visit.visited_at_ms == first_visit_time
        }));
        assert!(library
            .history_visits
            .iter()
            .any(|visit| visit.url == "https://example.com/other"));
        assert_eq!(library.removed_history.len(), 1);
        assert_eq!(
            library.removed_history[0].visit_at_ms,
            Some(second_visit_time)
        );
        assert!(library.removed_history[0].removed_at_ms > 0);

        library.clear_history();
        assert!(library.history.is_empty());
        assert!(library.history_visits.is_empty());
        assert!(library.removed_history.iter().any(|entry| {
            entry.url == "https://example.com/repeat" && entry.visit_at_ms.is_none()
        }));
    }

    #[test]
    fn legacy_history_migrates_to_one_event_per_url_and_backfills_titles() {
        let mut library: Library = serde_json::from_str(
            r#"{"history":[{"url":"https://old.example/","title":"Old title"}],"history_visits":[{"url":"https://old.example/","visited_at_ms":1234}]}"#,
        )
        .unwrap();
        assert!(library.migrate_history_events());
        assert_eq!(library.history_visits.len(), 1);
        assert_eq!(library.history_visits[0].visited_at_ms, 1234);
        assert_eq!(library.history_visits[0].title, "Old title");

        let mut no_metadata: Library = serde_json::from_str(
            r#"{"history":[{"url":"https://legacy.example/","title":"Legacy"}]}"#,
        )
        .unwrap();
        assert!(no_metadata.migrate_history_events());
        assert_eq!(no_metadata.history_visits.len(), 1);
        assert_eq!(no_metadata.history_visits[0].visited_at_ms, 0);
        assert_eq!(no_metadata.history_visits[0].title, "Legacy");
    }

    #[test]
    fn unknown_legacy_visit_times_sort_oldest_and_are_first_to_be_evicted() {
        let mut library: Library = serde_json::from_str(
            r#"{"history":[{"url":"https://legacy.example/","title":"Legacy"}]}"#,
        )
        .unwrap();
        library.migrate_history_events();
        library.visit("https://new.example/", "New", false);
        assert_eq!(library.history_visits[0].url, "https://new.example/");
        assert_eq!(library.history_visits[1].visited_at_ms, 0);

        for index in 0..MAX_HISTORY_ITEMS {
            library.visit(&format!("https://fresh-{index}.example/"), "Fresh", false);
        }
        assert_eq!(library.history_visits.len(), MAX_HISTORY_ITEMS);
        assert!(!library
            .history_visits
            .iter()
            .any(|visit| visit.url == "https://legacy.example/"));
    }

    #[test]
    fn history_metadata_defaults_for_legacy_profiles_and_private_visits_do_not_mutate_it() {
        let mut library: Library = serde_json::from_str(
            r#"{"bookmarks":[],"history":[{"url":"https://old.example/","title":"Old"}],"removed_bookmarks":[]}"#,
        )
        .unwrap();
        assert!(library.history_visits.is_empty());
        assert!(library.removed_history.is_empty());

        library.visit("https://private.example/", "Private", true);
        assert_eq!(library.history.len(), 1);
        assert!(library.history_visits.is_empty());
        assert!(library.removed_history.is_empty());
    }

    #[test]
    fn credentialed_urls_are_removed_from_legacy_library_and_rewritten() {
        let path = std::env::temp_dir().join(format!(
            "pip-library-credential-cleanup-{}.json",
            std::process::id()
        ));
        let legacy = serde_json::json!({
            "bookmarks": [{"url":"https://alice:secret@example.com/bookmark","title":"Bookmark"}],
            "bookmark_updates": [{"url":"https://alice:secret@example.com/bookmark","updated_at_ms":1}],
            "bookmark_tombstones": [{"url":"https://alice:secret@example.com/deleted-bookmark","deleted_at_ms":1}],
            "history": [{"url":"https://bob:secret@example.com/history","title":"History"}],
            "history_visits": [{"url":"https://bob:secret@example.com/history","visited_at_ms":1}],
            "removed_history": [{"url":"https://bob:secret@example.com/removed-history","removed_at_ms":1}],
            "removed_bookmarks": ["https://alice:secret@example.com/removed-bookmark"],
            "home_shortcuts": [{"url":"https://alice:secret@example.com/shortcut","title":"Shortcut"}],
            "home_shortcuts_configured": true
        });
        std::fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();

        let loaded = Library::load(&path).unwrap();
        assert!(loaded.bookmarks.is_empty());
        assert!(loaded.bookmark_updates.is_empty());
        assert!(loaded.bookmark_tombstones.is_empty());
        assert!(loaded.history.is_empty());
        assert!(loaded.history_visits.is_empty());
        assert!(loaded.removed_history.is_empty());
        assert!(loaded.removed_bookmarks.is_empty());
        assert!(loaded.home_shortcuts().is_empty());
        assert!(loaded.home_shortcuts_configured);

        let rewritten = std::fs::read(&path).unwrap();
        assert!(!String::from_utf8_lossy(&rewritten).contains("secret"));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn clearing_history_tombstones_each_visible_url_and_keeps_page_shape() {
        let mut library = Library::default();
        library.visit("https://one.example/", "One", false);
        library.visit("https://two.example/", "Two", false);
        let visible_before_clear = library.history.clone();

        library.clear_history();

        assert!(library.history.is_empty());
        assert!(library.history_visits.is_empty());
        let tombstoned: std::collections::HashSet<_> = library
            .removed_history
            .iter()
            .map(|entry| entry.url.as_str())
            .collect();
        assert_eq!(tombstoned.len(), 2);
        assert!(visible_before_clear
            .iter()
            .all(|page| tombstoned.contains(page.url.as_str())));

        let serialized = serde_json::to_value(&library).unwrap();
        assert!(serialized["history"].is_array());
        assert!(serialized["history"].as_array().unwrap().is_empty());
    }

    #[test]
    fn history_tombstones_are_bounded() {
        let mut library = Library::default();
        for index in 0..=MAX_HISTORY_TOMBSTONES {
            library.remove_history(&format!("https://removed-{index}.example/"));
        }
        assert_eq!(library.removed_history.len(), MAX_HISTORY_TOMBSTONES);
        assert_eq!(
            library.removed_history[0].url,
            format!("https://removed-{}.example/", MAX_HISTORY_TOMBSTONES)
        );
        assert!(!library
            .removed_history
            .iter()
            .any(|entry| entry.url == "https://removed-0.example/"));
    }

    #[test]
    fn address_bar_suggestions_are_local_bounded_deduplicated_and_private_safe() {
        let mut library = Library::default();
        library.visit("https://older.example/", "Pip history", false);
        library.visit("https://recent.example/", "Pip browser", false);
        library.toggle_bookmark("https://recent.example/", "Pip browser");
        library.toggle_bookmark("https://saved.example/", "Pip release");

        let normal = library.suggestions("pip", true, 8);
        assert_eq!(
            normal
                .iter()
                .map(|page| page.url.as_str())
                .collect::<Vec<_>>(),
            [
                "https://recent.example/",
                "https://older.example/",
                "https://saved.example/",
            ]
        );
        let private = library.suggestions("pip", false, 8);
        assert_eq!(
            private
                .iter()
                .map(|page| page.url.as_str())
                .collect::<Vec<_>>(),
            ["https://saved.example/", "https://recent.example/"]
        );
        assert_eq!(library.suggestions("pip", true, 1).len(), 1);
        assert!(library.suggestions("   ", true, 8).is_empty());
    }

    #[test]
    fn home_shortcuts_migrate_defaults_and_support_bounded_safe_edits() {
        let mut legacy: Library =
            serde_json::from_str(r#"{"bookmarks":[],"history":[],"removed_bookmarks":[]}"#)
                .unwrap();
        assert!(legacy.bookmark_updates.is_empty());
        assert!(legacy.bookmark_tombstones.is_empty());
        assert_eq!(legacy.home_shortcuts()[0].title, "Google");
        assert_eq!(legacy.home_shortcuts().len(), 4);

        assert!(legacy.add_home_shortcut("https://docs.example/", "Docs"));
        assert!(!legacy.add_home_shortcut("https://docs.example/", "Duplicate"));
        assert!(!legacy.add_home_shortcut("javascript:alert(1)", "Unsafe"));
        assert_eq!(legacy.home_shortcuts().len(), 5);
        assert!(legacy.move_home_shortcut("https://docs.example/", -1));
        assert_eq!(legacy.home_shortcuts()[3].title, "Docs");
        assert!(legacy.move_home_shortcut("https://docs.example/", 1));
        assert!(legacy.remove_home_shortcut("https://docs.example/"));
        assert!(!legacy.remove_home_shortcut("https://missing.example/"));

        for index in 0..8 {
            assert!(legacy.add_home_shortcut(
                &format!("https://extra-{index}.example/"),
                &format!("Extra {index}")
            ));
        }
        assert_eq!(legacy.home_shortcuts().len(), 12);
        assert!(!legacy.add_home_shortcut("https://overflow.example/", "Overflow"));

        for shortcut in legacy.home_shortcuts() {
            assert!(legacy.remove_home_shortcut(&shortcut.url));
        }
        assert!(legacy.home_shortcuts().is_empty());
        let bytes = serde_json::to_vec(&legacy).unwrap();
        let restored: Library = serde_json::from_slice(&bytes).unwrap();
        assert!(restored.home_shortcuts_configured);
        assert!(restored.home_shortcuts().is_empty());
    }

    #[test]
    fn imports_and_exports_browser_bookmark_html_safely() {
        let html = r#"<!DOCTYPE NETSCAPE-Bookmark-file-1>
          <DL><p>
          <DT><A HREF="https://example.com/?a=1&amp;b=2" ADD_DATE="1">A &amp; B</A>
          <DT><A data-custom='yes' HREF='https://second.example/'>Second &#x1F98B;</A>
          <DT><A HREF="javascript:alert(1)">Unsafe</A>
          <DT><A HREF="https://example.com/?a=1&amp;b=2">Duplicate</A>
          </DL><p>"#;
        let mut library = Library::default();
        assert_eq!(
            library.import_netscape_bookmarks(html).unwrap(),
            BookmarkImportResult {
                bookmarks: 2,
                folders: 0
            }
        );
        assert_eq!(library.bookmark_updates.len(), 2);
        assert_eq!(library.bookmarks[0].title, "A & B");
        assert_eq!(library.bookmarks[0].url, "https://example.com/?a=1&b=2");
        assert_eq!(library.bookmarks[1].title, "Second 🦋");
        assert_eq!(
            library.import_netscape_bookmarks(html).unwrap(),
            BookmarkImportResult::default()
        );

        let exported = library.export_netscape_bookmarks();
        assert!(exported.contains("<!DOCTYPE NETSCAPE-Bookmark-file-1>"));
        assert!(exported.contains("https://example.com/?a=1&amp;b=2"));
        assert!(exported.contains("A &amp; B"));
        let mut restored = Library::default();
        assert_eq!(
            restored.import_netscape_bookmarks(&exported).unwrap(),
            BookmarkImportResult {
                bookmarks: 2,
                folders: 0
            }
        );
        assert_eq!(restored.bookmarks[0].url, library.bookmarks[0].url);
        assert_eq!(restored.bookmarks[1].title, library.bookmarks[1].title);
    }

    #[test]
    fn nested_bookmark_html_roundtrip_preserves_folder_hierarchy() {
        let html = r#"<!DOCTYPE NETSCAPE-Bookmark-file-1>
<DL><p>
<DT><H3>Research &amp; Notes</H3>
<DL><p>
<DT><A HREF="https://one.example/">One</A>
<DT><H3>Nested</H3>
<DL><p><DT><A HREF="https://two.example/">Two</A></DL><p>
</DL><p>
</DL><p>"#;
        let mut library = Library::default();
        assert_eq!(
            library.import_netscape_bookmarks(html).unwrap(),
            BookmarkImportResult {
                bookmarks: 2,
                folders: 2
            }
        );
        assert_eq!(library.bookmark_folders.len(), 2);
        let parent = library
            .bookmark_folders
            .iter()
            .find(|folder| folder.name == "Research & Notes")
            .unwrap();
        let nested = library
            .bookmark_folders
            .iter()
            .find(|folder| folder.name == "Nested")
            .unwrap();
        assert_eq!(nested.parent_id.as_deref(), Some(parent.id.as_str()));
        assert_eq!(
            library.bookmark_folder_for("https://one.example/"),
            Some(parent.id.as_str())
        );
        assert_eq!(
            library.bookmark_folder_for("https://two.example/"),
            Some(nested.id.as_str())
        );

        let mut restored = Library::default();
        assert_eq!(
            restored
                .import_netscape_bookmarks(&library.export_netscape_bookmarks())
                .unwrap(),
            BookmarkImportResult {
                bookmarks: 2,
                folders: 2
            }
        );
        let restored_parent = restored
            .bookmark_folders
            .iter()
            .find(|folder| folder.name == parent.name)
            .unwrap();
        let restored_nested = restored
            .bookmark_folders
            .iter()
            .find(|folder| folder.name == nested.name)
            .unwrap();
        assert_eq!(
            restored_nested.parent_id.as_deref(),
            Some(restored_parent.id.as_str())
        );
        assert_eq!(
            restored.bookmark_folder_for("https://one.example/"),
            Some(restored_parent.id.as_str())
        );
        assert_eq!(
            restored.bookmark_folder_for("https://two.example/"),
            Some(restored_nested.id.as_str())
        );
    }

    #[test]
    fn bookmark_html_keeps_duplicate_sibling_folders_distinct_and_idempotent() {
        let html = r#"<!DOCTYPE NETSCAPE-Bookmark-file-1>
<DL><p>
<DT><H3>Read later</H3><DL><p><DT><A HREF="https://a.example/">A</A></DL><p>
<DT><H3>Read later</H3><DL><p><DT><A HREF="https://b.example/">B</A></DL><p>
</DL><p>"#;
        let mut library = Library::default();
        assert_eq!(
            library.import_netscape_bookmarks(html).unwrap(),
            BookmarkImportResult {
                bookmarks: 2,
                folders: 2
            }
        );
        let folder_a = library
            .bookmark_folders
            .iter()
            .find(|folder| {
                library.bookmark_folder_for("https://a.example/") == Some(folder.id.as_str())
            })
            .unwrap();
        let folder_b = library
            .bookmark_folders
            .iter()
            .find(|folder| {
                library.bookmark_folder_for("https://b.example/") == Some(folder.id.as_str())
            })
            .unwrap();
        assert_ne!(folder_a.id, folder_b.id);
        assert_eq!(folder_a.name, folder_b.name);

        assert_eq!(
            library.import_netscape_bookmarks(html).unwrap(),
            BookmarkImportResult::default()
        );
        assert_eq!(library.bookmark_folders.len(), 2);

        let mut restored = Library::default();
        assert_eq!(
            restored
                .import_netscape_bookmarks(&library.export_netscape_bookmarks())
                .unwrap(),
            BookmarkImportResult {
                bookmarks: 2,
                folders: 2
            }
        );
        assert_ne!(
            restored.bookmark_folder_for("https://a.example/"),
            restored.bookmark_folder_for("https://b.example/")
        );
    }

    #[test]
    fn bookmark_html_keeps_empty_nested_folders_and_rejects_over_capacity_atomically() {
        let html = r#"<!DOCTYPE NETSCAPE-Bookmark-file-1>
<DL><p><DT><H3>Empty</H3><DL><p>
<DT><H3>Child</H3><DL><p></DL><p>
</DL><p></DL><p>"#;
        let mut library = Library::default();
        assert_eq!(
            library.import_netscape_bookmarks(html).unwrap(),
            BookmarkImportResult {
                bookmarks: 0,
                folders: 2
            }
        );
        let parent = library
            .bookmark_folders
            .iter()
            .find(|folder| folder.name == "Empty")
            .unwrap();
        let child = library
            .bookmark_folders
            .iter()
            .find(|folder| folder.name == "Child")
            .unwrap();
        assert_eq!(child.parent_id.as_deref(), Some(parent.id.as_str()));

        let mut restored = Library::default();
        assert_eq!(
            restored
                .import_netscape_bookmarks(&library.export_netscape_bookmarks())
                .unwrap(),
            BookmarkImportResult {
                bookmarks: 0,
                folders: 2
            }
        );
        assert!(restored.bookmarks.is_empty());
        assert_eq!(restored.bookmark_folders.len(), 2);

        let mut overflow = String::from("<!DOCTYPE NETSCAPE-Bookmark-file-1><DL><p>");
        for index in 0..=MAX_BOOKMARK_FOLDERS {
            overflow.push_str(&format!("<DT><H3>Folder {index}</H3><DL><p></DL><p>"));
        }
        overflow.push_str("</DL><p>");
        let before = restored.clone();
        assert!(restored.import_netscape_bookmarks(&overflow).is_err());
        assert_eq!(restored, before);
    }

    #[test]
    fn bookmark_import_over_capacity_is_rejected_without_partial_changes() {
        let mut library = Library::default();
        for index in 0..MAX_BOOKMARKS {
            library.bookmarks.push(Page {
                url: format!("https://saved-{index}.example/"),
                title: "Saved".into(),
            });
        }
        let before = library.clone();
        let html = r#"<!DOCTYPE NETSCAPE-Bookmark-file-1><DL><p><DT><A HREF="https://new.example/">New</A></DL><p>"#;
        assert!(library.import_netscape_bookmarks(html).is_err());
        assert_eq!(library, before);
    }

    #[test]
    fn adding_bookmark_over_capacity_preserves_existing_items_and_folder_assignments() {
        let mut library = Library::default();
        for index in 0..MAX_BOOKMARKS {
            library.bookmarks.push(Page {
                url: format!("https://saved-{index}.example/"),
                title: "Saved".into(),
            });
        }
        let folder = library.create_bookmark_folder("Keep", None).unwrap();
        assert!(library.move_bookmark_to_folder("https://saved-0.example/", Some(&folder)));
        let before = library.clone();

        assert!(!library.toggle_bookmark("https://new.example/", "New"));
        assert_eq!(library, before);
        assert_eq!(
            library.bookmark_folder_for("https://saved-0.example/"),
            Some(folder.as_str())
        );

        assert!(library.toggle_bookmark("https://saved-0.example/", "Saved"));
        assert_eq!(library.bookmarks.len(), MAX_BOOKMARKS - 1);
        assert_eq!(
            library.bookmark_folder_for("https://saved-0.example/"),
            None
        );
    }

    #[test]
    fn folder_ids_validate_moves_and_delete_subtrees_without_losing_bookmarks() {
        let mut library = Library::default();
        library.toggle_bookmark("https://saved.example/", "Saved");
        assert!(!library.move_bookmark_to_folder("https://saved.example/", Some("missing")));
        let parent = library.create_bookmark_folder("Parent", None).unwrap();
        let child = library
            .create_bookmark_folder("Child", Some(&parent))
            .unwrap();
        assert!(library.move_bookmark_to_folder("https://saved.example/", Some(&child)));
        assert!(!library.delete_bookmark_folder("missing"));
        assert!(library.delete_bookmark_folder(&parent));
        assert!(library.bookmark_folders.is_empty());
        assert_eq!(library.bookmark_folder_for("https://saved.example/"), None);
        assert_eq!(library.bookmarks[0].url, "https://saved.example/");
        assert_eq!(library.bookmark_folder_tombstones.len(), 2);
        assert!(!library
            .create_bookmark_folder("Orphan", Some(&parent))
            .is_some());
    }
}
