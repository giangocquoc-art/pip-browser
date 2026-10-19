use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Application settings persisted to `%APPDATA%\<app>\settings.json` or local directory in portable mode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    #[serde(default = "default_profiles")]
    pub profiles: Vec<BrowserProfile>,
    #[serde(default)]
    pub google_dns: bool,
    /// Ad blocking posture. Balanced preserves current filtering behavior by default.
    #[serde(default)]
    pub adblock_mode: AdblockMode,
    /// Engine settings
    pub engine: EngineSettings,
    /// Privacy and ephemerality settings
    pub privacy: PrivacySettings,
    /// Performance and tab-sleep settings
    pub performance: PerformanceSettings,
    /// Appearance and UI theming
    pub appearance: AppearanceSettings,
    /// Default search engine ("google", "bing", "brave", "ecosia")
    pub search_engine: String,
    /// Download behavior
    pub downloads: DownloadSettings,
    /// Tab behavior and dragging
    pub tabs: TabSettings,
    /// Site permissions
    pub permissions: PermissionSettings,
    /// App update commands
    pub updates: UpdateSettings,
    /// Pluggable feature toggles
    #[serde(default)]
    pub features: FeatureFlags,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "BrowserProfileData")]
pub struct BrowserProfile {
    pub id: String,
    pub name: String,
    #[serde(default = "enabled_by_default")]
    pub autofill: bool,
    /// Enables WebView2's built-in Save/Update Password prompt and password suggestions.
    pub password_autosave: bool,
    /// DPAPI-protected Pip Sync credentials local to this Windows account.
    #[serde(default)]
    pub sync_credentials_protected: Option<String>,
    /// Optional browser history sync; disabled for profiles upgraded from V1.
    #[serde(default)]
    pub sync_history: bool,
    /// Optional cross-device open-tab list; private windows are never included.
    #[serde(default)]
    pub sync_open_tabs: bool,
    /// Optional allowlisted UI preferences sync.
    #[serde(default)]
    pub sync_settings: bool,
    /// Explicit opt-in for syncing Pip's separately DPAPI-protected password vault.
    #[serde(default)]
    pub sync_passwords: bool,
    /// Random local identifier for this installation, never derived from a URL/profile name.
    #[serde(default)]
    pub sync_device_id: Option<String>,
    /// Local conflict metadata for the allowlisted Sync settings category.
    #[serde(default)]
    pub sync_settings_updated_at_ms: u64,
    #[serde(default)]
    pub sync_settings_fingerprint: Option<String>,
    /// Profile-local values for the nine allowlisted Sync preferences. `None` keeps
    /// legacy profiles on the existing global values until they are changed/synced.
    #[serde(default)]
    pub sync_settings_values: Option<ProfileSyncSettings>,
}

/// The exact set of preferences that Sync V4 is allowed to move between devices.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProfileSyncSettings {
    pub search_engine: String,
    pub theme: String,
    pub density: String,
    pub default_zoom_level: f64,
    pub show_status_preview: bool,
    pub show_zoom_badge: bool,
    pub enable_tab_reordering: bool,
    pub enable_tab_tearoff: bool,
    pub warn_on_close_tabs: bool,
}

impl Default for ProfileSyncSettings {
    fn default() -> Self {
        Self {
            search_engine: "google".into(),
            theme: "system".into(),
            density: "comfortable".into(),
            default_zoom_level: 1.0,
            show_status_preview: true,
            show_zoom_badge: true,
            enable_tab_reordering: true,
            enable_tab_tearoff: true,
            warn_on_close_tabs: true,
        }
    }
}

impl ProfileSyncSettings {
    pub fn from_global(settings: &Settings) -> Self {
        Self {
            search_engine: settings.search_engine.clone(),
            theme: settings.appearance.theme.clone(),
            density: settings.appearance.density.clone(),
            default_zoom_level: settings.appearance.default_zoom_level,
            show_status_preview: settings.appearance.show_status_preview,
            show_zoom_badge: settings.appearance.show_zoom_badge,
            enable_tab_reordering: settings.tabs.enable_tab_reordering,
            enable_tab_tearoff: settings.tabs.enable_tab_tearoff,
            warn_on_close_tabs: settings.tabs.warn_on_close_tabs,
        }
    }

