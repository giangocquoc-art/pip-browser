use serde::{Deserialize, Serialize};

/// Unique identifier for an open browser tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TabId(pub u64);

/// Current lifecycle status of a tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TabStatus {
    /// Actively visible in the main viewport
    Active,
    /// In background, rendered but not visible
    Inactive,
    /// Suspended via TrySuspendAsync to reclaim memory
    Suspended,
    /// Renderer released under memory pressure; selecting the tab reloads its URL.
    Discarded,
    /// Encountered a crash/render failure
    Crashed,
}

/// Metadata and state for an individual tab.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TabState {
    pub id: TabId,
    pub url: String,
    pub title: String,
    pub favicon_uri: Option<String>,
    pub is_loading: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub is_audio_playing: bool,
    pub is_muted: bool,
    #[serde(default)]
    pub pinned: bool,
    /// Prevent automatic suspension while the user needs this tab to stay active.
    #[serde(default)]
    pub keep_awake: bool,
    /// Runtime-only conservative guard for pages that may contain unsaved state.
    #[serde(skip)]
    pub memory_discard_protected: bool,
    /// Optional visual grouping metadata, persisted with the tab session.
    #[serde(default)]
    pub group: Option<TabGroup>,
    pub status: TabStatus,
    pub last_active_timestamp_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabGroup {
    pub name: String,
    pub color: String,
}

/// Pure state manager for all tabs inside a single window.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TabManager {
    tabs: Vec<TabState>,
    active_tab_id: Option<TabId>,
    next_id: u64,
    closed_history: Vec<String>, // Recent URLs are persisted with the profile session.
}

impl TabManager {
    pub fn new() -> Self {
        Self {
            tabs: Vec::new(),
            active_tab_id: None,
            next_id: 1,
            closed_history: Vec::new(),
        }
    }

    pub fn create_tab(&mut self, url: &str, current_time_secs: u64) -> TabId {
        let id = TabId(self.next_id);
        self.next_id += 1;

        let new_tab = TabState {
            id,
            url: url.to_string(),
            title: "New Tab".to_string(),
            favicon_uri: None,
            is_loading: false,
            can_go_back: false,
            can_go_forward: false,
            is_audio_playing: false,
            is_muted: false,
            pinned: false,
            keep_awake: false,
            memory_discard_protected: false,
            group: None,
            status: TabStatus::Active,
            last_active_timestamp_secs: current_time_secs,
        };

        // If another tab was active, mark it inactive
        if let Some(prev_active_id) = self.active_tab_id {
            if let Some(prev) = self.tabs.iter_mut().find(|t| t.id == prev_active_id) {
                prev.status = TabStatus::Inactive;
            }
        }

        self.tabs.push(new_tab);
        self.active_tab_id = Some(id);
        id
    }

    pub fn close_tab(&mut self, id: TabId) -> Option<TabId> {
        if let Some(pos) = self.tabs.iter().position(|t| t.id == id) {
            let removed = self.tabs.remove(pos);
            if is_reopenable_url(&removed.url) && removed.url != "evergreen://newtab" {
                self.closed_history.push(removed.url);
                if self.closed_history.len() > 50 {
                    self.closed_history.remove(0);
                }
            }

            if self.active_tab_id == Some(id) {
                if self.tabs.is_empty() {
                    self.active_tab_id = None;
                } else {
                    let next_pos = if pos >= self.tabs.len() {
                        self.tabs.len() - 1
                    } else {
                        pos
                    };
                    let next_id = self.tabs[next_pos].id;
                    self.tabs[next_pos].status = TabStatus::Active;
                    self.active_tab_id = Some(next_id);
                }
            }
        }
        self.active_tab_id
    }

