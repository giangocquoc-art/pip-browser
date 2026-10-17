//! Small, bounded local library. The host never records private-window visits.
use serde::{Deserialize, Serialize};
use std::{io, path::Path};

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
        if !(url.starts_with("https://") || url.starts_with("http://")) || url.len() > 8192 {
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
            self.bookmarks.truncate(200);
        }
    }

    pub fn remove_history(&mut self, url: &str) {
        self.history.retain(|page| page.url != url);
    }
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
}
