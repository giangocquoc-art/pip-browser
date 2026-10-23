//! One Brave engine shared by tabs; native filtering, cosmetic rules and upstream scriptlets.
use adblock::{
    lists::{FilterSet, ParseOptions},
    request::Request,
    resources::{PermissionMask, Resource},
    Engine,
};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use evergreen_core::settings::AdblockMode;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::PathBuf,
    rc::{Rc, Weak},
    sync::OnceLock,
};
use webview2_com::{
    AddScriptToExecuteOnDocumentCreatedCompletedHandler, DOMContentLoadedEventHandler,
    Microsoft::Web::WebView2::Win32::*, WebResourceRequestedEventHandler,
};
use windows::core::{w, Interface, HSTRING, PWSTR};
use wry::WebViewExtWindows;

pub struct Blocker {
    engine: Engine,
    popup_engine: Engine,
    gambling_domains: GamblingDomains,
    mode: AdblockMode,
    disabled: HashSet<String>,
    exceptions_path: PathBuf,
    pub blocked: u64,
    pub redirected: u64,
    bootstraps: HashMap<String, Weak<RefCell<Bootstrap>>>,
    explicit_navigation_tickets: HashMap<(u64, String, String), std::time::Instant>,
}

const EXPLICIT_NAVIGATION_TICKET_TTL: std::time::Duration = std::time::Duration::from_secs(5);

pub(crate) enum FilterAction {
    Block,
    Redirect(String),
}

struct FilterSiteContext<'a> {
    top_host: &'a str,
    top_registrable_domain: Option<&'a str>,
    source_is_http_site: bool,
}

/// Ephemeral provenance retained only for popup-created `about:blank` tabs.
/// It is consumed after the first successful HTTP(S) document commit.
#[derive(Default)]
pub(crate) struct PopupNavigationContext {
    opener_url: Option<String>,
}

impl PopupNavigationContext {
    pub(crate) fn from_opener(opener_url: Option<String>) -> Self {
        Self { opener_url }
    }

    pub(crate) fn opener_for_source(&self, source: &str) -> Option<String> {
        (source == "about:blank")
            .then(|| self.opener_url.clone())
            .flatten()
    }

    pub(crate) fn is_pending(&self) -> bool {
        self.opener_url.is_some()
    }

    pub(crate) fn navigation_committed(&mut self, source: &str) {
        let is_http_page = source.parse::<wry::http::Uri>().ok().is_some_and(|uri| {
            matches!(uri.scheme_str(), Some("http" | "https")) && uri.host().is_some()
        });
        if is_http_page {
            self.opener_url = None;
        }
    }
}

#[derive(Default)]
struct Bootstrap {
    id: Option<String>,
}

#[cfg(test)]
const GAMBLING_DOMAIN_INPUT: &str = include_str!("../filters/gambling-hagezi-mini-domains.txt");
const GAMBLING_DOMAIN_SOURCE: &str =
    include_str!(concat!(env!("OUT_DIR"), "/gambling-domains.txt"));
include!(concat!(env!("OUT_DIR"), "/gambling-domain-offsets.rs"));

struct GamblingDomains {
    offsets: &'static [u32],
}

impl GamblingDomains {
    fn bundled() -> Self {
        Self {
            offsets: &GAMBLING_DOMAIN_OFFSETS,
        }
    }

    #[cfg(test)]
    fn empty() -> Self {
        Self { offsets: &[] }
    }
}

fn domain_at(offset: u32) -> &'static str {
    GAMBLING_DOMAIN_SOURCE[offset as usize..]
        .lines()
        .next()
        .expect("offset points to a domain")
        .trim()
}

fn site(url: &str) -> Option<String> {
    let uri = url.parse::<wry::http::Uri>().ok()?;
    if !matches!(uri.scheme_str(), Some("http" | "https")) {
        return None;
    }
    Some(uri.host()?.to_ascii_lowercase())
}

/// WebResourceRequested fires once for every page resource, while WebView2
/// reports the same top-level URL for the whole document. Keep its parsed host
/// across those callbacks to avoid reparsing and allocating it for every image,
/// script, and style request.
#[derive(Default)]
struct CachedTopSite {
    url: String,
    host: Option<String>,
    registrable_domain: Option<String>,
    registrable_domain_checked: bool,
    #[cfg(test)]
    parses: usize,
}

impl CachedTopSite {
    fn update_url(&mut self, url: &str) {
        if self.url != url {
            self.url.clear();
            self.url.push_str(url);
            self.host = site(url);
            self.registrable_domain = None;
            self.registrable_domain_checked = false;
            #[cfg(test)]
            {
                self.parses += 1;
            }
        }
    }

    #[cfg(test)]
    fn get(&mut self, url: &str) -> Option<&str> {
        self.update_url(url);
        self.host.as_deref()
    }

    fn get_with_registrable_domain(
        &mut self,
        url: &str,
        include_domain: bool,
    ) -> (Option<&str>, Option<&str>) {
        self.update_url(url);
        if include_domain && !self.registrable_domain_checked {
            self.registrable_domain = registrable_domain(&self.url);
            self.registrable_domain_checked = true;
        }
        (
            self.host.as_deref(),
            if include_domain {
                self.registrable_domain.as_deref()
            } else {
                None
            },
        )
    }

    #[cfg(test)]
    fn registrable_domain(&mut self) -> Option<&str> {
        if !self.registrable_domain_checked {
            self.registrable_domain = registrable_domain(&self.url);
            self.registrable_domain_checked = true;
        }
        self.registrable_domain.as_deref()
    }
}

fn is_http_site(url: &str) -> bool {
    url.parse::<wry::http::Uri>().ok().is_some_and(|uri| {
        matches!(uri.scheme_str(), Some("http" | "https")) && uri.host().is_some()
    })
}

fn registrable_domain(url: &str) -> Option<String> {
    adblock::url_parser::parse_url(url)
        .map(|parsed| parsed.domain().trim_end_matches('.').to_ascii_lowercase())
}

#[cfg(test)]
fn hosts_share_domain(first: &str, second: &str) -> bool {
    fn registrable_domain_for_host(host: &str) -> Option<String> {
        let host = host.trim_end_matches('.');
        if host.is_empty() {
            return None;
        }
        registrable_domain(&format!("https://{host}/"))
    }

    match (
        registrable_domain_for_host(first),
        registrable_domain_for_host(second),
    ) {
        (Some(first), Some(second)) => first == second,
        _ => first
            .trim_end_matches('.')
            .eq_ignore_ascii_case(second.trim_end_matches('.')),
    }
}

fn is_video_stream_request(url: &str, kind: &str) -> bool {
    if kind == "media" {
        return true;
    }
    if kind != "xmlhttprequest" {
        return false;
    }
    let Ok(uri) = url.parse::<wry::http::Uri>() else {
        return false;
    };
    let path = uri.path().as_bytes();
    const SEGMENT_EXTENSIONS: [&[u8]; 11] = [
        b".m3u8", b".m4s", b".ts", b".mpd", b".mp4", b".m4v", b".webm", b".m4a", b".aac", b".cmfv",
        b".cmfa",
    ];
    SEGMENT_EXTENSIONS.iter().any(|extension| {
        path.len() >= extension.len()
            && path[path.len() - extension.len()..].eq_ignore_ascii_case(extension)
    })
}

fn looks_like_gambling_host(host: &str) -> bool {
    const TERMS: &[&str] = &[
        "casino",
        "casinos",
        "gambling",
        "poker",
        "slot",
        "slots",
        "togel",
        "judi",
        "taruhan",
        "baccarat",
        "jackpot",
        "sportsbook",
        "bookmaker",
        "betting",
        "taixiu",
        "xocdia",
        "xoso",
        "blackjack",
        "roulette",
        "lottery",
        "lotto",
        "keno",
        "nhacai",
        "nohu",
        "gamebai",
        "cacuoc",
    ];
    host.split('.')
        .flat_map(|label| label.split(|ch: char| !ch.is_ascii_alphanumeric()))
        .filter(|part| !part.is_empty())
        .any(|part| {
            TERMS.iter().any(|term| {
                part == *term
                    || part.strip_prefix(term).is_some_and(|suffix| {
                        !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit())
                    })
                    // Gambling brands commonly put the stake/bonus number before
                    // "bet" (for example, 789bet). Check this only in popup
                    // destinations so ordinary sites and typed URLs keep working.
                    || (part.ends_with(term)
                        && part.len() > term.len()
                        && part[..part.len() - term.len()]
                            .chars()
                            .all(|ch| ch.is_ascii_digit()))
            }) || (part.starts_with("bet")
                && part.len() > 3
                && part[3..].chars().all(|ch| ch.is_ascii_digit()))
                || (part.ends_with("bet")
                    && part.len() > 3
                    && part[..part.len() - 3]
                        .chars()
                        .all(|ch| ch.is_ascii_digit()))
                // Rotating Vietnamese casino domains often use "vip" followed
                // by a changing number, with no casino keyword in the hostname.
                || part.find("vip").is_some_and(|index| {
                    let suffix = &part[index + 3..];
                    !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit())
                })
        })
}

fn matches_gambling_domain(host: &str, domains: &GamblingDomains) -> bool {
    // A fully qualified DNS name may include its terminal root dot. It is the
    // same host for navigation purposes, so normalize it before suffix lookup.
    let host = host.trim_end_matches('.');
    if host.is_empty() {
        return false;
    }
    let mut start = 0;
    loop {
        if domains
            .offsets
            .binary_search_by(|offset| domain_at(*offset).cmp(&host[start..]))
            .is_ok()
        {
            return true;
        }
        let Some(dot) = host[start..].find('.') else {
            return false;
        };
        start += dot + 1;
    }
}