    pub fn switch_tab(&mut self, id: TabId, current_time_secs: u64) -> bool {
        let exists = self.tabs.iter().any(|t| t.id == id);
        if !exists {
            return false;
        }

        for tab in &mut self.tabs {
            if tab.id == id {
                tab.status = TabStatus::Active;
                tab.last_active_timestamp_secs = current_time_secs;
                tab.memory_discard_protected = false;
            } else if tab.status == TabStatus::Active {
                tab.status = TabStatus::Inactive;
            }
        }
        self.active_tab_id = Some(id);
        true
    }

    pub fn active_tab(&self) -> Option<&TabState> {
        self.active_tab_id
            .and_then(|id| self.tabs.iter().find(|t| t.id == id))
    }

    pub fn active_tab_id(&self) -> Option<TabId> {
        self.active_tab_id
    }

    /// Return the tab adjacent to the active tab, wrapping at either end.
    pub fn adjacent_tab_id(&self, forward: bool) -> Option<TabId> {
        if self.tabs.len() < 2 {
            return None;
        }
        let current = self
            .tabs
            .iter()
            .position(|tab| Some(tab.id) == self.active_tab_id)?;
        let next = if forward {
            (current + 1) % self.tabs.len()
        } else if current == 0 {
            self.tabs.len() - 1
        } else {
            current - 1
        };
        Some(self.tabs[next].id)
    }

    pub fn tabs(&self) -> &[TabState] {
        &self.tabs
    }

    pub fn tabs_mut(&mut self) -> &mut [TabState] {
        &mut self.tabs
    }

