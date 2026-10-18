//! One Brave engine shared by tabs; native filtering, cosmetic rules and upstream scriptlets.
use adblock::{
    lists::{FilterSet, ParseOptions},
    request::Request,
    resources::{PermissionMask, Resource},
    Engine,
};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::PathBuf,
    rc::{Rc, Weak},
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
    disabled: HashSet<String>,
    exceptions_path: PathBuf,
    pub blocked: u64,
    pub redirected: u64,
    bootstraps: HashMap<String, Weak<RefCell<Bootstrap>>>,
}

pub(crate) enum FilterAction {
    Block,
    Redirect(String),
}

#[derive(Default)]
struct Bootstrap {
    id: Option<String>,
}

const GAMBLING_DOMAIN_SOURCE: &str = include_str!("../filters/gambling-hagezi-mini-domains.txt");

struct GamblingDomains {
    offsets: Vec<u32>,
}

impl GamblingDomains {
    fn bundled() -> Self {
        let mut offsets = Vec::new();
        let mut offset = 0usize;
        for line in GAMBLING_DOMAIN_SOURCE.split_inclusive('\n') {
            let domain = line.trim();
            if !domain.is_empty() && !domain.starts_with('#') {
                offsets.push(u32::try_from(offset).expect("bundled filter list is under 4 GiB"));
            }
            offset += line.len();
        }
        offsets.sort_unstable_by(|left, right| domain_at(*left).cmp(domain_at(*right)));
        offsets.dedup_by(|left, right| domain_at(*left) == domain_at(*right));
        Self { offsets }
    }