/// HaGeZi's popup list is a domain-only list (`||host^`), not a list of
/// `$popup` rules. Keep it as a lazily parsed sorted slice so it stays cheap
/// for normal page loads and avoids compiling 47k domain rules into another
/// adblock engine at startup.
fn matches_hagezi_popup_domain(host: &str) -> bool {
    static DOMAINS: OnceLock<Vec<&'static str>> = OnceLock::new();
    let domains = DOMAINS.get_or_init(|| {
        let mut domains = include_str!("../filters/popupads-hagezi.txt")
            .lines()
            .filter_map(|line| {
                line.trim()
                    .strip_prefix("||")?
                    .strip_suffix('^')
                    .filter(|domain| {
                        !domain.is_empty()
                            && domain.bytes().all(|byte| {
                                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-')
                            })
                    })
            })
            .collect::<Vec<_>>();
        domains.sort_unstable();
        domains.dedup();
        domains
    });

    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() {
        return false;
    }
    let mut start = 0;
    loop {
        if domains
            .binary_search_by(|domain| (*domain).cmp(&host[start..]))
            .is_ok()
        {
            return true;
        }
        let Some(dot) = host[start..].find('.') else {
            return false;
        };
        start += dot + 1;
    }
}

fn engine(rules: &str) -> Engine {
    let mut filters = FilterSet::new(false);
    filters.add_filter_list(
        rules.to_owned(),
        ParseOptions {
            rule_types: adblock::lists::RuleTypes::All,
            ..Default::default()
        },
    );
    Engine::new_with_filter_set(filters)
}

fn popup_rules(list: &str) -> String {
    let mut output = String::new();
    for line in list.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('!') || line.starts_with('[') {
            continue;
        }
        let Some((pattern, options)) = line.split_once('$') else {
            continue;
        };
        let options = options.split(',').map(str::trim).collect::<Vec<_>>();
        if !options
            .iter()
            .any(|option| matches!(*option, "popup" | "popunder"))
        {
            continue;
        }
        let remaining = options
            .into_iter()
            // Every request reaching this engine is a popup document, so
            // `$doc` is implicit and must not be checked against `other`.
            .filter(|option| !matches!(*option, "popup" | "popunder" | "doc" | "document"))
            .collect::<Vec<_>>();
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(pattern);
        if !remaining.is_empty() {
            output.push('$');
            output.push_str(&remaining.join(","));
        }
    }
    output
}

fn popup_engine_from_sources<'a>(sources: impl IntoIterator<Item = &'a str>) -> Engine {
    let mut filters = FilterSet::new(false);
    for source in sources {
        let rules = popup_rules(source);
        if !rules.is_empty() {
            filters.add_filter_list(
                rules,
                ParseOptions {
                    rule_types: adblock::lists::RuleTypes::All,
                    ..Default::default()
                },
            );
        }
    }
    Engine::new_with_filter_set(filters)
}