    pub fn update_title(&mut self, id: TabId, title: String) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
            tab.title = title;
        }
    }

    pub fn update_url(&mut self, id: TabId, url: String) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
            tab.url = url;
            tab.memory_discard_protected = false;
        }
    }

    pub fn update_favicon(&mut self, id: TabId, favicon_uri: Option<String>) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
            tab.favicon_uri = favicon_uri;
        }
    }

    pub fn update_history_state(&mut self, id: TabId, can_go_back: bool, can_go_forward: bool) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
            tab.can_go_back = can_go_back;
            tab.can_go_forward = can_go_forward;
        }
    }

    pub fn set_tab_suspended(&mut self, id: TabId, suspended: bool) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
            tab.status = if suspended {
                TabStatus::Suspended
            } else {
                TabStatus::Inactive
            };
        }
    }

    pub fn set_tab_discarded(&mut self, id: TabId) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
            tab.status = TabStatus::Discarded;
            tab.can_go_back = false;
            tab.can_go_forward = false;
            tab.is_audio_playing = false;
            tab.is_muted = false;
        }
    }

    pub fn set_memory_discard_protected(&mut self, id: TabId, protected: bool) {
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) {
            tab.memory_discard_protected = protected;
        }
    }

    pub fn set_audio_playing(&mut self, id: TabId, playing: bool) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
            tab.is_audio_playing = playing;
        }
    }

    pub fn toggle_pin(&mut self, id: TabId) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
            tab.pinned = !tab.pinned;
            self.tabs.sort_by_key(|t| !t.pinned);
        }
    }

    pub fn toggle_keep_awake(&mut self, id: TabId) -> Option<bool> {
        let tab = self.tabs.iter_mut().find(|tab| tab.id == id)?;
        tab.keep_awake = !tab.keep_awake;
        Some(tab.keep_awake)
    }

    /// Assign a tab to a named group, or remove it from its group when `name` is empty.
    /// Returns false if the tab doesn't exist or the group name is invalid.
    pub fn set_group(&mut self, id: TabId, name: &str) -> bool {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) else {
            return false;
        };
        let name = name.trim();
        if name.is_empty() {
            tab.group = None;
            return true;
        }
        if name.chars().count() > 24 || name.chars().any(char::is_control) {
            return false;
        }
        const COLORS: [&str; 8] = [
            "#e85d0d", "#3b82f6", "#8b5cf6", "#0f9d8a", "#d9467a", "#ca8a04", "#0891b2", "#64748b",
        ];
        let hash = name.to_lowercase().chars().fold(0usize, |hash, character| {
            hash.wrapping_mul(31).wrapping_add(character as usize)
        });
        tab.group = Some(TabGroup {
            name: name.to_string(),
            color: COLORS[hash % COLORS.len()].to_string(),
        });
        true
    }

    pub fn set_muted(&mut self, id: TabId, muted: bool) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
            tab.is_muted = muted;
        }
    }

    pub fn pop_last_closed(&mut self) -> Option<String> {
        self.closed_history.pop()
    }

    pub fn clear_closed_history(&mut self) {
        self.closed_history.clear();
    }

    /// Returns reopenable recently closed URLs newest-first for lightweight UI pickers.
    pub fn recently_closed_newest_first(&self) -> Vec<String> {
        self.closed_history
            .iter()
            .rev()
            .filter(|url| is_reopenable_url(url))
            .take(50)
            .cloned()
            .collect()
    }

    /// Removes one item by its newest-first display index, rejecting invalid or unsafe entries.
    pub fn take_closed_at_newest_index(&mut self, index: usize) -> Option<String> {
        let position = self
            .closed_history
            .len()
            .checked_sub(index.checked_add(1)?)?;
        let url = self.closed_history.get(position)?;
        if !is_reopenable_url(url) {
            return None;
        }
        Some(self.closed_history.remove(position))
    }

    pub fn closed_history_snapshot(&self) -> Vec<String> {
        self.closed_history.clone()
    }

    pub fn restore_closed_history(&mut self, urls: Vec<String>) {
        self.closed_history = urls
            .into_iter()
            .filter(|url| is_reopenable_url(url))
            .rev()
            .take(50)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
    }

    pub fn reorder_tab(&mut self, from_idx: usize, to_idx: usize) -> bool {
        if from_idx < self.tabs.len() && to_idx < self.tabs.len() && from_idx != to_idx {
            if self.tabs[from_idx].pinned != self.tabs[to_idx].pinned {
                return false;
            }
            let tab = self.tabs.remove(from_idx);
            self.tabs.insert(to_idx, tab);
            true
        } else {
            false
        }
    }

    pub fn get_tab_by_id(&self, id: TabId) -> Option<&TabState> {
        self.tabs.iter().find(|t| t.id == id)
    }

    /// Extract a tab from this manager (for moving to another window).
    pub fn extract_tab(&mut self, id: TabId) -> Option<TabState> {
        if let Some(pos) = self.tabs.iter().position(|t| t.id == id) {
            let tab = self.tabs.remove(pos);
            if self.active_tab_id == Some(id) {
                if self.tabs.is_empty() {
                    self.active_tab_id = None;
                } else {
                    let next_pos = if pos >= self.tabs.len() {
                        self.tabs.len() - 1
                    } else {
                        pos
                    };
                    let next_id = self.tabs[next_pos].id;
                    self.tabs[next_pos].status = TabStatus::Active;
                    self.active_tab_id = Some(next_id);
                }
            }
            Some(tab)
        } else {
            None
        }
    }

    /// Insert an existing tab state into this manager (e.g. from another window).
    pub fn insert_tab(&mut self, mut tab: TabState, at_index: Option<usize>) -> TabId {
        let id = if tab.id.0 > 0 {
            self.next_id = self.next_id.max(tab.id.0 + 1);
            tab.id
        } else {
            let new_id = TabId(self.next_id);
            self.next_id += 1;
            tab.id = new_id;
            new_id
        };
        tab.status = TabStatus::Active;

        if let Some(prev_active) = self.active_tab_id {
            if let Some(prev) = self.tabs.iter_mut().find(|t| t.id == prev_active) {
                prev.status = TabStatus::Inactive;
            }
        }

        let idx = at_index.unwrap_or(self.tabs.len()).min(self.tabs.len());
        self.tabs.insert(idx, tab);
        self.active_tab_id = Some(id);
        id
    }

    pub fn mark_tab_crashed(&mut self, id: TabId) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
            tab.status = TabStatus::Crashed;
        }
    }

    pub fn snapshot(&self) -> Vec<TabState> {
        self.tabs
            .iter()
            .filter(|tab| !url_has_userinfo(&tab.url))
            .cloned()
            .collect()
    }

    pub fn restore_from_snapshot(&mut self, tabs: Vec<TabState>) {
        // Session files written by older builds may contain userinfo in URLs.
        // Drop those tabs before callers recreate WebViews from the snapshot.
        self.tabs = tabs
            .into_iter()
            .filter(|tab| !url_has_userinfo(&tab.url))
            .collect();
        // A stale or partially-written session can contain multiple Active tabs,
        // or none at all. Keep the manager's active id and the tab statuses in
        // sync so callers never restore two foreground tabs (or an inactive
        // foreground tab).
        let active_index = self
            .tabs
            .iter()
            .position(|tab| tab.status == TabStatus::Active)
            .or_else(|| (!self.tabs.is_empty()).then_some(0));
        self.active_tab_id = active_index.map(|index| self.tabs[index].id);
        if let Some(active_index) = active_index {
            for (index, tab) in self.tabs.iter_mut().enumerate() {
                if index == active_index {
                    tab.status = TabStatus::Active;
                } else if tab.status == TabStatus::Active {
                    tab.status = TabStatus::Inactive;
                }
            }
        }
        if let Some(max_id) = self.tabs.iter().map(|t| t.id.0).max() {
            self.next_id = max_id + 1;
        }
    }
}

