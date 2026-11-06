//! Small per-profile store for HTTP(S) page zoom overrides.

use std::collections::HashMap;

const MIN_ZOOM: f64 = 0.25;
const MAX_ZOOM: f64 = 5.0;

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct SiteZoomStore {
    origins: HashMap<String, f64>,
}

impl SiteZoomStore {
    pub(crate) fn from_bytes(bytes: &[u8]) -> Self {
        serde_json::from_slice(bytes).unwrap_or_default()
    }

    pub(crate) fn to_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    pub(crate) fn save(&self, path: &std::path::Path, private: bool) -> std::io::Result<bool> {
        if private {
            return Ok(false);
        }
        let bytes = self
            .to_bytes()
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        super::persist_session_snapshot(path, &bytes)
    }

    pub(crate) fn load(path: &std::path::Path, private: bool) -> Self {
        if private {
            return Self::default();
        }
        std::fs::read(path)
            .map(|bytes| Self::from_bytes(&bytes))
            .unwrap_or_default()
    }

    pub(crate) fn get(&self, origin: &str) -> Option<f64> {
        self.origins.get(origin).copied().map(clamp_zoom)
    }

    pub(crate) fn set(&mut self, origin: String, factor: f64) {
        self.origins.insert(origin, clamp_zoom(factor));
    }

    pub(crate) fn remove(&mut self, origin: &str) {
        self.origins.remove(origin);
    }
}

pub(crate) fn clamp_zoom(factor: f64) -> f64 {
    if factor.is_finite() {
        ((factor * 100.0).round() / 100.0).clamp(MIN_ZOOM, MAX_ZOOM)
    } else {
        1.0
    }
}

/// Return a normalized HTTP(S) origin, omitting paths, credentials, and default ports.
pub(crate) fn origin_for_url(url: &str) -> Option<String> {
    let uri = url.parse::<wry::http::Uri>().ok()?;
    let scheme = uri.scheme_str()?.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let authority = uri.authority()?;
    let host = authority.host();
    if host.is_empty() || host.contains('@') || host.chars().any(char::is_control) {
        return None;
    }
    let host = host.to_ascii_lowercase();
    let formatted_host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host
    };
    let port = authority.port_u16();
    let default_port = match scheme.as_str() {
        "http" => 80,
        "https" => 443,
        _ => unreachable!(),
    };
    Some(match port.filter(|port| *port != default_port) {
        Some(port) => format!("{scheme}://{formatted_host}:{port}"),
        None => format!("{scheme}://{formatted_host}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_are_scheme_host_and_non_default_port_scoped() {
        assert_eq!(
            origin_for_url("https://User:pass@Example.COM:443/a?q=1#x").as_deref(),
            Some("https://example.com")
        );
        assert_eq!(
            origin_for_url("http://example.com:8080/path").as_deref(),
            Some("http://example.com:8080")
        );
        assert_ne!(
            origin_for_url("http://example.com").as_deref(),
            origin_for_url("https://example.com").as_deref()
        );
        assert_ne!(
            origin_for_url("https://example.com").as_deref(),
            origin_for_url("https://example.com:8443").as_deref()
        );
        assert_eq!(origin_for_url("evergreen://newtab"), None);
        assert_eq!(origin_for_url("file:///tmp/page.html"), None);
    }

    #[test]
    fn site_zoom_values_are_bounded_and_round_trip_per_origin() {
        let mut store = SiteZoomStore::default();
        store.set("https://example.com".into(), 9.0);
        store.set("http://other.test".into(), f64::NAN);
        let loaded = SiteZoomStore::from_bytes(&store.to_bytes().unwrap());
        assert_eq!(loaded.get("https://example.com"), Some(5.0));
        assert_eq!(loaded.get("http://other.test"), Some(1.0));
        assert_eq!(loaded.get("https://missing.test"), None);
    }

    #[test]
    fn private_zoom_is_not_loaded_or_written_to_profile_storage() {
        let path =
            std::env::temp_dir().join(format!("pip-private-site-zoom-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut store = SiteZoomStore::default();
        store.set("https://private.test".into(), 1.5);
        assert!(!store.save(&path, true).unwrap());
        assert!(!path.exists());
        assert_eq!(
            SiteZoomStore::load(&path, true).get("https://private.test"),
            None
        );
    }

    #[test]
    fn profile_zoom_files_remain_independent() {
        let root =
            std::env::temp_dir().join(format!("pip-site-zoom-profiles-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let profile_a = root.join("A").join("site-zoom.json");
        let profile_b = root.join("B").join("site-zoom.json");
        let mut store_a = SiteZoomStore::default();
        store_a.set("https://example.com".into(), 1.25);
        let mut store_b = SiteZoomStore::default();
        store_b.set("https://example.com".into(), 1.5);
        store_a.save(&profile_a, false).unwrap();
        store_b.save(&profile_b, false).unwrap();
        assert_eq!(
            SiteZoomStore::load(&profile_a, false).get("https://example.com"),
            Some(1.25)
        );
        assert_eq!(
            SiteZoomStore::load(&profile_b, false).get("https://example.com"),
            Some(1.5)
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
