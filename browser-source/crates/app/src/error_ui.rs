//! Static Pip not-found page for the internal `evergreen://404` route.

const MASCOT_404_BASE64: &str = include_str!("../ui/assets/mascot-404.webp.b64");

pub fn get_not_found_html() -> String {
    format!(
        r#"<!doctype html><html lang="vi"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Không tìm thấy trang — Pip</title><style>
        *{{box-sizing:border-box}}html,body{{margin:0;min-height:100%;font:15px/1.5 "Segoe UI Variable Text","Segoe UI",system-ui,sans-serif;color:#17202e;background:#fffaf6}}body{{min-height:100vh;display:grid;place-items:center;padding:24px}}.card{{width:min(900px,100%);min-height:390px;display:grid;grid-template-columns:1fr 1fr;align-items:center;gap:24px;padding:30px;border:1px solid #f1e2d5;border-radius:20px;background:linear-gradient(120deg,#fff 0%,#fff 54%,#fff0e5 100%);box-shadow:0 10px 32px rgba(98,57,28,.07)}}img{{width:100%;max-height:340px;object-fit:contain}}.eyebrow{{margin:0 0 8px;color:#f97316;font-size:11px;font-weight:800;letter-spacing:1.5px;text-transform:uppercase}}h1{{margin:0 0 10px;font-size:34px;line-height:1.12;letter-spacing:-1.1px}}p{{margin:0;color:#687589}}a{{display:inline-flex;margin-top:22px;padding:10px 19px;border-radius:22px;background:#c2410c;color:white;text-decoration:none;font-weight:650}}a:focus-visible{{outline:3px solid #fdba74;outline-offset:3px}}*,*::before,*::after{{animation:none!important;transition:none!important;scroll-behavior:auto!important}}@media(max-width:620px){{.card{{grid-template-columns:1fr;text-align:center;padding:24px}}img{{grid-row:1;max-height:230px}}h1{{font-size:29px}}}}
        @media(prefers-color-scheme:dark){{html,body{{color:#f5f2ef;background:#171719}}.card{{background:linear-gradient(120deg,#222327 0%,#222327 54%,#30251f 100%);border-color:#39383a;box-shadow:0 10px 32px rgba(0,0,0,.24)}}p{{color:#c1b8b0}}a{{background:#c2410c;color:#fff}}details button{{background:#292a2f!important;color:#f5f2ef!important;border-color:#704426!important}}}}
        </style></head><body><main class="card"><div><p class="eyebrow">Pip · 404</p><h1>Ối! Trang này không tồn tại.</h1><p>Có thể liên kết đã thay đổi hoặc trang không còn nữa.</p><a href="evergreen://newtab">Quay về trang chủ</a></div><img src="data:image/webp;base64,{}" alt="Mascot Pip cầm bảng 404"></main></body></html>"#,
        MASCOT_404_BASE64.trim()
    )
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
pub fn get_certificate_html(host: &str, url: &str, tab_id: u64) -> String {
    let message=serde_json::json!({"action":"BypassCertificateError","payload":{"tab_id":tab_id,"host":host,"url":url}}).to_string();
    let argument = serde_json::to_string(&message)
        .unwrap()
        .replace('<', "\\u003c");
    let controls = format!(
        r#"<a href="evergreen://newtab">Về trang chủ</a><details style="margin-top:20px"><summary>Chi tiết nâng cao</summary><p>Chứng chỉ của {} không hợp lệ. Tiếp tục có thể làm lộ dữ liệu của bạn.</p><p style="overflow-wrap:anywhere">{}</p><button id="proceed" style="margin-top:12px;padding:8px;border:1px solid #dc7f52;border-radius:8px;background:white;color:#a33613">Vẫn tiếp tục (không an toàn)</button></details><script>document.getElementById('proceed').onclick=()=>window.ipc.postMessage({argument});</script>"#,
        escape_html(host),
        escape_html(url)
    );
    get_not_found_html()
        .replace(MASCOT_404_BASE64.trim(),include_str!("../ui/assets/mascot-thinking.webp.b64").trim())
        .replace("Không tìm thấy trang — Pip","Kết nối chưa an toàn — Pip")
        .replace("Pip · 404","Pip · Bảo mật")
        .replace("Ối! Trang này không tồn tại.","Kết nối này chưa an toàn.")
        .replace("Có thể liên kết đã thay đổi hoặc trang không còn nữa.","Pip đã dừng kết nối vì không xác minh được chứng chỉ bảo mật của trang. Bạn nên quay về trang chủ.")
        .replace("<a href=\"evergreen://newtab\">Quay về trang chủ</a>",&controls)
        .replace("Mascot Pip cầm bảng 404","Mascot Pip đang suy nghĩ")
}
pub fn get_crash_html(tab_id: u64) -> String {
    get_not_found_html()
        .replace("Không tìm thấy trang — Pip","Trang tạm ngừng phản hồi — Pip")
        .replace("Pip · 404","Pip · Khôi phục trang")
        .replace("Ối! Trang này không tồn tại.","Trang tạm ngừng phản hồi.")
        .replace("Có thể liên kết đã thay đổi hoặc trang không còn nữa.","Bạn có thể tải lại trang này. Các tab khác vẫn được giữ.")
        .replace("<a href=\"evergreen://newtab\">Quay về trang chủ</a>",&format!(r##"<a href="#" onclick="event.preventDefault();window.ipc.postMessage(JSON.stringify({{action:'ReloadTab',payload:{{tab_id:{tab_id}}}}}))">Tải lại trang</a>"##))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn certificate_details_cannot_inject_internal_markup() {
        let hostile = "https://bad.example/'\"</script><img src=x onerror=alert(1)>";
        let html = get_certificate_html("bad.example", hostile, 4);
        assert!(!html.contains("<img src=x"));
        assert!(html.contains("\\u003c/script>"));
        assert!(html.contains("&lt;/script&gt;"));
    }

    #[test]
    fn not_found_page_embeds_its_mascot_and_returns_home() {
        let html = get_not_found_html();
        assert!(html.contains("Ối! Trang này không tồn tại."));
        assert!(html.contains("data:image/webp;base64,"));
        assert!(html.contains("href=\"evergreen://newtab\""));
    }
}