fn is_reopenable_url(url: &str) -> bool {
    if url.len() > 8192 {
        return false;
    }
    if url == "evergreen://newtab" {
        return true;
    }
    let Some((scheme, _)) = url.split_once("://") else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return false;
    }
    // Userinfo can embed credentials in the URL. Closed-tab history is
    // persisted in the profile session, so never retain those URLs there.
    !url_has_userinfo(url)
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

/// Normalize an omnibox input string into a valid HTTP/HTTPS URL or search engine query.
pub fn normalize_url(input: &str, search_template: &str) -> Result<String, String> {
    let trimmed = input.trim();
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("pip://") {
        return Ok(format!("evergreen://{}", &trimmed[6..]));
    }
    if trimmed.is_empty() || lower == "about:home" || lower == "about:newtab" {
        return Ok("evergreen://newtab".to_string());
    }

    if lower == "about:blank" {
        return Ok("about:blank".to_string());
    }

    if lower == "about:settings" {
        return Ok("evergreen://settings".to_string());
    }

    // Internal browser scheme
    if lower.starts_with("evergreen://") {
        return Ok(trimmed.to_string());
    }

    // Explicit valid scheme
    if lower.starts_with("https://") || lower.starts_with("http://") {
        return Ok(trimmed.to_string());
    }

    // Disallowed schemes (security check - case-insensitive)
    if lower.starts_with("javascript:")
        || lower.starts_with("file:")
        || lower.starts_with("data:")
        || lower.starts_with("vbscript:")
        || lower.starts_with("about:")
    {
        return Err(format!("Navigation to scheme prohibited: {}", trimmed));
    }

    // Hostname check: contains dot and no spaces
    if trimmed.contains('.') && !trimmed.contains(' ') {
        return Ok(format!("https://{}", trimmed));
    }

    // Fallback: search query
    let encoded_query = url_encode(trimmed);
    Ok(search_template.replace("%s", &encoded_query))
}