impl Blocker {
    pub fn enabled(&self, url: &str) -> Option<bool> {
        Some(!self.disabled.contains(&site(url)?))
    }
    pub fn load(directory: PathBuf, mode: AdblockMode) -> Self {
        let exceptions_path = directory.join("adblock-exceptions.json");
        let disabled = std::fs::read(&exceptions_path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let updated_lists = crate::adblock_updates::load_cached(&directory);
        let mut filters = FilterSet::new(false);
        let easylist = updated_lists
            .as_ref()
            .map(|lists| lists.easylist.as_str())
            .unwrap_or(include_str!("../filters/easylist.txt"));
        filters.add_filter_list(
            easylist.to_owned(),
            if updated_lists.is_some() {
                ParseOptions {
                    rule_types: adblock::lists::RuleTypes::NetworkOnly,
                    ..Default::default()
                }
            } else {
                ParseOptions::default()
            },
        );
        let ublock_network_rules = [
            updated_lists
                .as_ref()
                .map(|lists| lists.ublock_filters.as_str())
                .unwrap_or(include_str!(
                    "../filters/upstream/uBlockOrigin/uAssets/filters/filters.txt"
                )),
            updated_lists
                .as_ref()
                .map(|lists| lists.ublock_quick_fixes.as_str())
                .unwrap_or(include_str!(
                    "../filters/upstream/uBlockOrigin/uAssets/filters/quick-fixes.txt"
                )),
            updated_lists
                .as_ref()
                .map(|lists| lists.ublock_unbreak.as_str())
                .unwrap_or(include_str!(
                    "../filters/upstream/uBlockOrigin/uAssets/filters/unbreak.txt"
                )),
        ]
        .join("\n");
        filters.add_filter_list(
            ublock_network_rules,
            ParseOptions {
                rule_types: adblock::lists::RuleTypes::NetworkOnly,
                ..Default::default()
            },
        );
        // adblock-rust does not implement ABP popup/popunder resource types.
        // Translate those options into a dedicated engine queried only for
        // WebView2 popup destinations. Use bundled popup rules, the dedicated
        // HaGeZi popup ads domain list, and reviewed Pip overrides.
        let popup_engine = popup_engine_from_sources([
            easylist,
            updated_lists
                .as_ref()
                .map(|lists| lists.ublock_filters.as_str())
                .unwrap_or(include_str!(
                    "../filters/upstream/uBlockOrigin/uAssets/filters/filters.txt"
                )),
            updated_lists
                .as_ref()
                .map(|lists| lists.ublock_quick_fixes.as_str())
                .unwrap_or(include_str!(
                    "../filters/upstream/uBlockOrigin/uAssets/filters/quick-fixes.txt"
                )),
            updated_lists
                .as_ref()
                .map(|lists| lists.ublock_unbreak.as_str())
                .unwrap_or(include_str!(
                    "../filters/upstream/uBlockOrigin/uAssets/filters/unbreak.txt"
                )),
            include_str!("../filters/popupads-hagezi.txt"),
            include_str!("../filters/popup-overrides.txt"),
        ]);
        // Only pinned, reviewed upstream lists can invoke trusted scriptlets.
        filters.add_filter_list(
            include_str!("../filters/youtube.txt").to_owned(),
            ParseOptions {
                permissions: PermissionMask::from_bits(1),
                ..Default::default()
            },
        );
        let mut engine = Engine::new_with_filter_set(filters);
        let mut resources: Vec<Resource> =
            serde_json::from_str(include_str!("../filters/scriptlet-resources.json"))
                .expect("bundled upstream resources must be valid");
        resources.push(Resource {
            name: "noopjs".into(),
            aliases: vec!["noop.js".into()],
            kind: adblock::resources::ResourceType::Mime(
                adblock::resources::MimeType::ApplicationJavascript,
            ),
            content: BASE64_STANDARD.encode("void 0;"),
            dependencies: Vec::new(),
            permission: PermissionMask::default(),
        });
        resources.push(Resource {
            name: "nooptext".into(),
            aliases: vec!["noop.txt".into()],
            kind: adblock::resources::ResourceType::Mime(adblock::resources::MimeType::TextPlain),
            content: BASE64_STANDARD.encode(""),
            dependencies: Vec::new(),
            permission: PermissionMask::default(),
        });
        resources.push(Resource {
            name: "noopframe".into(),
            aliases: vec!["noop.html".into()],
            kind: adblock::resources::ResourceType::Mime(adblock::resources::MimeType::TextHtml),
            content: BASE64_STANDARD
                .encode("<!doctype html><meta name=referrer content=no-referrer>"),
            dependencies: Vec::new(),
            permission: PermissionMask::default(),
        });
        resources.push(Resource {
            name: "1x1.gif".into(),
            aliases: Vec::new(),
            kind: adblock::resources::ResourceType::Mime(adblock::resources::MimeType::ImageGif),
            content: "R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==".into(),
            dependencies: Vec::new(),
            permission: PermissionMask::default(),
        });
        engine.use_resources(resources);
        Self {
            engine,
            popup_engine,
            gambling_domains: GamblingDomains::bundled(),
            mode,
            disabled,
            exceptions_path,
            blocked: 0,
            redirected: 0,
            bootstraps: HashMap::new(),
            explicit_navigation_tickets: HashMap::new(),
        }
    }

    pub fn set_mode(&mut self, mode: AdblockMode) {
        self.mode = mode;
    }

    pub fn toggle(&mut self, url: &str) -> Result<Option<bool>, Box<dyn std::error::Error>> {
        let Some(host) = site(url) else {
            return Ok(None);
        };
        let mut disabled = self.disabled.clone();
        let enabled = if disabled.remove(&host) {
            true
        } else {
            disabled.insert(host);
            false
        };
        std::fs::create_dir_all(self.exceptions_path.parent().unwrap())?;
        std::fs::write(&self.exceptions_path, serde_json::to_vec_pretty(&disabled)?)?;
        self.disabled = disabled;
        Ok(Some(enabled))
    }

    /// Disable filtering on one supported site without flipping an existing exception back on.
    /// Returns whether the site had filtering enabled before this operation.
    pub fn disable_for_playback(
        &mut self,
        url: &str,
    ) -> Result<Option<bool>, Box<dyn std::error::Error>> {
        let Some(was_enabled) = self.enabled(url) else {
            return Ok(None);
        };
        if was_enabled {
            self.toggle(url)?;
        }
        Ok(Some(was_enabled))
    }

    #[cfg(test)]
    fn filter_action(
        &self,
        url: &str,
        top: &str,
        source: &str,
        kind: &str,
        method: &str,
    ) -> Option<FilterAction> {
        let host = site(top)?;
        if self.disabled.contains(&host) {
            return None;
        }
        self.filter_action_with_site_context(
            url,
            source,
            kind,
            method,
            FilterSiteContext {
                top_host: &host,
                top_registrable_domain: registrable_domain(top).as_deref(),
                source_is_http_site: is_http_site(source),
            },
        )
    }

    fn filter_action_with_site_context(
        &self,
        url: &str,
        source: &str,
        kind: &str,
        method: &str,
        site_context: FilterSiteContext<'_>,
    ) -> Option<FilterAction> {
        let FilterSiteContext {
            top_host,
            top_registrable_domain,
            source_is_http_site,
        } = site_context;
        if self.disabled.contains(top_host) || !source_is_http_site {
            return None;
        }
        // Never blank the top-level page. Media is classified by WebView2, not its extension.
        if kind == "document" {
            return None;
        }
        if self.mode == AdblockMode::VideoFirst && is_video_stream_request(url, kind) {
            return None;
        }
        if self.mode == AdblockMode::Strong {
            let request_host = site(url)?;
            let request_host = request_host.trim_end_matches('.');
            let top_host = top_host.trim_end_matches('.');
            let third_party = match (registrable_domain(url), top_registrable_domain) {
                (Some(request_domain), Some(top_domain)) => request_domain != top_domain,
                _ => !request_host.eq_ignore_ascii_case(top_host),
            };
            if third_party && matches_gambling_domain(request_host, &self.gambling_domains) {
                return Some(FilterAction::Block);
            }
        }
        let request = Request::new(url, source, kind, method).ok()?;
        let result = self.engine.check_network_request(&request);
        if !result.should_block() {
            return None;
        }
        Some(match result.redirect {
            Some(resource) => FilterAction::Redirect(resource),
            None => FilterAction::Block,
        })
    }

    /// Exempt the exact one-shot top-level navigation the user submitted through Pip's
    /// address bar or a browser-owned link command. WebView2 marks API-triggered navigations
    /// as user initiated, so this explicit provenance is narrower and reliable.
    pub(crate) fn issue_explicit_navigation_ticket(
        &mut self,
        tab_id: u64,
        target: &str,
        source: &str,
    ) {
        let now = std::time::Instant::now();
        self.explicit_navigation_tickets
            .retain(|_, issued| now.duration_since(*issued) < EXPLICIT_NAVIGATION_TICKET_TTL);
        self.explicit_navigation_tickets
            .insert((tab_id, target.to_owned(), source.to_owned()), now);
    }

    pub(crate) fn consume_explicit_navigation_ticket(
        &mut self,
        tab_id: u64,
        target: &str,
        source: &str,
    ) -> bool {
        self.explicit_navigation_tickets
            .remove(&(tab_id, target.to_owned(), source.to_owned()))
            .is_some_and(|issued| issued.elapsed() < EXPLICIT_NAVIGATION_TICKET_TTL)
    }

    #[cfg(test)]
    fn should_block(&self, url: &str, top: &str, source: &str, kind: &str, method: &str) -> bool {
        self.filter_action(url, top, source, kind, method).is_some()
    }

    #[cfg(test)]
    pub(crate) fn should_block_popup(&self, url: &str, source: &str, user_initiated: bool) -> bool {
        self.should_block_popup_with_popup_context(url, source, None, user_initiated)
    }

    /// A nested window opened from a popup's initial about:blank document keeps
    /// the original web opener as its first-party context until that popup
    /// commits its first HTTP(S) document.
    pub(crate) fn should_block_popup_with_popup_context(
        &self,
        url: &str,
        source: &str,
        popup_opener: Option<&str>,
        user_initiated: bool,
    ) -> bool {
        let source_url = if source == "about:blank" {
            popup_opener.unwrap_or(source)
        } else {
            source
        };
        let Some(source_host) = site(source_url) else {
            return !user_initiated;
        };
        if self.disabled.contains(&source_host) {
            return false;
        }
        // Some ad redirects are reported by WebView2 as user-initiated because
        // they piggyback on a click. Treat clear gambling brands as unwanted
        // only in the new-window path; ordinary navigation is untouched.
        if !user_initiated {
            return true;
        }
        if site(url).is_some_and(|host| {
            looks_like_gambling_host(&host)
                || matches_gambling_domain(&host, &self.gambling_domains)
                || matches_hagezi_popup_domain(&host)
        }) {
            return true;
        }
        // Some campaign URLs land on a generic/rotating host but expose the
        // gambling destination in their path or query string.
        if looks_like_gambling_popup_url(url) {
            return true;
        }
        // This engine is consulted only for NewWindowRequested destinations.
        // Use `other` so generic network rules apply; adblock-rust separates
        // document navigations from ordinary filterable resources.
        Request::new(url, source_url, "other", "GET")
            .ok()
            .is_some_and(|request| {
                self.popup_engine
                    .check_network_request(&request)
                    .should_block()
            })
    }

    /// Catch known ad/popup destinations that redirect the current tab instead of opening
    /// a WebView2 NewWindowRequested target. Keep this narrower than popup handling: only
    /// HTTP(S) cross-site navigations from an HTTP(S) page are eligible.
    pub(crate) fn should_block_navigation(&self, url: &str, source: &str) -> bool {
        self.should_block_navigation_with_popup_context(url, source, None)
    }

    /// `window.open('about:blank')` can navigate after NewWindowRequested has
    /// already created a tab. In that one case, retain the opener as the
    /// first-party context until the blank tab commits its first HTTP(S) page.
    pub(crate) fn should_block_navigation_with_popup_context(
        &self,
        url: &str,
        source: &str,
        popup_opener: Option<&str>,
    ) -> bool {
        let source_url = if source == "about:blank" {
            popup_opener.unwrap_or(source)
        } else {
            source
        };
        let (Some(destination), Some(source_host)) = (site(url), site(source_url)) else {
            return false;
        };
        if destination == source_host || self.disabled.contains(&source_host) {
            return false;
        }
        if looks_like_gambling_host(&destination)
            || matches_gambling_domain(&destination, &self.gambling_domains)
            || matches_hagezi_popup_domain(&destination)
            || looks_like_gambling_popup_url(url)
        {
            return true;
        }
        let popup_rule_matches = Request::new(url, source_url, "other", "GET")
            .ok()
            .is_some_and(|request| {
                self.popup_engine
                    .check_network_request(&request)
                    .should_block()
            });
        let document_rule_matches = Request::new(url, source_url, "document", "GET")
            .ok()
            .is_some_and(|request| self.engine.check_network_request(&request).should_block());
        popup_rule_matches || document_rule_matches
    }

    /// Apply the same suspicious cross-site destination filter to iframe
    /// navigations, using the containing page as the first-party context.
    pub(crate) fn should_block_frame_navigation(&self, url: &str, top_level: &str) -> bool {
        self.should_block_frame_navigation_with_popup_context(url, top_level, None)
    }

    /// A popup-created blank document inherits the opener's site context. Apply
    /// it to its frames only while that popup remains at about:blank and the
    /// ephemeral opener context has not yet been consumed by an HTTP(S) commit.
    pub(crate) fn should_block_frame_navigation_with_popup_context(
        &self,
        url: &str,
        top_level: &str,
        popup_opener: Option<&str>,
    ) -> bool {
        let source_url = if top_level == "about:blank" {
            popup_opener.unwrap_or(top_level)
        } else {
            top_level
        };
        let (Some(destination), Some(source_host)) = (site(url), site(source_url)) else {
            return false;
        };
        if destination == source_host || self.disabled.contains(&source_host) {
            return false;
        }
        if looks_like_gambling_host(&destination)
            || matches_gambling_domain(&destination, &self.gambling_domains)
            || matches_hagezi_popup_domain(&destination)
            || looks_like_gambling_popup_url(url)
        {
            return true;
        }
        Request::new(url, source_url, "other", "GET")
            .ok()
            .is_some_and(|request| {
                self.popup_engine
                    .check_network_request(&request)
                    .should_block()
            })
    }

    fn cosmetic_script(&self, url: &str) -> Option<String> {
        let host = site(url)?;
        if self.disabled.contains(&host) {
            return None;
        }
        let resources = self.engine.url_cosmetic_resources(url);
        if resources.hide_selectors.is_empty() && resources.procedural_actions.is_empty() {
            return None;
        }
        let actions = resources
            .procedural_actions
            .into_iter()
            .take(128)
            .filter_map(|action| serde_json::from_str::<serde_json::Value>(&action).ok())
            .collect::<Vec<_>>();
        Some(cosmetic_script(url, &resources.hide_selectors, &actions))
    }

    pub(crate) fn youtube_script(&self) -> String {
        let mut script =
            String::from("(()=>{if(window!==window.top)return;switch(location.hostname){");
        // No observers or scriptlet machinery on other websites or internal Pip pages.
        for host in [
            "www.youtube.com",
            "youtube.com",
            "m.youtube.com",
            "music.youtube.com",
            "www.youtube-nocookie.com",
            "youtube-nocookie.com",
        ] {
            if self.disabled.contains(host) {
                continue;
            }
            let resources = self
                .engine
                .url_cosmetic_resources(&format!("https://{host}/"));
            if !resources.injected_script.is_empty() {
                script.push_str(&format!(
                    "case {}:{{const scriptletGlobals={{}};{}\nbreak;}}",
                    serde_json::to_string(host).unwrap(),
                    resources.injected_script
                ));
            }
        }
        script.push_str("}})();");
        script
    }
}

pub(crate) fn decode_redirect_data_uri(uri: &str) -> Option<(String, Vec<u8>)> {
    const MAX_REDIRECT_BYTES: usize = 8 * 1024 * 1024;
    if uri.len() > MAX_REDIRECT_BYTES.saturating_mul(4) / 3 + 256 {
        return None;
    }
    let (header, encoded) = uri.strip_prefix("data:")?.split_once(',')?;
    let (mime, encoding) = header.split_once(';')?;
    if encoding != "base64"
        || mime.is_empty()
        || !mime
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'+' | b'-' | b'.'))
    {
        return None;
    }
    let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, encoded).ok()?;
    (bytes.len() <= MAX_REDIRECT_BYTES).then(|| (mime.to_ascii_lowercase(), bytes))
}

