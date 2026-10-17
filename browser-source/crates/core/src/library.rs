//! Small, bounded local library. The host never records private-window visits.
use serde::{Deserialize, Serialize};
use std::{io, path::Path};

const MAX_BOOKMARKS: usize = 200;
const MAX_BOOKMARK_IMPORT_BYTES: usize = 8 * 1024 * 1024;

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Library {
    pub bookmarks: Vec<Page>,
    pub history: Vec<Page>,
}
#[derive(Clone, Serialize, Deserialize)]
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
            self.bookmarks.retain(|p| p.url != url);
        } else if let Some(page) = Self::page(url, title) {
            self.bookmarks.insert(0, page);
            self.bookmarks.truncate(MAX_BOOKMARKS);
        }
    }

    pub fn remove_history(&mut self, url: &str) {
        self.history.retain(|page| page.url != url);
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
        restored.visit("https://example.com/history", "History", false);
        restored.remove_history("https://example.com/history");
        assert!(!restored
            .history
            .iter()
            .any(|page| page.url == "https://example.com/history"));
        std::fs::remove_file(path).unwrap();
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
