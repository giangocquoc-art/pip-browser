//! Static Pip startup/loading screen for the internal `evergreen://loading` route.

const LOADING_MASCOT_BASE64: &str = include_str!("../ui/assets/mascot-loading.webp.b64");

pub fn get_loading_html() -> String {
    format!(
        r#"<!doctype html><html lang="vi"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Đang tải — Pip</title><style>
        *{{box-sizing:border-box}}html,body{{margin:0;min-height:100%;font:14px/1.5 "Segoe UI Variable Text","Segoe UI",system-ui,sans-serif;color:#17202e;background:#fffaf6}}body{{min-height:100vh;display:grid;place-items:center;padding:24px}}.card{{width:min(760px,100%);min-height:440px;display:flex;flex-direction:column;align-items:center;justify-content:center;padding:30px;border:1px solid #f1e2d5;border-radius:20px;background:radial-gradient(ellipse at 50% 30%,#fff0e5 0,transparent 50%),#fff;box-shadow:0 10px 32px rgba(98,57,28,.06);text-align:center}}img{{width:min(320px,70vw);height:200px;object-fit:contain}}h1{{margin:8px 0 4px;font-size:22px;letter-spacing:-.5px}}*,*::before,*::after{{animation:none!important;transition:none!important;scroll-behavior:auto!important}}
        @media(prefers-color-scheme:dark){{html,body{{color:#f5f2ef;background:#171719}}.card{{background:radial-gradient(ellipse at 50% 30%,#30251f 0,transparent 50%),#222327;border-color:#39383a;box-shadow:0 10px 32px rgba(0,0,0,.24)}}}}
        </style></head><body><main class="card"><img src="data:image/webp;base64,{}" alt="Mascot Pip đang chờ"><h1>Đang khởi động Pip…</h1></main></body></html>"#,
        LOADING_MASCOT_BASE64.trim()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loading_page_embeds_mascot_without_animation() {
        let html = get_loading_html();
        assert!(html.contains("Đang khởi động Pip…"));
        assert!(html.contains("data:image/webp;base64,"));
        assert!(html.contains("animation:none"));
        assert!(!html.contains("aria-valuenow"));
    }
}
