//! Feather home page, replacing only the original static Evergreen template.
//! All actual navigation remains delegated through the original IPC contract.

pub const HOME_TEMPLATE: &str = include_str!("../ui/feather_newtab.html");
const FEATHER_ICON_BASE64: &str = include_str!("../ui/feather_icon.b64");
const HERO_BASE64: &str = include_str!("../ui/assets/home-hero.webp.b64");
const EMPTY_BASE64: &str = include_str!("../ui/assets/mascot-clean.webp.b64");
const PRIVATE_HERO_BASE64: &str = include_str!("../ui/assets/mascot-private.webp.b64");

pub fn get_home_html(search_name: &str, view: &str, private: bool) -> String {
    HOME_TEMPLATE
        .replace("{{FEATHER_ICON_BASE64}}", FEATHER_ICON_BASE64.trim())
        .replace("{{HERO_BASE64}}", HERO_BASE64.trim())
        .replace("{{EMPTY_BASE64}}", EMPTY_BASE64.trim())
        .replace("{{PRIVATE_HERO_BASE64}}", PRIVATE_HERO_BASE64.trim())
        .replace("{{PRIVATE_CLASS}}", if private { " private" } else { "" })
        .replace("{{PRIVATE_HIDDEN}}", if private { "" } else { "hidden" })
        .replace("{{SEARCH_ENGINE_NAME}}", search_name)
        .replace(
            "{{INITIAL_VIEW}}",
            match view {
                "bookmarks" => "bookmarks",
                "history" => "history",
                _ => "home",
            },
        )
}

#[cfg(test)]
mod tests {
    use super::get_home_html;

    #[test]
    fn private_home_shows_mascot_and_google_dns_state_without_changing_regular_home() {
        let private = get_home_html("Google", "home", true);
        assert!(private.contains("class=\" private\""));
        assert!(private.contains("class=\"private-home\""));
        assert!(private.contains("CỬA SỔ ẨN DANH"));
        assert!(private.contains("Khuyến nghị nên bật"));
        assert!(private.contains("Google DNS"));
        assert!(private.contains("nút G DNS trên thanh công cụ"));
        assert!(!private.contains("{{PRIVATE_HERO_BASE64}}"));

        let regular = get_home_html("Google", "home", false);
        assert!(regular.contains("class=\"\""));
        assert!(regular.contains("class=\"private-home\" aria-label=\"Trang chủ ẩn danh\" hidden"));
        assert!(regular.contains("class=\"hero\" id=\"homeView\""));
    }
}