    pub fn apply_to(&self, settings: &mut Settings) {
        settings.search_engine = self.search_engine.clone();
        settings.appearance.theme = self.theme.clone();
        settings.appearance.density = self.density.clone();
        settings.appearance.default_zoom_level = self.default_zoom_level;
        settings.appearance.show_status_preview = self.show_status_preview;
        settings.appearance.show_zoom_badge = self.show_zoom_badge;
        settings.tabs.enable_tab_reordering = self.enable_tab_reordering;
        settings.tabs.enable_tab_tearoff = self.enable_tab_tearoff;
        settings.tabs.warn_on_close_tabs = self.warn_on_close_tabs;
    }
}

#[derive(Deserialize)]
struct BrowserProfileData {
    id: String,
    name: String,
    #[serde(default = "enabled_by_default")]
    autofill: bool,
    /// Old settings used one toggle for both features; preserve its choice on upgrade.
    password_autosave: Option<bool>,
    #[serde(default)]
    sync_credentials_protected: Option<String>,
    #[serde(default)]
    sync_history: bool,
    #[serde(default)]
    sync_open_tabs: bool,
    #[serde(default)]
    sync_settings: bool,
    #[serde(default)]
    sync_passwords: bool,
    #[serde(default)]
    sync_device_id: Option<String>,
    #[serde(default)]
    sync_settings_updated_at_ms: u64,
    /// Legacy local conflict time, stored in seconds by pre-V3 Pip builds.
    #[serde(default)]
    sync_settings_updated_at_secs: u64,
    #[serde(default)]
    sync_settings_fingerprint: Option<String>,
    #[serde(default)]
    sync_settings_values: Option<ProfileSyncSettings>,
}

impl From<BrowserProfileData> for BrowserProfile {
    fn from(data: BrowserProfileData) -> Self {
        Self {
            id: data.id,
            name: data.name,
            autofill: data.autofill,
            password_autosave: data.password_autosave.unwrap_or(data.autofill),
            sync_credentials_protected: data.sync_credentials_protected,
            sync_history: data.sync_history,
            sync_open_tabs: data.sync_open_tabs,
            sync_settings: data.sync_settings,
            sync_passwords: data.sync_passwords,
            sync_device_id: data.sync_device_id,
            sync_settings_updated_at_ms: if data.sync_settings_updated_at_ms > 0 {
                data.sync_settings_updated_at_ms
            } else {
                data.sync_settings_updated_at_secs.saturating_mul(1_000)
            },
            sync_settings_fingerprint: data.sync_settings_fingerprint,
            sync_settings_values: data.sync_settings_values,
        }
    }
}

fn enabled_by_default() -> bool {
    true
}

pub fn valid_profile_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

pub fn default_profiles() -> Vec<BrowserProfile> {
    vec![BrowserProfile {
        id: "Default".into(),
        name: "Cá nhân".into(),
        autofill: true,
        password_autosave: true,
        sync_credentials_protected: None,
        sync_history: false,
        sync_open_tabs: false,
        sync_settings: false,
        sync_passwords: false,
        sync_device_id: None,
        sync_settings_updated_at_ms: 0,
        sync_settings_fingerprint: None,
        sync_settings_values: None,
    }]
}

