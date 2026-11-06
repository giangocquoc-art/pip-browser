"""Render built-in Pip surfaces in Edge headless for visual review."""
import json
from pathlib import Path
import re
import sys

root = Path(__file__).resolve().parents[1]
out = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else root / "ui-review"
out.mkdir(parents=True, exist_ok=True)


def source_template(file: str, name: str) -> str:
    text = (root / file).read_text(encoding="utf-8")
    match = re.search(rf"pub const {name}: &str = r#\"(.*?)\"#;", text, re.S)
    if not match:
        raise RuntimeError(f"Cannot extract {name} from {file}")
    return match.group(1)


def asset(file: str) -> str:
    return (root / file).read_text(encoding="ascii").strip()


logo = asset("crates/app/ui/logo.b64")
installer = source_template("crates/installer/src/installer_ui.rs", "INSTALLER_TEMPLATE")
installer = (
    installer.replace("{{LOGO_BASE64}}", logo)
    .replace("{{INSTALLER_TITLE}}", "Giấy phép và tùy chọn")
    .replace("{{UNINSTALL_MODE}}", "false")
    .replace("{{STEP0_ACTIVE}}", "")
    .replace("{{STEP_UNINSTALL0_ACTIVE}}", "")
)
installer = installer.replace('class="step-view" id="step1"', 'class="step-view active" id="step1"')
(out / "installer.html").write_text(installer, encoding="utf-8")

settings = source_template("crates/app/src/settings_ui.rs", "SETTINGS_TEMPLATE")
settings = (
    settings.replace("{{LOGO_BASE64}}", logo)
    .replace("{{MASCOT_BASE64}}", asset("crates/app/ui/assets/mascot-clean.webp.b64"))
    .replace("{{PROFILES_JSON}}", '[{"id":"Default","name":"Cá nhân","autofill":false}]')
    .replace("{{DEFAULT_DOWNLOAD_FOLDER}}", "C:\\Users\\Example\\Downloads")
    .replace("{{PERSISTENT_SITES_JSON}}", "[]")
    .replace("{{PERSISTENT_SITES_CHIPS}}", "")
    .replace("{{GOOGLE_DNS_ENABLED}}", "false")
    .replace("{{GOOGLE_DNS_ACTIVE}}", "false")
    .replace("{{GOOGLE_DNS_FAILED}}", "false")
    .replace("{{GOOGLE_DNS_ROUTE_OK}}", "null")
    .replace("{{CURRENT_PROFILE}}", "Default")
    .replace("{{CURRENT_PRIVATE}}", "false")
    .replace("Đang kiểm tra...", "v155.0.4283.45 (Active)")
)
(out / "settings.html").write_text(settings, encoding="utf-8")

sidebar = source_template("crates/app/src/sidebar_ui.rs", "SIDEBAR_TEMPLATE")
sidebar = (
    sidebar.replace("{{LOGO_BASE64}}", logo)
    .replace("{{MASCOT_BASE64}}", asset("crates/app/ui/assets/mascot-clean.webp.b64"))
    .replace("{{DOWNLOAD_MASCOT_BASE64}}", asset("crates/app/ui/assets/mascot-download.webp.b64"))
)
(out / "sidebar.html").write_text(sidebar, encoding="utf-8")

home = (root / "crates/app/ui/feather_newtab.html").read_text(encoding="utf-8")
for placeholder, value in {
    "{{FEATHER_ICON_BASE64}}": asset("crates/app/ui/feather_icon.b64"),
    "{{HERO_BASE64}}": asset("crates/app/ui/assets/home-hero.webp.b64"),
    "{{EMPTY_BASE64}}": asset("crates/app/ui/assets/mascot-clean.webp.b64"),
    "{{PRIVATE_HERO_BASE64}}": asset("crates/app/ui/assets/mascot-private.webp.b64"),
    "{{PRIVATE_CLASS}}": "",
    "{{PRIVATE_HIDDEN}}": "hidden",
    "{{THINKING_BASE64}}": asset("crates/app/ui/assets/mascot-clean.webp.b64"),
    "{{SEARCH_ENGINE_NAME}}": "Google",
    "{{SEARCH_ENGINE_URL}}": "https://www.google.com/search?q=%s",
    "{{INITIAL_VIEW}}": "home",
}.items():
    home = home.replace(placeholder, value)
(out / "home.html").write_text(home, encoding="utf-8")
private_home = home.replace('<body class="">', '<body class=" private">')
private_home = private_home.replace('aria-label="Trang chủ ẩn danh" hidden', 'aria-label="Trang chủ ẩn danh"')
(out / "private-home.html").write_text(private_home, encoding="utf-8")

error_source = (root / "crates/app/src/error_ui.rs").read_text(encoding="utf-8")
error_match = re.search(r'r#"(.*?)"#,', error_source, re.S)
if not error_match:
    raise RuntimeError("Cannot extract 404 page")
error_html = error_match.group(1).replace("{{", "{").replace("}}", "}")
error_html = error_html.replace("{}", asset("crates/app/ui/assets/mascot-404.webp.b64"))
(out / "error.html").write_text(error_html, encoding="utf-8")

chrome = (root / "crates/app/ui/index.html").read_text(encoding="utf-8")
for placeholder, value in {
    "{{LOGO_BASE64}}": logo,
    "{{SEARCH_ENGINE_NAME}}": "Google",
    "{{PROFILE_LABEL}}": "Cá nhân",
    "{{PRIVATE_LABEL}}": "",
}.items():
    chrome = chrome.replace(placeholder, value)
chrome = chrome.replace(
    "</body>",
    "<style>body{background:#f6f0ea!important}</style>"
    "<script>window.__renderProfileMenu([{id:'Default',name:'Cá nhân'},{id:'PipWork',name:'Công việc'},{id:'PipFamily',name:'Gia đình'}],'Default',false);toggleProfileMenu()</script></body>",
)
(out / "chrome-profile-menu.html").write_text(chrome, encoding="utf-8")

report = {
    "status": "templates-generated",
    "templates": ["home.html", "private-home.html", "settings.html", "sidebar.html", "error.html", "installer.html", "chrome-profile-menu.html"],
    "verification": "Not performed by this renderer. Run node scripts/visual_e2e_playwright.js and inspect ui-review/qa/report.json; run cargo run -p evergreen-browser --locked -- --verify-native-ui for the real Windows app.",
}
(out / "evidence.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
print(out)