fn cosmetic_script(
    url: &str,
    selectors: &HashSet<String>,
    procedural_actions: &[serde_json::Value],
) -> String {
    let url = serde_json::to_string(url).unwrap();
    let selectors = serde_json::to_string(selectors).unwrap();
    let actions = serde_json::to_string(procedural_actions).unwrap();
    format!(
        r#"(()=>{{if(location.href!=={url})return;
const css={selectors},rules={actions};
if(css.length){{const sheet=new CSSStyleSheet();for(const selector of css){{try{{sheet.insertRule(selector+'{{display:none!important}}')}}catch(_){{}}}}document.adoptedStyleSheets=[...document.adoptedStyleSheets,sheet]}}
const test=(value,pattern)=>{{value=String(value??'').slice(0,2048);pattern=String(pattern??'').trim();if(!pattern)return false;if(pattern[0]==='/'){{const end=pattern.lastIndexOf('/');if(end>0){{try{{return new RegExp(pattern.slice(1,end),pattern.slice(end+1).replace(/[^dgimsuvy]/g,'')).test(value)}}catch(_){{return false}}}}}}return value.toLowerCase().includes(pattern.toLowerCase())}};
const matchArg=(value,arg)=>{{arg=String(arg??'');const equal=arg.indexOf('=');const name=(equal<0?arg:arg.slice(0,equal)).trim();if(!name)return false;const actual=value.getAttribute(name);if(actual===null)return false;return equal<0||test(actual,arg.slice(equal+1).trim())}};
const apply=()=>{{let visited=0;for(const rule of rules){{if(++visited>128)break;let current=[];for(const op of rule.selector||[]){{const type=op.type,arg=op.arg;if(type==='css-selector'){{try{{current=Array.from(document.querySelectorAll(arg)).slice(0,300)}}catch(_){{current=[]}}continue}}if(type==='xpath'){{try{{const next=[];for(const node of current){{const result=document.evaluate(arg,node,null,XPathResult.ORDERED_NODE_SNAPSHOT_TYPE,null);for(let i=0;i<Math.min(result.snapshotLength,100);i++){{const item=result.snapshotItem(i);if(item instanceof Element)next.push(item)}}}}current=next.slice(0,300)}}catch(_){{current=[]}}continue}}current=current.filter(node=>{{try{{switch(type){{case'has-text':return test(node.textContent,arg);case'matches-attr':return matchArg(node,arg);case'matches-css':{{const [property,...rest]=String(arg).split(':');return test(getComputedStyle(node).getPropertyValue(property.trim()),rest.join(':'))}}case'matches-css-before':case'matches-css-after':{{const [property,...rest]=String(arg).split(':');return test(getComputedStyle(node,type.endsWith('before')?'::before':'::after').getPropertyValue(property.trim()),rest.join(':'))}}case'matches-path':return test(location.pathname,arg);case'min-text-length':return (node.textContent||'').trim().length>=Math.min(10000,Math.max(0,Number(arg)||0));case'upward':{{if(/^\d+$/.test(String(arg))){{let parent=node;for(let i=0;i<Math.min(20,Number(arg));i++)parent=parent?.parentElement;return !!parent}}return !!node.closest(arg)}}default:return false}}}}catch(_){{return false}}}})}}if(!current.length)continue;const action=rule.action;if(!action){{for(const node of current)node.style.setProperty('display','none','important');continue}}for(const node of current){{try{{switch(action.type){{case'remove':node.remove();break;case'style':node.style.cssText+=';'+action.arg;break;case'remove-attr':node.removeAttribute(action.arg);break;case'remove-class':node.classList.remove(action.arg);break}}}}catch(_){{}}}}}}}};
apply();let runs=0,timer=0;const observer=new MutationObserver(records=>{{if(runs>=16){{observer.disconnect();clearTimeout(timer);return}}if(!records.some(record=>record.addedNodes.length))return;clearTimeout(timer);timer=setTimeout(()=>{{runs++;apply();if(runs>=16)observer.disconnect()}},300)}});if(document.documentElement)observer.observe(document.documentElement,{{childList:true,subtree:true}});return rules.length;
}})();"#,
    )
}

#[cfg(debug_assertions)]
pub(crate) fn procedural_test_script(url: &str) -> String {
    let host = site(url).unwrap();
    let test_engine = engine(&format!("{host}##.ad:has-text(Casino):remove()"));
    let resources = test_engine.url_cosmetic_resources(url);
    let actions = resources
        .procedural_actions
        .into_iter()
        .filter_map(|action| serde_json::from_str::<serde_json::Value>(&action).ok())
        .collect::<Vec<_>>();
    cosmetic_script(url, &resources.hide_selectors, &actions)
}

fn looks_like_gambling_popup_url(url: &str) -> bool {
    let Ok(uri) = url.parse::<wry::http::Uri>() else {
        return false;
    };
    let path_and_query = uri.path_and_query().map_or("", |value| value.as_str());
    // Redirectors often percent-encode a destination in one or more query
    // parameters. Inspect at most two decoded layers, and only in this
    // popup/navigation heuristic; regular page content and requests are not
    // affected. The bound keeps deliberately huge URLs cheap to inspect.
    let mut candidate = path_and_query.as_bytes()[..path_and_query.len().min(8192)].to_vec();
    for _ in 0..=2 {
        if gambling_keyword_in_url_bytes(&candidate) {
            return true;
        }
        let decoded = decode_percent_encoded_ascii(&candidate);
        if decoded == candidate {
            break;
        }
        candidate = decoded;
    }
    false
}

fn decode_percent_encoded_ascii(input: &[u8]) -> Vec<u8> {
    fn hex_value(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    }

    let mut decoded = Vec::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        if input[index] == b'%' && index + 2 < input.len() {
            if let (Some(high), Some(low)) =
                (hex_value(input[index + 1]), hex_value(input[index + 2]))
            {
                let value = high * 16 + low;
                // Gambling terms are ASCII. Preserve escaped UTF-8 bytes so
                // decoding never creates invalid text for unrelated URLs.
                if value.is_ascii() {
                    decoded.push(value);
                } else {
                    decoded.extend_from_slice(&input[index..index + 3]);
                }
                index += 3;
                continue;
            }
        }
        decoded.push(input[index]);
        index += 1;
    }
    decoded
}

fn gambling_keyword_in_url_bytes(path_and_query: &[u8]) -> bool {
    let normalized_path = String::from_utf8_lossy(path_and_query).to_ascii_lowercase();
    normalized_path
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .any(|token| {
            let keyword_match = [
                "casino",
                "gambling",
                "poker",
                "slot",
                "slots",
                "togel",
                "judi",
                "taruhan",
                "baccarat",
                "jackpot",
                "sportsbook",
                "bookmaker",
                "betting",
                "taixiu",
                "xocdia",
                "xoso",
                "blackjack",
                "roulette",
                "lottery",
                "lotto",
                "keno",
                "nhacai",
                "nohu",
                "gamebai",
                "cacuoc",
            ]
            .iter()
            .any(|term| token == *term || token.starts_with(term) || token.ends_with(term));
            // Redirect services often wrap a rotating betting destination in a
            // generic query parameter (for example `?target=789bet`). Match the
            // same numeric brand form used for hostnames, without treating plain
            // English `bet` in an ordinary URL path as gambling.
            let numbered_bet = token.strip_prefix("bet").is_some_and(|suffix| {
                !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit())
            }) || token.strip_suffix("bet").is_some_and(|prefix| {
                !prefix.is_empty() && prefix.chars().all(|ch| ch.is_ascii_digit())
            });
            keyword_match || numbered_bet
        })
}

// Reuse WebView2's message-pumping completion helper, as Wry does for its
// initialization scripts. The first remote navigation waits for registration.
fn install_bootstrap(
    core: &ICoreWebView2,
    state: Rc<RefCell<Bootstrap>>,
    script: String,
) -> windows::core::Result<()> {
    let registration_core = core.clone();
    let new_id = Rc::new(RefCell::new(None));
    let callback_id = new_id.clone();
    AddScriptToExecuteOnDocumentCreatedCompletedHandler::wait_for_async_operation(
        Box::new(move |handler| unsafe {
            registration_core
                .AddScriptToExecuteOnDocumentCreated(&HSTRING::from(script), &handler)
                .map_err(Into::into)
        }),
        Box::new(move |result, id| {
            result?;
            *callback_id.borrow_mut() = Some(id);
            Ok(())
        }),
    )
    .map_err(|error| {
        windows::core::Error::new(
            windows::core::HRESULT(0x80004005u32 as i32),
            error.to_string(),
        )
    })?;
    let previous = std::mem::replace(&mut state.borrow_mut().id, new_id.borrow_mut().take());
    if let Some(id) = previous {
        unsafe {
            core.RemoveScriptToExecuteOnDocumentCreated(&HSTRING::from(id))?;
        }
    }
    Ok(())
}

pub fn refresh_scriptlets(
    view: &wry::WebView,
    blocker: &Rc<RefCell<Blocker>>,
) -> windows::core::Result<()> {
    let (state, script) = {
        let blocker = blocker.borrow();
        (
            blocker.bootstraps.get(view.id()).and_then(Weak::upgrade),
            blocker.youtube_script(),
        )
    };
    if let Some(state) = state {
        install_bootstrap(&unsafe { view.controller().CoreWebView2()? }, state, script)?;
    }
    Ok(())
}

