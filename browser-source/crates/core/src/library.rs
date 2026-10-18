//! Small, bounded local library. The host never records private-window visits.
use serde::{Deserialize, Serialize};
use std::{io, path::Path};

const MAX_BOOKMARKS: usize = 200;
const MAX_BOOKMARK_TOMBSTONES: usize = 2000;
const MAX_BOOKMARK_IMPORT_BYTES: usize = 8 * 1024 * 1024;
const MAX_HOME_SHORTCUTS: usize = 12;

#[derive(Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Library {
    pub bookmarks: Vec<Page>,
    pub history: Vec<Page>,
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
impl Library {
    pub fn load(path: &Path) -> io::Result<Self> {
        match std::fs::read(path) {
            Ok(bytes) if bytes.len() <= 8 * 1024 * 1024 => {
                serde_json::from_slice(&bytes).map_err(io::Error::other)
            }
            Ok(_) => Err(io::Error::other("Thư viện vượt giới hạn dung lượng")),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error),
        }
    }
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let staging = path.with_extension("json.tmp");
        std::fs::write(
            &staging,
            serde_json::to_vec(self).map_err(io::Error::other)?,
        )?;
        std::fs::rename(staging, path)
    }
    fn page(url: &str, title: &str) -> Option<Page> {
        if !(url.starts_with("https://") || url.starts_with("http://"))
            || url.len() > 8192
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
            self.history.retain(|p| p.url != url);
            self.history.insert(0, page);
            self.history.truncate(500);
        }
    }
    pub fn toggle_bookmark(&mut self, url: &str, title: &str) {
        if self.bookmarks.iter().any(|p| p.url == url) {
            self.remove_bookmark(url);
        } else if let Some(page) = Self::page(url, title) {
            self.removed_bookmarks.retain(|removed| removed != url);
            self.bookmarks.insert(0, page);
            self.bookmarks.truncate(MAX_BOOKMARKS);
        }
    }

    pub fn remove_bookmark(&mut self, url: &str) {
        if self.bookmarks.iter().any(|page| page.url == url) {
            self.bookmarks.retain(|page| page.url != url);
            self.removed_bookmarks.retain(|removed| removed != url);
            self.removed_bookmarks.push(url.to_string());
            if self.removed_bookmarks.len() > MAX_BOOKMARK_TOMBSTONES {
                let excess = self.removed_bookmarks.len() - MAX_BOOKMARK_TOMBSTONES;
                self.removed_bookmarks.drain(..excess);
            }
        }
    }

    pub fn remove_history(&mut self, url: &str) {
        self.history.retain(|page| page.url != url);
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

    pub fn import_netscape_bookmarks(&mut self, html: &str) -> io::Result<usize> {
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

        let mut imported = Vec::new();
        let mut cursor = 0;
        while let Some(relative) = lower[cursor..].find("<a") {
            let start = cursor + relative;
            let after_name = start + 2;
            if lower
                .as_bytes()
                .get(after_name)
                .is_some_and(|byte| !byte.is_ascii_whitespace() && *byte != b'>')
            {
                cursor = after_name;
                continue;
            }
            let Some(tag_end) = find_tag_end(html, after_name) else {
                break;
            };
            let Some(href) = anchor_href(&html[after_name..tag_end]) else {
                cursor = tag_end + 1;
                continue;
            };
            let Some(close_relative) = lower[tag_end + 1..].find("</a") else {
                break;
            };
            let content_start = tag_end + 1;
            let content_end = content_start + close_relative;
            let title = decode_html_entities(&strip_tags(&html[content_start..content_end]));
            let url = decode_html_entities(href.trim());
            if let Some(page) = Self::page(&url, &title) {
                if !self
                    .bookmarks
                    .iter()
                    .any(|existing| existing.url == page.url)
                    && !imported
                        .iter()
                        .any(|existing: &Page| existing.url == page.url)
                {
                    imported.push(page);
                }
            }
            cursor = content_end + 3;
        }

        let available = MAX_BOOKMARKS.saturating_sub(self.bookmarks.len());
        imported.truncate(available);
        let count = imported.len();
        for page in &imported {
            self.removed_bookmarks
                .retain(|removed| removed != &page.url);
        }
        self.bookmarks.splice(0..0, imported);
        Ok(count)
    }

    pub fn export_netscape_bookmarks(&self) -> String {
        let mut html = String::from(
            "<!DOCTYPE NETSCAPE-Bookmark-file-1>\n<META HTTP-EQUIV=\"Content-Type\" CONTENT=\"text/html; charset=UTF-8\">\n<TITLE>Pip Bookmarks</TITLE>\n<H1>Bookmarks</H1>\n<DL><p>\n",
        );
        for page in &self.bookmarks {
            html.push_str("<DT><A HREF=\"");
            html.push_str(&escape_html(&page.url));
            html.push_str("\">");
            html.push_str(&escape_html(&page.title));
            html.push_str("</A>\n");
        }
        html.push_str("</DL><p>\n");
        html
    }
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
        for n in 0..510 {
            library.visit(&format!("https://example.com/{n}"), "Page", false);
        }
        assert_eq!(library.history.len(), 500);
        library.visit("https://example.com/509", "New title", false);
        assert_eq!(library.history.len(), 500);
        assert_eq!(library.history[0].title, "New title");
        library.toggle_bookmark("https://example.com/", "<untrusted>");
        let path = std::env::temp_dir().join(format!("pip-library-{}.json", std::process::id()));
        library.save(&path).unwrap();
        let mut restored = Library::load(&path).unwrap();
        assert_eq!(restored.bookmarks[0].title, "<untrusted>");
        restored.toggle_bookmark("https://example.com/", "Page");
        assert!(restored.bookmarks.is_empty());
        assert_eq!(restored.removed_bookmarks, vec!["https://example.com/"]);
        restored.toggle_bookmark("https://example.com/", "Added again");
        assert!(restored.removed_bookmarks.is_empty());
        restored.toggle_bookmark("https://example.com/", "Page");
        assert_eq!(restored.removed_bookmarks, vec!["https://example.com/"]);
        restored.visit("https://example.com/history", "History", false);
        restored.remove_history("https://example.com/history");
        assert!(!restored
            .history
            .iter()
            .any(|page| page.url == "https://example.com/history"));
        std::fs::remove_file(path).unwrap();
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
        assert_eq!(library.import_netscape_bookmarks(html).unwrap(), 2);
        assert_eq!(library.bookmarks[0].title, "A & B");
        assert_eq!(library.bookmarks[0].url, "https://example.com/?a=1&b=2");
        assert_eq!(library.bookmarks[1].title, "Second 🦋");
        assert_eq!(library.import_netscape_bookmarks(html).unwrap(), 0);

        let exported = library.export_netscape_bookmarks();
        assert!(exported.contains("<!DOCTYPE NETSCAPE-Bookmark-file-1>"));
        assert!(exported.contains("https://example.com/?a=1&amp;b=2"));
        assert!(exported.contains("A &amp; B"));
        let mut restored = Library::default();
        assert_eq!(restored.import_netscape_bookmarks(&exported).unwrap(), 2);
        assert_eq!(restored.bookmarks[0].url, library.bookmarks[0].url);
        assert_eq!(restored.bookmarks[1].title, library.bookmarks[1].title);
    }
}