pub fn valid_profile_name(name: &str) -> bool {
    !name.trim().is_empty() && name.chars().count() <= 40 && !name.chars().any(char::is_control)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EngineSettings {
    /// Command to run when the user triggers "Update engine now"
    pub update_command: String,
    /// Automatically check for engine updates
    pub auto_check: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrivacySettings {
    /// If true, tabs run in ephemeral/private mode by default (no history or persistent cookies)
    pub ephemeral_default: bool,
    /// Domains explicitly allowed to retain cookies/session
    pub persistent_sites: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PerformanceSettings {
    /// Duration of inactivity in seconds before suspending a background tab (default: 300s = 5m)
    pub sleep_after_secs: u64,
    /// Automatically suspend idle background tabs to reduce CPU and memory usage
    pub background_throttling: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppearanceSettings {
    /// Theme preference: "system", "light", "dark"
    pub theme: String,
    /// UI density: "comfortable", "compact"
    pub density: String,
    /// Default page zoom level (e.g. 1.0 = 100%)
    pub default_zoom_level: f64,
    /// Show zoom indicator badge on address bar when zoomed
    pub show_zoom_badge: bool,
    /// Enable interactive link preview card (Peek)
    pub enable_link_preview: bool,
    /// Show destination URL in bottom status bar on hover
    pub show_status_preview: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DownloadSettings {
    /// Prompt for save destination on each download
    pub ask_where_to_save: bool,
    /// Show download progress ring in toolbar
    pub show_progress_toolbar: bool,
    /// Default download folder
    pub default_folder: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TabSettings {
    /// Enable tab drag-and-drop horizontal reordering
    pub enable_tab_reordering: bool,
    /// Enable detaching tab into a new window when dragged outside the tab strip
    pub enable_tab_tearoff: bool,
    /// Confirm before closing multiple tabs
    pub warn_on_close_tabs: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PermissionSettings {
    /// Location permission policy: "ask", "allow", "block"
    pub location: String,
    /// Camera permission policy: "ask", "allow", "block"
    pub camera: String,
    /// Microphone permission policy: "ask", "allow", "block"
    pub microphone: String,
    /// Notifications permission policy: "ask", "allow", "block"
    pub notifications: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdblockMode {
    #[default]
    Balanced,
    Strong,
    VideoFirst,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateSettings {
    /// Check for app updates on startup
    pub auto_check_app_updates: bool,
    /// Local rebuild/update command for developer and fork builds
    pub fork_update_command: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureFlags {
    /// Enable Find-in-Page search widget (Ctrl+F)
    pub enable_find_in_page: bool,
    /// Enable Downloads management, shelf, and confirmation prompts
    pub enable_downloads_manager: bool,
    /// Enable link destination status tooltip and Peek preview
    pub enable_link_preview: bool,
    /// Enable tab drag reordering and window tear-off
    pub enable_tab_gestures: bool,
    /// Enable custom zoom badge and controls
    pub enable_zoom_controls: bool,
    /// Enable site permissions prompt bar
    pub enable_permissions_prompt: bool,
}

impl Default for FeatureFlags {
    fn default() -> Self {
        Self {
            enable_find_in_page: true,
            enable_downloads_manager: true,
            enable_link_preview: true,
            enable_tab_gestures: true,
            enable_zoom_controls: true,
            enable_permissions_prompt: true,
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        let default_download_dir = dirs_fallback_downloads();
        Self {
            profiles: default_profiles(),
            google_dns: false,
            adblock_mode: AdblockMode::Balanced,
            engine: EngineSettings {
                update_command: "powershell -NoProfile -Command \"$u='https://go.microsoft.com/fwlink/p/?LinkId=2124703'; $o=\\\"$env:TEMP\\MicrosoftEdgeWebview2Setup.exe\\\"; Invoke-WebRequest -Uri $u -OutFile $o; Start-Process -FilePath $o -ArgumentList '/silent','/install' -Wait\"".to_string(),
                auto_check: true,
            },
            privacy: PrivacySettings {
                ephemeral_default: true,
                persistent_sites: Vec::new(),
            },
            performance: PerformanceSettings {
                sleep_after_secs: 300,
                background_throttling: true,
            },
            appearance: AppearanceSettings {
                theme: "system".to_string(),
                density: "comfortable".to_string(),
                default_zoom_level: 1.0,
                show_zoom_badge: true,
                enable_link_preview: true,
                show_status_preview: true,
            },
            search_engine: "google".to_string(),
            downloads: DownloadSettings {
                ask_where_to_save: true,
                show_progress_toolbar: true,
                default_folder: default_download_dir,
            },
            tabs: TabSettings {
                enable_tab_reordering: true,
                enable_tab_tearoff: true,
                warn_on_close_tabs: true,
            },
            permissions: PermissionSettings {
                location: "ask".to_string(),
                camera: "ask".to_string(),
                microphone: "ask".to_string(),
                notifications: "ask".to_string(),
            },
            updates: UpdateSettings {
                auto_check_app_updates: false,
                fork_update_command: "git pull && cargo build --release".to_string(),
            },
            features: FeatureFlags::default(),
        }
    }
}

impl Settings {
    /// Return this profile's effective settings, falling back to the legacy global
    /// values until the profile has its own Sync-preference snapshot.
    pub fn for_profile(&self, profile_id: &str) -> Self {
        let mut effective = self.clone();
        if let Some(values) = self
            .profiles
            .iter()
            .find(|profile| profile.id == profile_id)
            .and_then(|profile| profile.sync_settings_values.as_ref())
        {
            values.apply_to(&mut effective);
        }
        effective
    }

    pub fn profile_sync_settings(&self, profile_id: &str) -> Option<ProfileSyncSettings> {
        let profile = self
            .profiles
            .iter()
            .find(|profile| profile.id == profile_id)?;
        Some(
            profile
                .sync_settings_values
                .clone()
                .unwrap_or_else(|| ProfileSyncSettings::from_global(self)),
        )
    }

    pub fn search_url_template(&self) -> &'static str {
        match self.search_engine.to_lowercase().as_str() {
            "google" => "https://www.google.com/search?q=%s",
            "bing" => "https://www.bing.com/search?q=%s",
            "brave" => "https://search.brave.com/search?q=%s",
            "ecosia" => "https://www.ecosia.org/search?q=%s",
            _ => "https://www.google.com/search?q=%s",
        }
    }

    pub fn search_engine_display_name(&self) -> &'static str {
        match self.search_engine.to_lowercase().as_str() {
            "google" => "Google",
            "bing" => "Bing",
            "brave" => "Brave",
            "ecosia" => "Ecosia",
            _ => "Google",
        }
    }

    /// Returns true if the given URL or domain is whitelisted for persistent session storage.
    pub fn is_site_persistent(&self, url_or_domain: &str) -> bool {
        if !self.privacy.ephemeral_default {
            return true;
        }
        let domain = extract_domain(url_or_domain);
        if domain.is_empty() {
            return false;
        }
        self.privacy.persistent_sites.iter().any(|allowed| {
            let trimmed = allowed.trim().to_lowercase();
            if trimmed.is_empty() {
                return false;
            }
            let base_pattern = if let Some(stripped) = trimmed.strip_prefix("*.") {
                stripped
            } else {
                trimmed.as_str()
            };
            let pattern_domain = extract_domain(base_pattern);
            if pattern_domain.is_empty() {
                return false;
            }
            if domain == pattern_domain || domain.ends_with(&format!(".{}", pattern_domain)) {
                return true;
            }
            false
        })
    }

    /// Update settings in place from a partial or complete JSON value.
    pub fn update_from_json(&mut self, val: &serde_json::Value) {
        if let Some(obj) = val.as_object() {
            for (k, v) in obj {
                match k.as_str() {
                    "downloads" => self.update_from_json(&serde_json::json!({
                        "askWhereToSave": v.get("ask_where_to_save"),
                        "showProgressToolbar": v.get("show_progress_toolbar"),
                    })),
                    "tabs" => self.update_from_json(&serde_json::json!({
                        "tabReordering": v.get("enable_tab_reordering"),
                        "tabTearoff": v.get("enable_tab_tearoff"),
                        "warnOnCloseTabs": v.get("warn_on_close_tabs"),
                    })),
                    "appearance" => self.update_from_json(&serde_json::json!({
                        "theme": v.get("theme"),
                        "density": v.get("density"),
                        "default_zoom_level": v.get("default_zoom_level"),
                        "showZoomBadge": v.get("show_zoom_badge"),
                        "enableLinkPreview": v.get("enable_link_preview"),
                        "showStatusPreview": v.get("show_status_preview"),
                    })),
                    "performance" => {
                        if let Some(performance) = v.as_object() {
                            if let Some(enabled) = performance
                                .get("background_throttling")
                                .and_then(|value| value.as_bool())
                            {
                                self.performance.background_throttling = enabled;
                            }
                            if let Some(seconds) = performance
                                .get("sleep_after_secs")
                                .and_then(|value| value.as_u64())
                            {
                                self.performance.sleep_after_secs = seconds.min(7200);
                            }
                        }
                    }
                    "search_engine" => {
                        if let Some(s) = v.as_str() {
                            self.search_engine = s.to_string();
                        }
                    }
                    "adblock_mode" => {
                        if let Ok(mode) = serde_json::from_value::<AdblockMode>(v.clone()) {
                            self.adblock_mode = mode;
                        }
                    }
                    "askWhereToSave" => {
                        if let Some(b) = v.as_bool() {
                            self.downloads.ask_where_to_save = b;
                        }
                    }
                    "showProgressToolbar" => {
                        if let Some(b) = v.as_bool() {
                            self.downloads.show_progress_toolbar = b;
                        }
                    }
                    "tabReordering" => {
                        if let Some(b) = v.as_bool() {
                            self.tabs.enable_tab_reordering = b;
                        }
                    }
                    "tabTearoff" => {
                        if let Some(b) = v.as_bool() {
                            self.tabs.enable_tab_tearoff = b;
                        }
                    }
                    "warnOnCloseTabs" => {
                        if let Some(b) = v.as_bool() {
                            self.tabs.warn_on_close_tabs = b;
                        }
                    }
                    "theme" => {
                        if let Some(theme) = v
                            .as_str()
                            .filter(|v| matches!(*v, "system" | "light" | "dark"))
                        {
                            self.appearance.theme = theme.to_string();
                        }
                    }
                    "density" => {
                        if let Some(density) = v
                            .as_str()
                            .filter(|v| matches!(*v, "comfortable" | "compact"))
                        {
                            self.appearance.density = density.to_string();
                        }
                    }
                    "default_zoom_level" => {
                        if let Some(f) = v.as_f64() {
                            self.appearance.default_zoom_level = f.clamp(0.25, 5.0);
                        }
                    }
                    "showZoomBadge" => {
                        if let Some(b) = v.as_bool() {
                            self.appearance.show_zoom_badge = b;
                        }
                    }
                    "enableLinkPreview" => {
                        if let Some(b) = v.as_bool() {
                            self.appearance.enable_link_preview = b;
                        }
                    }
                    "showStatusPreview" => {
                        if let Some(b) = v.as_bool() {
                            self.appearance.show_status_preview = b;
                        }
                    }
                    "permissions" => {
                        if let Some(p_obj) = v.as_object() {
                            if let Some(loc) = p_obj.get("location").and_then(|x| x.as_str()) {
                                self.permissions.location = loc.to_string();
                            }
                            if let Some(cam) = p_obj.get("camera").and_then(|x| x.as_str()) {
                                self.permissions.camera = cam.to_string();
                            }
                            if let Some(mic) = p_obj.get("microphone").and_then(|x| x.as_str()) {
                                self.permissions.microphone = mic.to_string();
                            }
                            if let Some(notif) = p_obj.get("notifications").and_then(|x| x.as_str())
                            {
                                self.permissions.notifications = notif.to_string();
                            }
                        }
                    }
                    "features" => {
                        if let Some(f_obj) = v.as_object() {
                            if let Some(b) =
                                f_obj.get("enable_find_in_page").and_then(|x| x.as_bool())
                            {
                                self.features.enable_find_in_page = b;
                            }
                            if let Some(b) = f_obj
                                .get("enable_downloads_manager")
                                .and_then(|x| x.as_bool())
                            {
                                self.features.enable_downloads_manager = b;
                            }
                            if let Some(b) =
                                f_obj.get("enable_link_preview").and_then(|x| x.as_bool())
                            {
                                self.features.enable_link_preview = b;
                            }
                            if let Some(b) =
                                f_obj.get("enable_tab_gestures").and_then(|x| x.as_bool())
                            {
                                self.features.enable_tab_gestures = b;
                            }
                            if let Some(b) =
                                f_obj.get("enable_zoom_controls").and_then(|x| x.as_bool())
                            {
                                self.features.enable_zoom_controls = b;
                            }
                            if let Some(b) = f_obj
                                .get("enable_permissions_prompt")
                                .and_then(|x| x.as_bool())
                            {
                                self.features.enable_permissions_prompt = b;
                            }
                        }
                    }
                    "enable_find_in_page" => {
                        if let Some(b) = v.as_bool() {
                            self.features.enable_find_in_page = b;
                        }
                    }
                    "enable_downloads_manager" => {
                        if let Some(b) = v.as_bool() {
                            self.features.enable_downloads_manager = b;
                        }
                    }
                    "enable_link_preview" => {
                        if let Some(b) = v.as_bool() {
                            self.features.enable_link_preview = b;
                        }
                    }
                    "enable_tab_gestures" => {
                        if let Some(b) = v.as_bool() {
                            self.features.enable_tab_gestures = b;
                        }
                    }
                    "enable_zoom_controls" => {
                        if let Some(b) = v.as_bool() {
                            self.features.enable_zoom_controls = b;
                        }
                    }
                    "enable_permissions_prompt" => {
                        if let Some(b) = v.as_bool() {
                            self.features.enable_permissions_prompt = b;
                        }
                    }
                    "privacy" => {
                        if let Some(p_obj) = v.as_object() {
                            if let Some(b) =
                                p_obj.get("ephemeral_default").and_then(|x| x.as_bool())
                            {
                                self.privacy.ephemeral_default = b;
                            }
                            if let Some(arr) =
                                p_obj.get("persistent_sites").and_then(|x| x.as_array())
                            {
                                self.privacy.persistent_sites = arr
                                    .iter()
                                    .filter_map(|x| x.as_str().map(|s| s.trim().to_string()))
                                    .filter(|s| !s.is_empty())
                                    .collect();
                            }
                        }
                    }
                    "persistent_sites" => {
                        if let Some(arr) = v.as_array() {
                            self.privacy.persistent_sites = arr
                                .iter()
                                .filter_map(|x| x.as_str().map(|s| s.trim().to_string()))
                                .filter(|s| !s.is_empty())
                                .collect();
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

fn dirs_fallback_downloads() -> PathBuf {
    if let Ok(userprofile) = std::env::var("USERPROFILE") {
        PathBuf::from(userprofile).join("Downloads")
    } else {
        PathBuf::from("Downloads")
    }
}

impl Settings {
    pub fn load_from_path(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        if !path.exists() {
            let default_settings = Self::default();
            default_settings.save_to_path(path)?;
            return Ok(default_settings);
        }
        let content = std::fs::read_to_string(path)?;
        let settings = serde_json::from_str(&content)?;
        Ok(settings)
    }

    pub fn save_to_path(&self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        let staging = path.with_extension("json.tmp");
        std::fs::write(&staging, content)?;
        std::fs::rename(staging, path)?;
        Ok(())
    }
}

/// Helper to extract a normalized domain from a URL or raw domain string.
pub fn extract_domain(input: &str) -> String {
    let mut s = input.trim();
    if s.is_empty() {
        return String::new();
    }
    // Reject internal evergreen schemes or about:blank
    if s.starts_with("evergreen://")
        || s.starts_with("about:")
        || s.starts_with("data:")
        || s.starts_with("javascript:")
    {
        return String::new();
    }
    // Strip http:// or https:// or any other scheme
    if let Some(idx) = s.find("://") {
        s = &s[idx + 3..];
    }
    // Strip trailing path/query/fragment
    if let Some(idx) = s.find(['/', '?', '#']) {
        s = &s[..idx];
    }
    // Strip userinfo if present (user:pass@host)
    if let Some(idx) = s.find('@') {
        s = &s[idx + 1..];
    }
    // Strip port if present (host:8080)
    if let Some(idx) = s.rfind(':') {
        if !s.starts_with('[') || s.ends_with(']') {
            s = &s[..idx];
        }
    }
    // Strip surrounding brackets for IPv6
    s = s.trim_matches(|c| c == '[' || c == ']');
    // Normalize to lowercase
    let mut domain = s.to_lowercase();
    // Strip optional leading www.
    if let Some(stripped) = domain.strip_prefix("www.") {
        domain = stripped.to_string();
    }
    domain
}