pub fn attach(view: &wry::WebView, blocker: Rc<RefCell<Blocker>>) -> windows::core::Result<()> {
    let core = unsafe { view.controller().CoreWebView2()? };
    let bootstrap = Rc::new(RefCell::new(Bootstrap::default()));
    {
        let mut blocker = blocker.borrow_mut();
        blocker
            .bootstraps
            .retain(|_, value| value.strong_count() > 0);
        blocker
            .bootstraps
            .insert(view.id().to_string(), Rc::downgrade(&bootstrap));
    }
    let script = blocker.borrow().youtube_script();
    install_bootstrap(&core, bootstrap.clone(), script)?;
    let environment = view.environment();
    let top_site_cache = Rc::new(RefCell::new(CachedTopSite::default()));
    let request_top_site_cache = top_site_cache.clone();
    let cosmetic_blocker = blocker.clone();
    let cosmetic_handler = DOMContentLoadedEventHandler::create(Box::new(move |sender, _| {
        let _registration_lifetime = &bootstrap;
        if let Some(sender) = sender {
            let mut raw = PWSTR::null();
            unsafe { sender.Source(&mut raw)? };
            let url = webview2_com::take_pwstr(raw);
            // Release RefCell before calling COM, which may dispatch reentrant callbacks.
            let script = cosmetic_blocker.borrow().cosmetic_script(&url);
            if let Some(script) = script {
                unsafe { sender.ExecuteScript(&HSTRING::from(script), None)? };
            }
        }
        Ok(())
    }));
    let mut cosmetic_token = 0;
    unsafe {
        core.cast::<ICoreWebView2_2>()?
            .add_DOMContentLoaded(&cosmetic_handler, &mut cosmetic_token)?;
    }
    let handler = WebResourceRequestedEventHandler::create(Box::new(move |sender, args| {
        let (Some(sender), Some(args)) = (sender, args) else {
            return Ok(());
        };
        let request = unsafe { args.Request()? };
        let mut raw = PWSTR::null();
        unsafe {
            request.Uri(&mut raw)?;
        }
        let url = webview2_com::take_pwstr(raw);
        let mut raw = PWSTR::null();
        unsafe {
            sender.Source(&mut raw)?;
        }
        let top = webview2_com::take_pwstr(raw);
        let mut raw = PWSTR::null();
        unsafe {
            request.Method(&mut raw)?;
        }
        let method = webview2_com::take_pwstr(raw);
        // Referrer is the best available frame context in this WebView2 event.
        // ponytail: missing referrer falls back to top-level URL; no fabricated frame graph.
        let referrer = unsafe { request.Headers() }.ok().and_then(|headers| {
            let mut raw = PWSTR::null();
            unsafe { headers.GetHeader(w!("Referer"), &mut raw) }.ok()?;
            Some(webview2_com::take_pwstr(raw))
        });
        let mut context = COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL;
        unsafe {
            args.ResourceContext(&mut context)?;
        }
        let kind = match context {
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_SCRIPT => "script",
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE => "image",
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_STYLESHEET => "stylesheet",
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FONT => "font",
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_XML_HTTP_REQUEST
            | COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FETCH => "xmlhttprequest",
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_PING => "ping",
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_MEDIA => "media",
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_OTHER => "other",
            _ => return Ok(()),
        };
        let action = {
            // Drop this RefCell borrow before making any WebView2 COM calls
            // below; event callbacks may reenter the browser host.
            let mut top_site_cache = request_top_site_cache.borrow_mut();
            let needs_registrable_domain = blocker.borrow().mode == AdblockMode::Strong;
            let (top_host, top_registrable_domain) =
                top_site_cache.get_with_registrable_domain(&top, needs_registrable_domain);
            let (source, source_is_http_site) = match referrer {
                Some(referrer) if is_http_site(&referrer) => (referrer, true),
                _ => (top.clone(), top_host.is_some()),
            };
            top_host.and_then(|top_host| {
                blocker.borrow().filter_action_with_site_context(
                    &url,
                    &source,
                    kind,
                    &method,
                    FilterSiteContext {
                        top_host,
                        top_registrable_domain,
                        source_is_http_site,
                    },
                )
            })
        };
        if let Some(action) = action {
            let redirected = matches!(action, FilterAction::Redirect(_));
            let response = match action {
                FilterAction::Redirect(data_uri) => {
                    if let Some((mime, body)) = decode_redirect_data_uri(&data_uri) {
                        let stream = unsafe {
                            windows::Win32::System::Com::StructuredStorage::CreateStreamOnHGlobal(
                                windows::Win32::Foundation::HGLOBAL::default(),
                                true,
                            )?
                        };
                        unsafe {
                            stream
                                .Write(body.as_ptr().cast(), body.len() as u32, None)
                                .ok()?;
                            stream.Seek(0, windows::Win32::System::Com::STREAM_SEEK_SET, None)?;
                        }
                        let headers = HSTRING::from(format!(
                            "Content-Type: {mime}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n"
                        ));
                        unsafe {
                            environment.CreateWebResourceResponse(
                                Some(&stream),
                                200,
                                w!("OK"),
                                windows::core::PCWSTR(headers.as_ptr()),
                            )?
                        }
                    } else {
                        unsafe {
                            environment.CreateWebResourceResponse(
                                None,
                                204,
                                w!("No Content"),
                                w!("Cache-Control: no-store\r\n"),
                            )?
                        }
                    }
                }
                FilterAction::Block => unsafe {
                    environment.CreateWebResourceResponse(
                        None,
                        204,
                        w!("No Content"),
                        w!("Cache-Control: no-store\r\n"),
                    )?
                },
            };
            unsafe {
                args.SetResponse(&response)?;
            }
            let mut blocker = blocker.borrow_mut();
            blocker.blocked += 1;
            if redirected {
                blocker.redirected += 1;
            }
        }
        Ok(())
    }));
    let mut token = 0;
    unsafe {
        core.add_WebResourceRequested(&handler, &mut token)?;
        // Explicit document-source filters also cover cross-origin frames on modern runtimes.
        // Worker requests need profile-level ownership/context; don't duplicate them across tabs.
        let modern = core.cast::<ICoreWebView2_22>().ok();
        for context in [
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_SCRIPT,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_STYLESHEET,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FONT,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_XML_HTTP_REQUEST,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FETCH,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_PING,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_MEDIA,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_OTHER,
        ] {
            for pattern in [w!("https://*"), w!("http://*")] {
                if let Some(modern) = &modern {
                    modern.AddWebResourceRequestedFilterWithRequestSourceKinds(
                        pattern,
                        context,
                        COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_DOCUMENT,
                    )?;
                } else {
                    core.AddWebResourceRequestedFilter(pattern, context)?;
                }
            }
        }
        if let Some(modern) = &modern {
            // Each Wry WebView owns its own CoreWebView2Environment. Register one combined
            // worker-source filter per view/environment so shared worker requests are not
            // duplicated within an environment and service-worker traffic is still covered.
            let worker_sources = COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS(
                COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_SERVICE_WORKER.0
                    | COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_SHARED_WORKER.0,
            );
            for context in [
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_SCRIPT,
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE,
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_STYLESHEET,
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FONT,
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_XML_HTTP_REQUEST,
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FETCH,
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_PING,
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_MEDIA,
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_OTHER,
            ] {
                for pattern in [w!("https://*"), w!("http://*")] {
                    modern.AddWebResourceRequestedFilterWithRequestSourceKinds(
                        pattern,
                        context,
                        worker_sources,
                    )?;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_top_site_reuses_parsed_host_until_document_url_changes() {
        let mut cached = CachedTopSite::default();
        let first = "https://Example.com/article";

        assert_eq!(cached.get(first), Some("example.com"));
        assert_eq!(cached.get(first), Some("example.com"));
        assert_eq!(cached.parses, 1);
        assert_eq!(cached.registrable_domain(), Some("example.com"));
        assert_eq!(cached.registrable_domain(), Some("example.com"));

        assert_eq!(cached.get("https://other.example/"), Some("other.example"));
        assert_eq!(cached.parses, 2);
        assert_eq!(cached.registrable_domain(), Some("other.example"));
        assert_eq!(cached.get("about:blank"), None);
        assert_eq!(cached.get("about:blank"), None);
        assert_eq!(cached.parses, 3);
    }

    #[test]
    fn cached_network_filter_updates_are_loaded_without_enabling_remote_scriptlets() {
        let directory = std::env::temp_dir().join(format!(
            "pip-adblock-updates-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let mut easylist = (0..120)
            .map(|index| format!("||refreshed-ad-{index}.example^"))
            .collect::<Vec<_>>();
        easylist.push("||dynamic-ad.example^$script".into());
        let ordinary = (0..120)
            .map(|index| format!("||ublock-ad-{index}.example^"))
            .collect::<Vec<_>>()
            .join("\n");
        let lists = crate::adblock_updates::UpdatedLists {
            version: 1,
            easylist: format!("! Title: Updated EasyList\n{}", easylist.join("\n")),
            ublock_filters: ordinary.clone(),
            ublock_quick_fixes: ordinary.clone(),
            ublock_unbreak: ordinary,
        };
        std::fs::write(
            directory.join("adblock-updated-lists.json"),
            serde_json::to_vec(&lists).unwrap(),
        )
        .unwrap();

        let blocker = Blocker::load(directory.clone(), AdblockMode::Balanced);
        let top = "https://news.example/";
        assert!(blocker.should_block(
            "https://dynamic-ad.example/banner.js",
            top,
            top,
            "script",
            "GET"
        ));
        let cosmetic = blocker.engine.url_cosmetic_resources(top);
        assert!(cosmetic.hide_selectors.is_empty());

        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn youtube_resources_resolve_and_exceptions_remove_document_start_code() {
        let mut blocker = Blocker::load(
            std::env::temp_dir().join("pip-unused-test-path"),
            AdblockMode::Balanced,
        );
        let script = blocker.youtube_script();
        assert!(script.contains("function jsonPruneFetchResponse"));
        assert!(script.contains("function setConstant"));
        assert!(script.contains("function safeSelf"));
        assert!(script.contains("case \"www.youtube.com\""));
        assert!(!script.contains("case \"youtube.com.evil\""));
        let artifact = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/youtube-bootstrap.js");
        std::fs::write(artifact, &script).unwrap();
        blocker.disabled.insert("www.youtube.com".into());
        assert!(!blocker
            .youtube_script()
            .contains("case \"www.youtube.com\""));
    }

    #[test]
    fn incremental_popup_filter_construction_matches_combined_rules() {
        let sources = [
            "||ads.example^$popup\n@@||ads.example/allowed$popup,third-party\n||cdn.example^$popunder,third-party",
            "||script-only.example^$script\n||tracking.example^$popup,third-party\n@@||tracking.example/allow$popup",
        ];
        let combined_rules = sources
            .iter()
            .map(|source| popup_rules(source))
            .collect::<Vec<_>>()
            .join("\n");
        let combined_engine = engine(&combined_rules);
        let incremental_engine = popup_engine_from_sources(sources);
        let top = "https://news.example/article";
        let cases = [
            ("https://ads.example/redirect", true),
            ("https://ads.example/allowed", false),
            ("https://cdn.example/landing", true),
            ("https://tracking.example/pixel", true),
            ("https://tracking.example/allow", false),
            ("https://script-only.example/run.js", false),
        ];

        for (url, expected_block) in cases {
            let request = Request::new(url, top, "other", "GET").unwrap();
            let combined_block = combined_engine
                .check_network_request(&request)
                .should_block();
            let incremental_block = incremental_engine
                .check_network_request(&request)
                .should_block();
            assert_eq!(combined_block, expected_block, "combined rules: {url}");
            assert_eq!(
                incremental_block, expected_block,
                "incremental rules: {url}"
            );
        }
    }

    #[test]
    fn blocking_exceptions_and_video() {
        let mut b = Blocker {
            engine: engine("||ads.example^\n@@||ads.example/allowed.js$script"),
            popup_engine: engine(""),
            gambling_domains: GamblingDomains::empty(),
            mode: AdblockMode::Balanced,
            disabled: HashSet::new(),
            exceptions_path: PathBuf::new(),
            blocked: 0,
            redirected: 0,
            bootstraps: HashMap::new(),
            explicit_navigation_tickets: HashMap::new(),
        };
        let top = "https://news.example/";
        assert!(b.should_block("https://ads.example/banner.js", top, top, "script", "GET"));
        assert!(!b.should_block("https://ads.example/allowed.js", top, top, "script", "GET"));
        assert!(b.should_block(
            "https://ads.example/live.M3U8?token=x",
            top,
            top,
            "xmlhttprequest",
            "GET"
        ));
        assert!(b.should_block("https://ads.example/video", top, top, "media", "GET"));
        assert!(!b.should_block("https://video.example/live.m3u8", top, top, "media", "GET"));
        assert!(!b.should_block("https://ads.example/", "about:blank", top, "script", "GET"));
        b.disabled.insert("news.example".into());
        assert!(!b.should_block("https://ads.example/banner.js", top, top, "script", "GET"));
        assert!(b.should_block(
            "https://ads.example/banner.js",
            "https://news.example.evil/",
            top,
            "script",
            "GET"
        ));
        let bundled = engine(include_str!("../filters/easylist.txt"));
        assert!(bundled
            .url_cosmetic_resources(top)
            .hide_selectors
            .contains("[data-ad-name]"));
        assert!(
            bundled
                .url_cosmetic_resources("http://127.0.0.1:1234/")
                .generichide
        );
        assert!(bundled
            .check_network_request(
                &Request::new("https://ad.doubleclick.net/banner.js", top, "script", "GET")
                    .unwrap()
            )
            .should_block());
        let bundled_popup_rules = [
            popup_rules(include_str!("../filters/easylist.txt")),
            popup_rules(include_str!(
                "../filters/upstream/uBlockOrigin/uAssets/filters/filters.txt"
            )),
            include_str!("../filters/popupads-hagezi.txt").to_owned(),
        ]
        .join("\n");
        let bundled_popups = engine(&bundled_popup_rules);
        assert!(bundled_popups
            .check_network_request(
                &Request::new("https://adsession.com/redirect", top, "other", "GET").unwrap()
            )
            .should_block());
        assert!(bundled_popups
            .check_network_request(
                &Request::new(
                    "https://dogsprotectedfeed.info/landing",
                    top,
                    "other",
                    "GET"
                )
                .unwrap()
            )
            .should_block());
        assert!(bundled_popups
            .check_network_request(
                &Request::new(
                    "https://fd01ce9a02.0012e30263.com/landing",
                    top,
                    "other",
                    "GET"
                )
                .unwrap()
            )
            .should_block());
    }

    #[test]
    fn validated_request_site_context_preserves_filtering_without_reparsing_hosts() {
        let blocker = Blocker {
            engine: engine("||ads.example^"),
            popup_engine: engine(""),
            gambling_domains: GamblingDomains::empty(),
            mode: AdblockMode::Balanced,
            disabled: HashSet::new(),
            exceptions_path: PathBuf::new(),
            blocked: 0,
            redirected: 0,
            bootstraps: HashMap::new(),
            explicit_navigation_tickets: HashMap::new(),
        };
        let top = "https://news.example/article";
        let source = "https://news.example/frame";
        let url = "https://ads.example/banner.js";
        let top_host = site(top).unwrap();
        let top_domain = registrable_domain(top);

        let legacy_action = blocker.filter_action(url, top, source, "script", "GET");
        let contextual_action = blocker.filter_action_with_site_context(
            url,
            source,
            "script",
            "GET",
            FilterSiteContext {
                top_host: &top_host,
                top_registrable_domain: top_domain.as_deref(),
                source_is_http_site: is_http_site(source),
            },
        );
        assert!(matches!(legacy_action, Some(FilterAction::Block)));
        assert!(matches!(contextual_action, Some(FilterAction::Block)));
        assert!(is_http_site(top));
        assert!(!is_http_site("evergreen://newtab"));

        assert!(blocker
            .filter_action_with_site_context(
                url,
                "evergreen://newtab",
                "script",
                "GET",
                FilterSiteContext {
                    top_host: &top_host,
                    top_registrable_domain: top_domain.as_deref(),
                    source_is_http_site: false,
                }
            )
            .is_none());
    }

    #[test]
    fn video_first_keeps_media_and_common_segments_while_filtering_other_requests() {
        let blocker = Blocker {
            engine: engine("||ads.example^"),
            popup_engine: engine(""),
            gambling_domains: GamblingDomains::empty(),
            mode: AdblockMode::VideoFirst,
            disabled: HashSet::new(),
            exceptions_path: PathBuf::new(),
            blocked: 0,
            redirected: 0,
            bootstraps: HashMap::new(),
            explicit_navigation_tickets: HashMap::new(),
        };
        let top = "https://news.example/article";

        assert!(blocker
            .filter_action("https://ads.example/video", top, top, "media", "GET")
            .is_none());
        assert!(blocker
            .filter_action(
                "https://ads.example/video/segment.M4S?token=1",
                top,
                top,
                "xmlhttprequest",
                "GET"
            )
            .is_none());
        assert!(matches!(
            blocker.filter_action("https://ads.example/advert.js", top, top, "script", "GET"),
            Some(FilterAction::Block)
        ));
        assert!(matches!(
            blocker.filter_action(
                "https://ads.example/collect",
                top,
                top,
                "xmlhttprequest",
                "GET"
            ),
            Some(FilterAction::Block)
        ));
    }

    #[test]
    fn strong_mode_blocks_known_third_party_gambling_domains_only() {
        let blocker = Blocker {
            engine: engine(""),
            popup_engine: engine(""),
            gambling_domains: GamblingDomains::bundled(),
            mode: AdblockMode::Strong,
            disabled: HashSet::new(),
            exceptions_path: PathBuf::new(),
            blocked: 0,
            redirected: 0,
            bootstraps: HashMap::new(),
            explicit_navigation_tickets: HashMap::new(),
        };
        let top = "https://news.example/article";
        assert!(matches!(
            blocker.filter_action(
                "https://cdn.bet365.com.au/widget.js",
                top,
                top,
                "script",
                "GET"
            ),
            Some(FilterAction::Block)
        ));
        assert!(blocker
            .filter_action(
                "https://bet365.com.au/widget.js",
                "https://shop.bet365.com.au/page",
                "https://shop.bet365.com.au/page",
                "script",
                "GET"
            )
            .is_none());
        // Sibling subdomains are same-site when their public-suffix-derived
        // registrable domain is equal, even though neither host suffix-matches
        // the other.
        assert!(blocker
            .filter_action(
                "https://cdn.bet365.com.au/widget.js",
                "https://shop.bet365.com.au/page",
                "https://shop.bet365.com.au/page",
                "script",
                "GET"
            )
            .is_none());
        assert!(blocker
            .filter_action(
                "https://unknown-casino.example/widget.js",
                top,
                top,
                "script",
                "GET"
            )
            .is_none());
    }

    #[test]
    fn same_site_uses_psl_registrable_domains_and_preserves_host_boundaries() {
        assert!(hosts_share_domain(
            "cdn.bet365.com.au",
            "shop.bet365.com.au"
        ));
        assert!(hosts_share_domain(
            "video.cdn.example.co.uk",
            "shop.example.co.uk"
        ));
        assert!(hosts_share_domain("example.com.", "cdn.example.com"));
        assert!(hosts_share_domain("localhost", "localhost"));

        assert!(!hosts_share_domain("example.com", "example.net"));
        assert!(!hosts_share_domain("bet365.com.au", "bet365.co.nz"));
        assert!(!hosts_share_domain("example.com", "example.com.evil"));
    }

    #[test]
    fn playback_repair_disables_once_and_persists_site_exception() {
        let run_id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "pip-playback-repair-{}-{run_id}",
            std::process::id()
        ));
        let mut blocker = Blocker::load(directory.clone(), AdblockMode::Balanced);

        assert_eq!(
            blocker
                .disable_for_playback("https://www.youtube.com/watch?v=test")
                .unwrap(),
            Some(true)
        );
        assert_eq!(blocker.enabled("https://www.youtube.com/"), Some(false));
        assert_eq!(
            blocker
                .disable_for_playback("https://www.youtube.com/watch?v=again")
                .unwrap(),
            Some(false),
            "using repair a second time must not re-enable the blocker"
        );

        let reloaded = Blocker::load(directory.clone(), AdblockMode::Balanced);
        assert_eq!(reloaded.enabled("https://www.youtube.com/"), Some(false));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn bundled_redirect_resources_are_available_to_the_webview_response_layer() {
        let mut redirect_engine = engine("||ads.example^$script,redirect=noop.js");
        redirect_engine.use_resources([Resource {
            name: "noop.js".into(),
            aliases: Vec::new(),
            kind: adblock::resources::ResourceType::Mime(
                adblock::resources::MimeType::ApplicationJavascript,
            ),
            content: "bm9vcCgpOw==".into(),
            dependencies: Vec::new(),
            permission: PermissionMask::default(),
        }]);
        let request = Request::new(
            "https://ads.example/ad.js",
            "https://news.example/",
            "script",
            "GET",
        )
        .unwrap();
        let response = redirect_engine.check_network_request(&request);
        assert!(response.should_block());
        let (mime, body) = decode_redirect_data_uri(response.redirect.as_deref().unwrap()).unwrap();
        assert_eq!(mime, "application/javascript");
        assert_eq!(body, b"noop();");
    }

    #[test]
    fn bundled_ublock_network_rules_redirect_known_ad_scripts_to_local_noop() {
        let blocker = Blocker::load(
            std::env::temp_dir().join("pip-unused-test-path"),
            AdblockMode::Balanced,
        );
        let action = blocker
            .filter_action(
                "https://go.oclasrv.com/apu.php",
                "https://news.example/",
                "https://news.example/",
                "script",
                "GET",
            )
            .expect("pinned uBlock filter should match");
        let FilterAction::Redirect(data_uri) = action else {
            panic!("uBlock redirect rule was reduced to an empty block");
        };
        let (mime, body) = decode_redirect_data_uri(&data_uri).unwrap();
        assert_eq!(mime, "application/javascript");
        assert_eq!(body, b"void 0;");
    }

    #[test]
    fn redirect_data_uri_decoder_rejects_untrusted_schemes_and_malformed_mime() {
        assert!(decode_redirect_data_uri("https://example.test/redirect.js").is_none());
        assert!(decode_redirect_data_uri("data:text/html;name=x;base64,PGI+").is_none());
        assert_eq!(
            decode_redirect_data_uri(
                "data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw=="
            )
            .unwrap()
            .0,
            "image/gif"
        );
    }

    #[test]
    fn popup_filters_block_listed_or_automatic_popups_only() {
        let source = "https://news.example/article";
        let blocker = Blocker {
            engine: engine(""),
            popup_engine: engine(&popup_rules(
                "||casino.example^$popup\n||tracker.example^$popup,third-party\n@@||trusted.example^$popup,domain=news.example",
            )),
            gambling_domains: GamblingDomains::empty(),
            mode: AdblockMode::Balanced,
            disabled: HashSet::new(),
            exceptions_path: PathBuf::new(),
            blocked: 0,
            redirected: 0,
            bootstraps: HashMap::new(),
            explicit_navigation_tickets: HashMap::new(),
        };
        assert!(blocker.should_block_popup("https://random.example/", source, false));
        assert!(blocker.should_block_popup("https://casino.example/", source, true));
        assert!(blocker.should_block_popup("https://tracker.example/", source, true));
        assert!(!blocker.should_block_popup("https://trusted.example/", source, true));
        assert!(!blocker.should_block_popup("https://login.example/", source, true));
    }

    #[test]
    fn pip_popup_overrides_are_translated_into_active_network_rules() {
        let popup_engine =
            popup_engine_from_sources([include_str!("../filters/popup-overrides.txt")]);
        let request = Request::new(
            "https://ad88vip6.com/home/register?id=940814853",
            "https://news.example/article",
            "other",
            "GET",
        )
        .expect("valid popup destination request");

        assert!(popup_engine.check_network_request(&request).should_block());
    }

    #[test]
    fn known_gambling_popup_domain_is_blocked_even_when_webview_marks_it_user_initiated() {
        let blocker = Blocker {
            engine: engine(""),
            popup_engine: engine(include_str!("../filters/popup-overrides.txt")),
            gambling_domains: GamblingDomains::empty(),
            mode: AdblockMode::Balanced,
            disabled: HashSet::new(),
            exceptions_path: PathBuf::new(),
            blocked: 0,
            redirected: 0,
            bootstraps: HashMap::new(),
            explicit_navigation_tickets: HashMap::new(),
        };
        assert!(blocker.should_block_popup(
            "https://ad88vip6.com/home/register?id=940814853",
            "https://news.example/article",
            true,
        ));
    }

    #[test]
    fn nested_popup_from_about_blank_keeps_opener_filter_context() {
        let blocker = Blocker {
            engine: engine(""),
            popup_engine: engine(&popup_rules("||tracker.example^$popup,third-party")),
            gambling_domains: GamblingDomains::empty(),
            mode: AdblockMode::Balanced,
            disabled: HashSet::new(),
            exceptions_path: PathBuf::new(),
            blocked: 0,
            redirected: 0,
            bootstraps: HashMap::new(),
            explicit_navigation_tickets: HashMap::new(),
        };
        let opener = "https://news.example/article";
        let popup_context = PopupNavigationContext::from_opener(Some(opener.into()));
        let inherited_opener = popup_context.opener_for_source("about:blank");

        assert!(blocker.should_block_popup_with_popup_context(
            "https://ad88vip6.com/home/register?id=940814853",
            "about:blank",
            inherited_opener.as_deref(),
            true,
        ));
        assert!(blocker.should_block_popup_with_popup_context(
            "https://tracker.example/ad",
            "about:blank",
            inherited_opener.as_deref(),
            true,
        ));
        // A normal user-clicked link from the same blank popup remains available.
        assert!(!blocker.should_block_popup_with_popup_context(
            "https://accounts.google.com/",
            "about:blank",
            inherited_opener.as_deref(),
            true,
        ));
    }

    #[test]
    fn current_tab_redirect_filter_catches_gambling_ads_without_blocking_normal_navigation() {
        let blocker = Blocker {
            engine: engine(""),
            popup_engine: engine(include_str!("../filters/popup-overrides.txt")),
            gambling_domains: GamblingDomains::empty(),
            mode: AdblockMode::Balanced,
            disabled: HashSet::new(),
            exceptions_path: PathBuf::new(),
            blocked: 0,
            redirected: 0,
            bootstraps: HashMap::new(),
            explicit_navigation_tickets: HashMap::new(),
        };
        let source = "https://news.example/article";
        assert!(blocker
            .should_block_navigation("https://ad88vip6.com/home/register?id=940814853", source,));
        assert!(!blocker.should_block_navigation("https://accounts.google.com/", source));
        assert!(!blocker.should_block_navigation(
            "https://ad88vip6.com/home/register?id=940814853",
            "evergreen://newtab",
        ));
        assert!(blocker.should_block_navigation_with_popup_context(
            "https://ad88vip6.com/home/register?id=940814853",
            "about:blank",
            Some(source),
        ));
        assert!(!blocker.should_block_navigation_with_popup_context(
            "https://accounts.google.com/",
            "about:blank",
            Some(source),
        ));
        assert!(!blocker.should_block_navigation_with_popup_context(
            "https://ad88vip6.com/home/register?id=940814853",
            "evergreen://newtab",
            None,
        ));
    }

    #[test]
    fn top_level_navigation_applies_document_rules_but_preserves_normal_sites() {
        let blocker = Blocker {
            engine: engine("||blocked.example^$document"),
            popup_engine: engine(include_str!("../filters/popup-overrides.txt")),
            gambling_domains: GamblingDomains::empty(),
            mode: AdblockMode::Balanced,
            disabled: HashSet::new(),
            exceptions_path: PathBuf::new(),
            blocked: 0,
            redirected: 0,
            bootstraps: HashMap::new(),
            explicit_navigation_tickets: HashMap::new(),
        };

        assert!(blocker.should_block_navigation(
            "https://blocked.example/landing",
            "https://publisher.example/article",
        ));
        assert!(!blocker.should_block_navigation(
            "https://accounts.google.com/",
            "https://publisher.example/article",
        ));
        assert!(!blocker
            .should_block_navigation("https://blocked.example/landing", "evergreen://newtab",));
    }

    #[test]
    fn explicit_navigation_tickets_are_exact_and_one_shot() {
        let mut blocker = Blocker {
            engine: engine(""),
            popup_engine: engine(include_str!("../filters/popup-overrides.txt")),
            gambling_domains: GamblingDomains::empty(),
            mode: AdblockMode::Balanced,
            disabled: HashSet::new(),
            exceptions_path: PathBuf::new(),
            blocked: 0,
            redirected: 0,
            bootstraps: HashMap::new(),
            explicit_navigation_tickets: HashMap::new(),
        };
        blocker.issue_explicit_navigation_ticket(
            42,
            "https://blocked.example/landing",
            "https://publisher.example/article",
        );

        assert!(!blocker.consume_explicit_navigation_ticket(
            43,
            "https://blocked.example/landing",
            "https://publisher.example/article",
        ));
        assert!(!blocker.consume_explicit_navigation_ticket(
            42,
            "https://other.example/landing",
            "https://publisher.example/article",
        ));
        assert!(blocker.consume_explicit_navigation_ticket(
            42,
            "https://blocked.example/landing",
            "https://publisher.example/article",
        ));
        assert!(!blocker.consume_explicit_navigation_ticket(
            42,
            "https://blocked.example/landing",
            "https://publisher.example/article",
        ));
    }

    #[test]
    fn popup_opener_context_survives_blank_commit_and_clears_after_http_commit() {
        let mut context =
            PopupNavigationContext::from_opener(Some("https://news.example/article".into()));
        assert_eq!(
            context.opener_for_source("about:blank").as_deref(),
            Some("https://news.example/article")
        );
        context.navigation_committed("about:blank");
        assert!(context.opener_for_source("about:blank").is_some());
        context.navigation_committed("https://accounts.google.com/");
        assert!(context.opener_for_source("about:blank").is_none());
    }

    #[test]
    fn iframe_navigation_filter_blocks_suspicious_cross_site_destinations_and_honors_exceptions() {
        let mut blocker = Blocker {
            engine: engine(""),
            popup_engine: engine(include_str!("../filters/popup-overrides.txt")),
            gambling_domains: GamblingDomains::empty(),
            mode: AdblockMode::Balanced,
            disabled: HashSet::new(),
            exceptions_path: PathBuf::new(),
            blocked: 0,
            redirected: 0,
            bootstraps: HashMap::new(),
            explicit_navigation_tickets: HashMap::new(),
        };
        let top_level = "https://news.example/article";
        assert!(blocker.should_block_frame_navigation(
            "https://ad88vip6.com/home/register?id=940814853",
            top_level,
        ));
        assert!(
            !blocker.should_block_frame_navigation("https://video.example/embed/123", top_level,)
        );
        assert!(!blocker
            .should_block_frame_navigation("https://news.example/embedded-content", top_level,));
        blocker.disabled.insert("news.example".into());
        assert!(!blocker.should_block_frame_navigation(
            "https://ad88vip6.com/home/register?id=940814853",
            top_level,
        ));
    }

    #[test]
    fn popup_blank_frame_filter_uses_pending_opener_context_only_for_popup_tabs() {
        let blocker = Blocker {
            engine: engine(""),
            popup_engine: engine(&popup_rules("||tracker.example^$popup,third-party")),
            gambling_domains: GamblingDomains::empty(),
            mode: AdblockMode::Balanced,
            disabled: HashSet::new(),
            exceptions_path: PathBuf::new(),
            blocked: 0,
            redirected: 0,
            bootstraps: HashMap::new(),
            explicit_navigation_tickets: HashMap::new(),
        };
        let opener = "https://news.example/article";

        assert!(blocker.should_block_frame_navigation_with_popup_context(
            "https://ad88vip6.com/home/register?id=940814853",
            "about:blank",
            Some(opener),
        ));
        assert!(blocker.should_block_frame_navigation_with_popup_context(
            "https://tracker.example/ad-frame",
            "about:blank",
            Some(opener),
        ));
        assert!(!blocker.should_block_frame_navigation_with_popup_context(
            "https://video.example/embed/123",
            "about:blank",
            Some(opener),
        ));

        // Ordinary blank tabs have no opener context and preserve the existing
        // behavior, even for a destination that is filtered in popup context.
        assert!(!blocker.should_block_frame_navigation(
            "https://ad88vip6.com/home/register?id=940814853",
            "about:blank",
        ));
        assert!(!blocker
            .should_block_frame_navigation("https://tracker.example/ad-frame", "about:blank",));
    }

    #[test]
    fn gambling_category_popup_fallback_catches_new_domains_without_blocking_normal_links() {
        let blocker = Blocker {
            engine: engine(""),
            popup_engine: engine(""),
            gambling_domains: GamblingDomains::empty(),
            mode: AdblockMode::Balanced,
            disabled: HashSet::new(),
            exceptions_path: PathBuf::new(),
            blocked: 0,
            redirected: 0,
            bootstraps: HashMap::new(),
            explicit_navigation_tickets: HashMap::new(),
        };
        let source = "https://news.example/article";
        assert!(blocker.should_block_popup(
            "https://bonus-casino88.example/register",
            source,
            true
        ));
        assert!(blocker.should_block_popup("https://bet365.example/offer", source, true));
        assert!(blocker.should_block_popup("https://slots.example/", source, true));
        assert!(blocker.should_block_popup("https://789bet.example/", source, true));
        assert!(blocker.should_block_popup("https://ad88vip6.example/", source, true));
        assert!(blocker.should_block_popup(
            "https://rotating-cdn.example/casino/register?id=940814853",
            source,
            true
        ));
        assert!(blocker.should_block_popup(
            "https://redirect.example/out?target=789bet",
            source,
            true
        ));
        assert!(
            blocker.should_block_navigation("https://redirect.example/out?target=789bet", source,)
        );
        assert!(!blocker.should_block_popup("https://accounts.google.com/", source, true));
        assert!(!blocker.should_block_popup(
            "https://offers.example/bet?campaign=spring",
            source,
            true
        ));
        assert!(!looks_like_gambling_host("sports"));
        assert!(!blocker.should_block(
            "https://789bet.example/",
            "https://789bet.example/",
            "https://789bet.example/",
            "document",
            "GET"
        ));
        assert!(!blocker.should_block(
            "https://bonus-casino88.example/register",
            source,
            source,
            "document",
            "GET"
        ));
    }

    #[test]
    fn popup_fallback_decodes_redirect_keywords_without_changing_regular_navigation() {
        let blocker = Blocker {
            engine: engine(""),
            popup_engine: engine(""),
            gambling_domains: GamblingDomains::empty(),
            mode: AdblockMode::Balanced,
            disabled: HashSet::new(),
            exceptions_path: PathBuf::new(),
            blocked: 0,
            redirected: 0,
            bootstraps: HashMap::new(),
            explicit_navigation_tickets: HashMap::new(),
        };
        let source = "https://news.example/article";

        // The destination keyword is percent-encoded twice by a redirector.
        assert!(blocker.should_block_popup(
            "https://redirect.example/out?target=%2563%2561%2573%2569%256E%256F",
            source,
            true,
        ));
        assert!(blocker.should_block_popup(
            "https://redirect.example/out?target=%63asino",
            source,
            true,
        ));
        assert!(!blocker.should_block_popup(
            "https://redirect.example/out?target=account-settings",
            source,
            true,
        ));
        assert!(!blocker.should_block_navigation(
            "https://redirect.example/out?target=%2563%2561%2573%2569%256E%256F",
            "evergreen://newtab",
        ));
    }

    #[test]
    fn bundled_gambling_domains_block_popup_hosts_and_subdomains_only() {
        let blocker = Blocker {
            engine: engine(""),
            popup_engine: engine(""),
            gambling_domains: GamblingDomains::bundled(),
            mode: AdblockMode::Balanced,
            disabled: HashSet::new(),
            exceptions_path: PathBuf::new(),
            blocked: 0,
            redirected: 0,
            bootstraps: HashMap::new(),
            explicit_navigation_tickets: HashMap::new(),
        };
        let source = "https://news.example/article";
        assert!(blocker.should_block_popup("https://bet365.com.au/", source, true));
        assert!(blocker.should_block_popup("https://bet365.com.au./", source, true));
        assert!(blocker.should_block_popup("https://www.bet365.com.au/", source, true));
        assert!(blocker.should_block_popup("https://promo.12betsports.best/", source, true));
        assert!(!blocker.should_block_popup("https://accounts.google.com/", source, true));
        assert!(!blocker.should_block("https://bet365.com.au/", source, source, "document", "GET"));
        assert!(!blocker.should_block(
            "https://12betsports.best/",
            source,
            source,
            "document",
            "GET"
        ));
    }

    #[test]
    fn production_blocker_uses_bundled_popup_domains_for_popup_redirect_and_frame_paths() {
        // This randomized domain is present only in the bundled HaGeZi popup
        // list, so this exercises the production Blocker::load wiring rather
        // than a hand-built adblock engine containing a duplicate rule.
        let blocker = Blocker::load(
            PathBuf::from("target/adblock-test-hagezi-popup"),
            AdblockMode::Balanced,
        );
        let source = "https://news.example/article";
        let destination = "https://fd01ce9a02.0012e30263.com/landing";
        let unrelated = "https://accounts.google.com/";

        assert!(matches_hagezi_popup_domain("fd01ce9a02.0012e30263.com"));
        assert!(blocker.should_block_popup(destination, source, true));
        assert!(blocker.should_block_navigation(destination, source));
        assert!(blocker.should_block_frame_navigation(destination, source));

        let popup_context = PopupNavigationContext::from_opener(Some(source.to_owned()));
        let inherited_opener = popup_context.opener_for_source("about:blank");
        assert!(blocker.should_block_popup_with_popup_context(
            destination,
            "about:blank",
            inherited_opener.as_deref(),
            true,
        ));
        assert!(blocker.should_block_navigation_with_popup_context(
            destination,
            "about:blank",
            inherited_opener.as_deref(),
        ));
        assert!(blocker.should_block_frame_navigation_with_popup_context(
            destination,
            "about:blank",
            inherited_opener.as_deref(),
        ));

        assert!(!matches_hagezi_popup_domain(
            "evilfd01ce9a02.0012e30263.com"
        ));
        assert!(!blocker.should_block_popup(unrelated, source, true));
        assert!(!blocker.should_block_navigation(unrelated, source));
        assert!(!blocker.should_block_frame_navigation(unrelated, source));
    }

    #[test]
    fn generated_gambling_domains_are_a_stable_sorted_unique_canonical_source() {
        let mut domains: Vec<&str> = GAMBLING_DOMAIN_INPUT
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .collect();
        domains.sort_unstable();
        domains.dedup();

        let expected_source = format!("{}\n", domains.join("\n"));
        assert_eq!(
            GAMBLING_DOMAIN_SOURCE.as_bytes(),
            expected_source.as_bytes()
        );
        assert_eq!(GAMBLING_DOMAIN_OFFSETS.len(), domains.len());

        let mut expected_offsets = Vec::with_capacity(domains.len());
        let mut byte_offset = 0usize;
        for domain in &domains {
            expected_offsets.push(u32::try_from(byte_offset).unwrap());
            byte_offset += domain.len() + 1;
        }
        assert_eq!(GAMBLING_DOMAIN_OFFSETS.as_slice(), expected_offsets);
        for (index, domain) in domains.iter().enumerate() {
            assert_eq!(domain_at(GAMBLING_DOMAIN_OFFSETS[index]), *domain);
        }
    }

    #[test]
    fn generated_gambling_domain_lookup_matches_exact_domains_and_subdomains() {
        let domains = GamblingDomains::bundled();

        assert!(matches_gambling_domain("bet365.com.au", &domains));
        assert!(matches_gambling_domain("bet365.com.au.", &domains));
        assert!(matches_gambling_domain("www.bet365.com.au", &domains));
        assert!(matches_gambling_domain("promo.12betsports.best.", &domains));
        assert!(matches_gambling_domain("promo.12betsports.best", &domains));
        assert!(!matches_gambling_domain("notbet365.com.au", &domains));
        assert!(!matches_gambling_domain("bet365.com.au.evil", &domains));
        assert!(!matches_gambling_domain("accounts.google.com", &domains));
    }

    #[test]
    fn cosmetic_rules_respect_site_exceptions() {
        let mut b = Blocker {
            engine: engine(
                "news.example##.sponsored\nnews.example#@#.allowed\nnews.example##.allowed",
            ),
            popup_engine: engine(""),
            gambling_domains: GamblingDomains::empty(),
            mode: AdblockMode::Balanced,
            disabled: HashSet::new(),
            exceptions_path: PathBuf::new(),
            blocked: 0,
            redirected: 0,
            bootstraps: HashMap::new(),
            explicit_navigation_tickets: HashMap::new(),
        };
        let script = b.cosmetic_script("https://news.example/").unwrap();
        assert!(script.contains(".sponsored"));
        assert!(!script.contains(".allowed"));
        assert!(b.cosmetic_script("evergreen://newtab").is_none());
        b.disabled.insert("news.example".into());
        assert!(b.cosmetic_script("https://news.example/").is_none());
    }

    #[test]
    fn procedural_cosmetic_rules_are_serialized_for_bounded_page_execution() {
        let script = procedural_test_script("https://example.test/");
        assert!(script.contains("has-text"));
        assert!(script.contains("remove"));
        assert!(
            script.contains("Casino"),
            "procedural filter was not included"
        );
        assert!(
            script.contains(".ad"),
            "procedural CSS selector was not included"
        );
        assert!(script.contains("MutationObserver"));
        assert!(script.contains("runs>=16"));
        assert!(
            script.contains("if(runs>=16)observer.disconnect()"),
            "cosmetic observer should release its DOM subscription after the final bounded pass"
        );
    }
}