/// Simple percent encoder for search queries
pub fn url_encode(input: &str) -> String {
    let mut result = String::new();
    for byte in input.bytes() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(byte as char);
            }
            b' ' => result.push('+'),
            _ => {
                result.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{is_reopenable_url, TabManager, TabStatus};

    fn tab_with_status(manager: &mut TabManager, url: &str, status: TabStatus) -> super::TabId {
        let id = manager.create_tab(url, 0);
        manager
            .tabs_mut()
            .iter_mut()
            .find(|tab| tab.id == id)
            .unwrap()
            .status = status;
        id
    }

    #[test]
    fn restore_snapshot_normalizes_multiple_active_tabs() {
        let mut source = TabManager::new();
        let first = tab_with_status(&mut source, "https://first.example", TabStatus::Active);
        let second = tab_with_status(&mut source, "https://second.example", TabStatus::Active);
        source
            .tabs_mut()
            .iter_mut()
            .find(|tab| tab.id == first)
            .unwrap()
            .status = TabStatus::Active;

        let mut restored = TabManager::new();
        restored.restore_from_snapshot(source.snapshot());

        assert_eq!(restored.active_tab_id(), Some(first));
        assert_eq!(
            restored.get_tab_by_id(first).unwrap().status,
            TabStatus::Active
        );
        assert_eq!(
            restored.get_tab_by_id(second).unwrap().status,
            TabStatus::Inactive
        );
        assert_eq!(
            restored
                .tabs()
                .iter()
                .filter(|tab| tab.status == TabStatus::Active)
                .count(),
            1
        );
    }

    #[test]
    fn restore_snapshot_activates_fallback_when_saved_status_has_no_active_tab() {
        let mut source = TabManager::new();
        let first = tab_with_status(&mut source, "https://first.example", TabStatus::Suspended);
        tab_with_status(&mut source, "https://second.example", TabStatus::Inactive);

        let mut restored = TabManager::new();
        restored.restore_from_snapshot(source.snapshot());

        assert_eq!(restored.active_tab_id(), Some(first));
        assert_eq!(
            restored.get_tab_by_id(first).unwrap().status,
            TabStatus::Active
        );
        assert_eq!(
            restored
                .tabs()
                .iter()
                .filter(|tab| tab.status == TabStatus::Active)
                .count(),
            1
        );
    }

    #[test]
    fn closed_tab_history_rejects_urls_with_credentials() {
        let mut manager = TabManager::new();
        for url in [
            "https://alice:secret@example.com/path",
            "http://user@example.com/",
        ] {
            let id = manager.create_tab(url, 0);
            manager.close_tab(id);
        }

        assert!(manager.recently_closed_newest_first().is_empty());
        assert!(!is_reopenable_url("https://alice:secret@example.com/path"));
        assert!(!is_reopenable_url("http://user@example.com/"));
        assert!(is_reopenable_url(
            "https://example.com/path?contact=a@b.test"
        ));
    }

    #[test]
    fn restoring_legacy_closed_history_drops_urls_with_credentials() {
        let mut manager = TabManager::new();
        manager.restore_closed_history(vec![
            "https://safe.example/path".into(),
            "https://alice:secret@example.com/path".into(),
            "http://user@example.com/".into(),
        ]);

        assert_eq!(
            manager.recently_closed_newest_first(),
            vec!["https://safe.example/path"]
        );
    }

    #[test]
    fn session_snapshots_do_not_persist_or_restore_urls_with_credentials() {
        let mut manager = TabManager::new();
        manager.create_tab("https://alice:secret@example.com/", 0);
        manager.create_tab("HTTPS://bob:secret@example.net/", 0);
        manager.create_tab("https://safe.example/", 0);

        let snapshot = manager.snapshot();
        let serialized = serde_json::to_string(&snapshot).unwrap();
        assert!(!serialized.contains("secret"));
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].url, "https://safe.example/");

        // Legacy session files are passed through restore_from_snapshot too.
        let legacy: Vec<_> = [
            "https://alice:secret@example.com/",
            "http://bob:secret@example.net/",
            "https://safe.example/",
        ]
        .into_iter()
        .map(|url| {
            let mut source = TabManager::new();
            source.create_tab("https://placeholder.example/", 0);
            let mut tab = source.snapshot().into_iter().next().unwrap();
            tab.url = url.into();
            tab
        })
        .collect();
        let mut restored = TabManager::new();
        restored.restore_from_snapshot(legacy);
        assert_eq!(restored.tabs().len(), 1);
        assert_eq!(restored.tabs()[0].url, "https://safe.example/");
    }
}