    #[cfg(test)]
    fn empty() -> Self {
        Self {
            offsets: Vec::new(),
        }
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
    list.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('!') || line.starts_with('[') {
                return None;
            }
            let (pattern, options) = line.split_once('$')?;
            let options = options.split(',').map(str::trim).collect::<Vec<_>>();
            if !options
                .iter()
                .any(|option| matches!(*option, "popup" | "popunder"))
            {
                return None;
            }
            let remaining = options
                .into_iter()
                // Every request reaching this engine is a popup document, so
                // `$doc` is implicit and must not be checked against `other`.
                .filter(|option| !matches!(*option, "popup" | "popunder" | "doc" | "document"))
                .collect::<Vec<_>>();
            if remaining.is_empty() {
                Some(pattern.to_string())
            } else {
                Some(format!("{pattern}${}", remaining.join(",")))
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

impl Blocker {
    pub fn enabled(&self, url: &str) -> Option<bool> {
        Some(!self.disabled.contains(&site(url)?))
    }
    pub fn load(directory: PathBuf) -> Self {
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
        let popup_rules = [
            popup_rules(easylist),
            popup_rules(
                updated_lists
                    .as_ref()
                    .map(|lists| lists.ublock_filters.as_str())
                    .unwrap_or(include_str!(
                        "../filters/upstream/uBlockOrigin/uAssets/filters/filters.txt"
                    )),
            ),
            popup_rules(
                updated_lists
                    .as_ref()
                    .map(|lists| lists.ublock_quick_fixes.as_str())
                    .unwrap_or(include_str!(
                        "../filters/upstream/uBlockOrigin/uAssets/filters/quick-fixes.txt"
                    )),
            ),
            popup_rules(
                updated_lists
                    .as_ref()
                    .map(|lists| lists.ublock_unbreak.as_str())
                    .unwrap_or(include_str!(
                        "../filters/upstream/uBlockOrigin/uAssets/filters/unbreak.txt"
                    )),
            ),
            include_str!("../filters/popupads-hagezi.txt").to_owned(),
            include_str!("../filters/popup-overrides.txt").to_owned(),
        ]
        .join("\n");
        let popup_engine = engine(&popup_rules);
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
            disabled,
            exceptions_path,
            blocked: 0,
            redirected: 0,
            bootstraps: HashMap::new(),
        }
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

    pub(crate) fn filter_action(
        &self,
        url: &str,
        top: &str,
        source: &str,
        kind: &str,
        method: &str,
    ) -> Option<FilterAction> {
        let host = site(top)?;
        if self.disabled.contains(&host) || site(source).is_none() {
            return None;
        }
        // Never blank the top-level page. Media is classified by WebView2, not its extension.
        if kind == "document" {
            return None;
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

    #[cfg(test)]
    fn should_block(&self, url: &str, top: &str, source: &str, kind: &str, method: &str) -> bool {
        self.filter_action(url, top, source, kind, method).is_some()
    }

    pub(crate) fn should_block_popup(&self, url: &str, source: &str, user_initiated: bool) -> bool {
        let Some(source_host) = site(source) else {
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
        Request::new(url, source, "other", "GET")
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
apply();let runs=0,timer=0;const observer=new MutationObserver(records=>{{if(runs>=16||!records.some(record=>record.addedNodes.length))return;clearTimeout(timer);timer=setTimeout(()=>{{runs++;apply()}},300)}});if(document.documentElement)observer.observe(document.documentElement,{{childList:true,subtree:true}});return rules.length;
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
    let normalized_path = path_and_query.to_ascii_lowercase();
    let tokens = normalized_path
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty());
    tokens.into_iter().any(|token| {
        [
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
        .any(|term| token == *term || token.starts_with(term) || token.ends_with(term))
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
        let source = unsafe { request.Headers() }
            .ok()
            .and_then(|headers| {
                let mut raw = PWSTR::null();
                unsafe { headers.GetHeader(w!("Referer"), &mut raw) }.ok()?;
                Some(webview2_com::take_pwstr(raw))
            })
            .filter(|s| site(s).is_some())
            .unwrap_or_else(|| top.clone());
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
        let action = blocker
            .borrow()
            .filter_action(&url, &top, &source, kind, &method);
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

        let blocker = Blocker::load(directory.clone());
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
        let mut blocker = Blocker::load(std::env::temp_dir().join("pip-unused-test-path"));
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
    fn blocking_exceptions_and_video() {
        let mut b = Blocker {
            engine: engine("||ads.example^\n@@||ads.example/allowed.js$script"),
            popup_engine: engine(""),
            gambling_domains: GamblingDomains::empty(),
            disabled: HashSet::new(),
            exceptions_path: PathBuf::new(),
            blocked: 0,
            redirected: 0,
            bootstraps: HashMap::new(),
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
    fn playback_repair_disables_once_and_persists_site_exception() {
        let run_id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "pip-playback-repair-{}-{run_id}",
            std::process::id()
        ));
        let mut blocker = Blocker::load(directory.clone());

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

        let reloaded = Blocker::load(directory.clone());
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
        let blocker = Blocker::load(std::env::temp_dir().join("pip-unused-test-path"));
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
            disabled: HashSet::new(),
            exceptions_path: PathBuf::new(),
            blocked: 0,
            redirected: 0,
            bootstraps: HashMap::new(),
        };
        assert!(blocker.should_block_popup("https://random.example/", source, false));
        assert!(blocker.should_block_popup("https://casino.example/", source, true));
        assert!(blocker.should_block_popup("https://tracker.example/", source, true));
        assert!(!blocker.should_block_popup("https://trusted.example/", source, true));
        assert!(!blocker.should_block_popup("https://login.example/", source, true));
    }

    #[test]
    fn known_gambling_popup_domain_is_blocked_even_when_webview_marks_it_user_initiated() {
        let blocker = Blocker {
            engine: engine(""),
            popup_engine: engine(include_str!("../filters/popup-overrides.txt")),
            gambling_domains: GamblingDomains::empty(),
            disabled: HashSet::new(),
            exceptions_path: PathBuf::new(),
            blocked: 0,
            redirected: 0,
            bootstraps: HashMap::new(),
        };
        assert!(blocker.should_block_popup(
            "https://ad88vip6.com/home/register?id=940814853",
            "https://news.example/article",
            true,
        ));
    }

    #[test]
    fn gambling_category_popup_fallback_catches_new_domains_without_blocking_normal_links() {
        let blocker = Blocker {
            engine: engine(""),
            popup_engine: engine(""),
            gambling_domains: GamblingDomains::empty(),
            disabled: HashSet::new(),
            exceptions_path: PathBuf::new(),
            blocked: 0,
            redirected: 0,
            bootstraps: HashMap::new(),
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
        assert!(!blocker.should_block_popup("https://accounts.google.com/", source, true));
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
    fn bundled_gambling_domains_block_popup_hosts_and_subdomains_only() {
        let blocker = Blocker {
            engine: engine(""),
            popup_engine: engine(""),
            gambling_domains: GamblingDomains::bundled(),
            disabled: HashSet::new(),
            exceptions_path: PathBuf::new(),
            blocked: 0,
            redirected: 0,
            bootstraps: HashMap::new(),
        };
        let source = "https://news.example/article";
        assert!(blocker.should_block_popup("https://bet365.com.au/", source, true));
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
    fn cosmetic_rules_respect_site_exceptions() {
        let mut b = Blocker {
            engine: engine(
                "news.example##.sponsored\nnews.example#@#.allowed\nnews.example##.allowed",
            ),
            popup_engine: engine(""),
            gambling_domains: GamblingDomains::empty(),
            disabled: HashSet::new(),
            exceptions_path: PathBuf::new(),
            blocked: 0,
            redirected: 0,
            bootstraps: HashMap::new(),
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
    }
}
