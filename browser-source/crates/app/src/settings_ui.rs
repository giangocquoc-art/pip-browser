//! Internal Settings Tab for Pip
//! Styled in Direction 01 — Fluent Desktop (WinUI 3 / Edge Dev minimal).

pub const LOGO_BASE64: &str = include_str!("../ui/logo.b64");
pub const MASCOT_BASE64: &str = include_str!("../ui/assets/mascot-clean.webp.b64");

pub const SETTINGS_TEMPLATE: &str = r#"<!DOCTYPE html>
<html lang="vi">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>Cài đặt — Pip</title>
  <link rel="icon" type="image/svg+xml" href="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='%239595a6' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'%3E%3Ccircle cx='12' cy='12' r='3'/%3E%3Cpath d='M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z'/%3E%3C/svg%3E">
  <style>
    :root {
      color-scheme: dark;
      --bg-surface: #181820;
      --bg-card: rgba(255, 255, 255, 0.035);
      --bg-card-hover: rgba(255, 255, 255, 0.05);
      --border-card: rgba(255, 255, 255, 0.07);
      --border-separator: rgba(255, 255, 255, 0.05);
      --text-main: #f0f0f5;
      --text-muted: #9595a8;
      --accent-blue: #4e8cff;
      --accent-blue-hover: #3d7ae8;
      --accent-green: #34d399;
      --accent-danger: #ef4444;
      --font-family: "Segoe UI Variable Text", "Segoe UI", system-ui, -apple-system, sans-serif;
    }

    * {
      box-sizing: border-box;
      margin: 0;
      padding: 0;
    }

    body {
      background-color: var(--bg-surface);
      color: var(--text-main);
      font-family: var(--font-family);
      padding: 48px 32px 80px;
      line-height: 1.5;
      user-select: none;
    }

    .container {
      max-width: 760px;
      margin: 0 auto;
    }

    /* Header */
    .header {
      margin-bottom: 32px;
    }

    .page-title {
      font-size: 26px;
      font-weight: 600;
      letter-spacing: -0.4px;
    }

    /* Section & Cards */
    .section-title {
      font-size: 14px;
      font-weight: 600;
      color: var(--text-muted);
      margin: 28px 0 12px;
      text-transform: uppercase;
      letter-spacing: 0.5px;
    }

    .card {
      background: var(--bg-card);
      border: 1px solid var(--border-card);
      border-radius: 12px;
      overflow: hidden;
      margin-bottom: 20px;
      box-shadow: 0 4px 16px rgba(0, 0, 0, 0.2);
    }

    .row {
      display: flex;
      align-items: center;
      justify-content: space-between;
      padding: 18px 24px;
      border-bottom: 1px solid var(--border-separator);
    }
    .row[hidden] { display: none; }

    .row:last-child {
      border-bottom: none;
    }

    .site-permissions-list{display:grid;gap:8px;padding:10px 18px 16px}
    .site-permission-item{display:flex;align-items:center;justify-content:space-between;gap:12px;padding:10px 12px;border:1px solid var(--border-separator);border-radius:9px}
    .site-permission-detail{display:flex;min-width:0;flex-direction:column;gap:3px}
    .site-permission-detail strong{overflow-wrap:anywhere;font-size:13px;color:var(--text-main)}
    .site-permission-detail span,.empty-state{font-size:12px;color:var(--text-muted)}
    .empty-state{padding:4px 2px}
    .password-form{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:10px;padding:16px 20px;border-bottom:1px solid var(--border-separator)}
    .password-form .password-wide{grid-column:1/-1}
    .password-list{display:grid;gap:8px;padding:12px 18px 16px}
    .password-item{display:flex;align-items:center;justify-content:space-between;gap:12px;padding:12px;border:1px solid var(--border-separator);border-radius:9px}
    .password-detail{display:flex;min-width:0;flex-direction:column;gap:3px;overflow-wrap:anywhere}
    .password-detail strong{font-size:13px}.password-detail span{font-size:12px;color:var(--text-muted)}
    .password-actions{display:flex;flex-wrap:wrap;justify-content:flex-end;gap:6px}
    @media(max-width:640px){.password-form{grid-template-columns:1fr}.password-form .password-wide{grid-column:auto}.password-item{align-items:flex-start;flex-direction:column}.password-actions{justify-content:flex-start}}
    .extension-list{display:grid;gap:8px;padding:12px 18px 16px}
    .extension-item{display:flex;align-items:center;justify-content:space-between;gap:12px;padding:12px;border:1px solid var(--border-separator);border-radius:9px}
    .extension-detail{display:flex;min-width:0;flex-direction:column;gap:3px;overflow-wrap:anywhere}
    .extension-detail strong{font-size:13px}.extension-detail span{font-size:12px;color:var(--text-muted)}
    .extension-actions{display:flex;flex-wrap:wrap;gap:6px}
    @media(max-width:640px){.extension-item{align-items:flex-start;flex-direction:column}}

    .row-info {
      display: flex;
      flex-direction: column;
      gap: 3px;
    }

    .row-label {
      font-size: 14px;
      font-weight: 500;
      color: var(--text-main);
    }

    .row-desc {
      font-size: 12px;
      color: var(--text-muted);
    }

    .row-action {
      display: flex;
      align-items: center;
      gap: 12px;
    }

    .val-badge {
      font-size: 12px;
      color: var(--text-muted);
      background: rgba(255, 255, 255, 0.05);
      border: 1px solid var(--border-card);
      padding: 4px 10px;
      border-radius: 8px;
    }

    .val-badge.active {
      color: var(--accent-green);
      border-color: rgba(52, 211, 153, 0.3);
      background: rgba(52, 211, 153, 0.08);
      font-weight: 500;
    }

    /* Buttons */
    .btn {
      background: var(--accent-blue);
      color: #fff;
      border: none;
      border-radius: 8px;
      padding: 7px 16px;
      font-size: 13px;
      font-family: inherit;
      font-weight: 500;
      cursor: pointer;
      display: inline-flex;
      align-items: center;
      gap: 6px;

    }

    .btn:hover {
      background: var(--accent-blue-hover);
    }
    .btn[hidden] { display:none!important; }
    #savePasswordButton,#addBrowserExtensionButton{color:#111827}

    .btn-secondary {
      background: rgba(255, 255, 255, 0.08);
      color: var(--text-main);
      border: 1px solid var(--border-card);
    }

    .btn-secondary:hover {
      background: rgba(255, 255, 255, 0.12);
    }

    /* Status Banner */
    .status-banner {
      display: none;
      padding: 12px 18px;
      border-radius: 8px;
      font-size: 13px;
      margin-top: 16px;
      align-items: center;
      gap: 8px;
    }

    .status-banner.show {
      display: flex;
    }

    .status-banner.info {
      background: rgba(78, 140, 255, 0.12);
      border: 1px solid rgba(78, 140, 255, 0.3);
      color: #93c5fd;
    }

    .select-box {
      background: rgba(255, 255, 255, 0.08);
      border: 1px solid var(--border);
      color: var(--text-main);
      padding: 6px 14px;
      border-radius: 8px;
      font-size: 13px;
      outline: none;
      cursor: pointer;
      font-family: inherit;
    }

    .select-box option {
      background: #1e1e28;
      color: var(--text-main);
    }

    /* Toggle Switch */
    .switch {
      position: relative;
      display: inline-block;
      width: 40px;
      height: 22px;
    }

    .switch input {
      opacity: 0;
      width: 0;
      height: 0;
    }

    .slider {
      position: absolute;
      cursor: pointer;
      top: 0;
      left: 0;
      right: 0;
      bottom: 0;
      background-color: rgba(255, 255, 255, 0.1);

      border-radius: 22px;
      border: 1px solid var(--border-card);
    }

    .slider:before {
      position: absolute;
      content: "";
      height: 14px;
      width: 14px;
      left: 3px;
      bottom: 3px;
      background-color: #fff;

      border-radius: 50%;
    }

    input:checked + .slider {
      background-color: var(--accent-blue);
      border-color: var(--accent-blue);
    }

    input:checked + .slider:before {
      transform: translateX(18px);
    }

    /* Persistent Site Whitelist */
    .whitelist-container {
      display: flex;
      flex-direction: column;
      gap: 12px;
      padding: 6px 20px 18px 20px;
    }

    .whitelist-input-row {
      display: flex;
      gap: 10px;
      align-items: center;
    }

    .text-input {
      flex: 1;
      background: rgba(255, 255, 255, 0.06);
      border: 1px solid var(--border-card);
      color: var(--text-main);
      padding: 8px 14px;
      border-radius: 8px;
      font-size: 13px;
      outline: none;
      font-family: inherit;

    }

    .text-input:focus {
      border-color: var(--accent-blue);
      background: rgba(255, 255, 255, 0.09);
    }

    .text-input::placeholder {
      color: var(--text-muted);
    }

    .chips-container {
      display: flex;
      flex-wrap: wrap;
      gap: 8px;
      margin-top: 4px;
    }

    .domain-chip {
      display: inline-flex;
      align-items: center;
      gap: 8px;
      background: rgba(52, 211, 153, 0.12);
      border: 1px solid rgba(52, 211, 153, 0.28);
      color: #34d399;
      font-size: 12.5px;
      font-weight: 500;
      padding: 4px 10px;
      border-radius: 6px;
    }

    .chip-remove {
      display: inline-flex;
      align-items: center;
      justify-content: center;
      width: 15px;
      height: 15px;
      border-radius: 50%;
      cursor: pointer;
      color: #94a3b8;

      font-size: 15px;
      line-height: 1;
    }

    .chip-remove:hover {
      color: #ef4444;
      background: rgba(239, 68, 68, 0.2);
    }

    .chips-empty-hint {
      font-size: 12.5px;
      color: var(--text-muted);
      font-style: italic;
    }

    .switch input{position:absolute;inset:0;width:100%;height:100%;margin:0;opacity:0;z-index:1;cursor:pointer}.switch:focus-within{outline:2px solid #f97316;outline-offset:3px}
    *,*::before,*::after { animation:none!important; transition:none!important; scroll-behavior:auto!important; backdrop-filter:none!important; }
    :root{color-scheme:light;--bg-surface:#fffaf6;--bg-card:#fff;--bg-card-hover:#fff4ec;--border-card:#f1e2d5;--border-separator:#f3e8df;--text-main:#17202e;--text-muted:#596779;--accent-blue:#f97316;--accent-blue-hover:#e85d0d;--accent-green:#15803d;--accent-danger:#dc2626;--border-focus:#f97316}
    body{display:grid;grid-template-columns:190px minmax(0,900px);justify-content:center;align-items:start;gap:28px;padding:28px 24px 54px;background:var(--bg-surface)}
    .container{width:100%;max-width:900px;margin:0}.header{padding:22px 25px;margin-bottom:22px;border:1px solid var(--border-card);border-radius:16px;background:linear-gradient(105deg,#fff 0%,#fff 62%,#fff0e5 100%);position:relative;overflow:hidden;min-height:130px}
    .header::after{content:"";position:absolute;right:10px;bottom:-35px;width:155px;height:155px;background:url("data:image/webp;base64,{{MASCOT_BASE64}}") center/contain no-repeat;pointer-events:none}
    .page-title{font-size:28px}.section-title{text-transform:none;letter-spacing:0;color:#344054;scroll-margin-top:18px}.card{box-shadow:0 3px 12px rgba(98,57,28,.035)}.row{padding:15px 19px}.row-desc{color:#596779}.val-badge.active{color:#166534;border-color:#bbf7d0;background:#f0fdf4}.status-banner.info{background:#fff4ec;border:1px solid #f1d1b9;color:#8a3d0c}.select-box,.text-input{background:#fff;border:1px solid #e4d5c9;color:#17202e}.select-box option{background:#fff;color:#17202e}.text-input:focus,.select-box:focus{border-color:#f97316;background:#fff}.domain-chip{background:#f0fdf4;border-color:#bbf7d0;color:#166534}
    .settings-nav{position:sticky;top:28px;align-self:start;max-height:calc(100vh - 56px);overflow:auto;padding:8px;border:1px solid #f1e2d5;border-radius:13px;background:#fff;box-shadow:0 3px 12px rgba(98,57,28,.035)}.settings-nav-title{padding:8px 9px 10px;color:#596779;font-size:11px;font-weight:700;letter-spacing:.5px;text-transform:uppercase}.settings-nav button{display:block;width:100%;padding:8px 10px;border:0;border-radius:8px;background:transparent;color:#344054;text-align:left;font:500 12px/1.35 "Segoe UI",system-ui,sans-serif;cursor:pointer}.settings-nav button:hover{background:#fff4ec;color:#b84b08}.settings-nav button[aria-current="location"]{background:#fff0e5;color:#b84b08;font-weight:700}
    input[type=checkbox]{accent-color:#f97316}.switch input:checked + .slider{background:#f97316;border-color:#f97316}.switch .slider{background:#f1e8e1;border-color:#dfd1c6}.switch .slider:before{background:#fff;box-shadow:0 1px 3px rgba(35,24,16,.2)}
    @media(max-width:1050px){body{grid-template-columns:160px minmax(0,1fr);gap:18px;padding-inline:16px}.settings-nav{padding:6px}.settings-nav button{padding:8px}}
    @media(max-width:760px){body{display:block;padding:12px 12px 35px}.settings-nav{position:sticky;top:0;z-index:5;display:flex;align-items:center;gap:4px;max-height:none;overflow-x:auto;margin:0 -12px 14px;padding:6px 12px;border-radius:0 0 12px 12px}.settings-nav-title{position:sticky;left:0;flex:none;padding:7px;color:#596779;background:#fff}.settings-nav button{width:auto;flex:none;white-space:nowrap}.header{padding:18px;min-height:116px}.header::after{right:0;width:108px;height:108px}.row{padding:13px;gap:12px}.row-info{min-width:0}}
    @media(prefers-color-scheme:dark){:root{color-scheme:dark;--bg-surface:#171719;--bg-card:#222327;--bg-card-hover:#2b2c31;--border-card:#38393f;--border-separator:#33343a;--text-main:#f5f2ef;--text-muted:#c1b8b0;--accent-blue:#fb923c;--accent-blue-hover:#fdba74;--accent-green:#4ade80;--accent-danger:#f87171}.header{background:linear-gradient(105deg,#222327,#29231f);border-color:#493629}.header::after{opacity:.86}.section-title{color:#e8ddd4}.row-desc,.settings-nav-title{color:#c1b8b0}.card,.settings-nav{background:#222327;border-color:#38393f}.val-badge,.select-box,.text-input{background:#292a2f;color:#f5f2ef;border-color:#45464d}.val-badge.active{background:#183523;color:#86efac;border-color:#275a38}.select-box option{background:#222327;color:#f5f2ef}.settings-nav button{color:#e8ddd4}.settings-nav button:hover,.settings-nav button[aria-current="location"]{background:#3a291f;color:#fdba74}.status-banner.info{background:#37291f;border-color:#6a4229;color:#ffd1ad}.domain-chip{background:#183523;border-color:#275a38;color:#86efac}.switch .slider{background:#3a3b42;border-color:#55565e}.settings-nav-title{background:#222327}}
    .help-card{padding:0 19px}.help-card details+details{border-top:1px solid var(--border-separator)}.help-card summary{padding:15px 0;font-weight:600;font-size:14px;cursor:pointer}.help-card p{font-size:13px;color:var(--text-muted);line-height:1.65;padding:0 0 17px;user-select:text}.help-card b{color:var(--text-main)}button:focus-visible,summary:focus-visible,select:focus-visible{outline:2px solid var(--accent-blue);outline-offset:3px}.row{gap:16px;flex-wrap:wrap}.row-info{flex:1 1 230px}.row-action{flex:0 1 auto;max-width:100%}input,select{max-width:100%}.val-badge{white-space:normal}.row-desc{user-select:text}.status-banner{position:sticky;bottom:12px;z-index:6}
</style>
</head>
<body>
  <nav class="settings-nav" id="settingsNav" aria-label="Danh mục cài đặt"><div class="settings-nav-title">Danh mục</div></nav>
  <div class="container">
    <div class="header">
      <h1 class="page-title">Cài đặt</h1><p class="row-desc" style="max-width:70%;margin-top:8px">Lựa chọn được lưu trên máy. Đọc mô tả dưới mỗi chức năng để biết tác dụng và cách dùng.</p>
    </div>

    <div class="section-title">Hướng dẫn Pip</div>
    <div class="card help-card">
      <details><summary>Thanh công cụ: hồ sơ, dấu trang, khiên và G DNS</summary><p><b>Hồ sơ</b> mở các cửa sổ có đăng nhập, lịch sử và dấu trang riêng. <b>Ngôi sao</b> hoặc Ctrl+D lưu trang đang xem. <b>Khiên</b> đổi chặn quảng cáo cho hostname hiện tại; nhấn F5 sau khi đổi. <b>G DNS</b> đổi cách tra cứu địa chỉ website qua Google; mở lại Pip để áp dụng.</p></details>
      <details><summary>Chặn quảng cáo hoạt động thế nào?</summary><p>Brave Adblock Rust lọc yêu cầu mạng bằng bộ lọc tích hợp, kèm quy tắc ẩn quảng cáo và scriptlet cho YouTube. Nếu trang lỗi, thử tắt khiên cho trang đó rồi tải lại. Quảng cáo YouTube có thể thay đổi và vẫn xuất hiện; bạn có thể cập nhật danh sách trong Cài đặt.</p></details>
      <details><summary>Hồ sơ và ẩn danh khác nhau thế nào?</summary><p>Hồ sơ giữ đăng nhập và dữ liệu website riêng trên thiết bị. Pip Sync chỉ đồng bộ dấu trang đã mã hóa sau khi bạn tạo mã và bấm đồng bộ. Ẩn danh không ghi lịch sử. Đóng toàn bộ cửa sổ ẩn danh của hồ sơ để kết thúc phiên; tệp tải và dấu trang chủ động lưu vẫn còn. Website và nhà mạng vẫn có thể nhận biết kết nối.</p></details>
      <details><summary>Các phím tắt thường dùng</summary><p>Ctrl+T: tab mới · Ctrl+Tab hoặc Ctrl+PageDown: tab kế · Ctrl+Shift+Tab hoặc Ctrl+PageUp: tab trước · Ctrl+W: đóng tab · Ctrl+Shift+W: đóng cửa sổ · Ctrl+Shift+T: mở lại tab đã đóng · Ctrl+L: thanh địa chỉ · Ctrl+H: lịch sử · Ctrl+Shift+O: dấu trang · Ctrl+J: tải xuống · Ctrl+Shift+Delete: xóa dữ liệu duyệt web · Ctrl+F: tìm chữ · Ctrl+G / Ctrl+Shift+G hoặc F3 / Shift+F3: kết quả kế / trước · Ctrl+Shift+M: chuyển hồ sơ · Ctrl+Shift+N: ẩn danh · Alt+Shift+P: điền mật khẩu duy nhất đã lưu cho website hiện tại. Xem danh sách đầy đủ trong Menu → Phím tắt.</p></details>
    </div>
    <div class="section-title">Chặn quảng cáo</div>
    <div class="card">
      <div class="row">
        <div class="row-info"><span class="row-label">Mức chặn</span><span class="row-desc">Cân bằng giữ cách lọc hiện tại. Mạnh chặn thêm tên miền cá cược đã nhận diện khi được nhúng trong website. Ưu tiên video bỏ qua luật chặn với luồng media và phân đoạn video phổ biến.</span></div>
        <select id="adblockMode" aria-label="Mức chặn quảng cáo" onchange="onAdblockModeChange(this.value)"><option value="balanced">Cân bằng (khuyên dùng)</option><option value="strong">Mạnh hơn với nội dung cá cược</option><option value="video_first">Ưu tiên video</option></select>
      </div>
      <div class="row">
        <div class="row-info">
          <span class="row-label">Cập nhật danh sách bộ lọc</span>
          <span class="row-desc" id="adblockUpdateStatus" aria-live="polite">Tải EasyList và uBlock Origin qua HTTPS. Danh sách chỉ chứa luật lọc mạng; khởi động lại Pip để áp dụng.</span>
        </div>
        <button class="btn btn-secondary" id="updateAdblockFiltersButton" onclick="updateAdblockFilters()">Cập nhật bộ lọc</button>
      </div>
    </div>
    <div class="section-title">Bộ nhớ và dữ liệu</div>
    <div class="card help-card">
      <details><summary>Mở nhiều tab mà nhẹ hơn</summary><p>Tab được khôi phục từ phiên trước chỉ tải khi bạn chọn. Tab nền được WebView2 yêu cầu tạm dừng. Nếu Windows còn dưới 512 MB RAM trống, tab nền đã ngủ ít nhất 30 phút có thể được nhả renderer để giải phóng thêm RAM. Pip giữ lại tab có thao tác người dùng, biểu mẫu chưa gửi, âm thanh/video, quyền media, lượt tải, tab ghim hoặc tab được chọn Giữ thức. Khi chọn tab đã nhả, Pip tải lại địa chỉ; một số trạng thái riêng mà website chỉ giữ trong mã trang vẫn có thể mất.</p></details>
      <details><summary>RAM, dung lượng đĩa và Internet</summary><p>RAM là bộ nhớ của các trang đang mở. Dung lượng đĩa gồm bộ nhớ đệm, cookie, dữ liệu website và tệp đã tải, có thể tăng theo cách dùng. Mỗi hồ sơ lưu một bộ dữ liệu riêng; chỉ tạo khi cần tách tài khoản. Pip dùng chung WebView2 của Microsoft. Không có trình duyệt nhiều tab nào sử dụng bằng không các tài nguyên này.</p></details>
      <div class="row"><div class="row-info"><span class="row-label">Tự ngủ tab nền để giảm hoạt động</span><span class="row-desc">WebView2 tạm dừng đồng hồ và hoạt ảnh của tab không hoạt động, đồng thời được yêu cầu tiết kiệm bộ nhớ; mức RAM thu hồi tùy trang và WebView2. Khi RAM trống thấp, tab ngủ lâu và không phát hiện dữ liệu đang sửa có thể được nhả renderer; chọn lại sẽ tải lại trang. Tab có thao tác, biểu mẫu chưa gửi, âm thanh/video, camera/micrô, đang tải, tab ghim hoặc được chọn Giữ thức sẽ không tự nhả.</span></div><div class="row-action"><label><input type="checkbox" id="backgroundThrottlingToggle" {{BACKGROUND_TAB_SLEEP_ENABLED}} onchange="onToggleChange('backgroundThrottling',this.checked)"> Bật</label><select id="sleepAfterSelect" {{SLEEP_DELAY_DISABLED}} aria-label="Thời gian trước khi tab nền ngủ" onchange="onIdleSleepChange(this.value)"><option value="0">Không tự ngủ</option><option value="60">Sau 1 phút</option><option value="300">Sau 5 phút</option><option value="900">Sau 15 phút</option><option value="1800">Sau 30 phút</option></select></div></div>
      <div class="row"><div class="row-info"><span class="row-label">Dọn bộ nhớ đệm website</span><span class="row-desc">Xóa bản sao tạm của ảnh và tệp website thuộc hồ sơ hiện tại để thu hồi dung lượng đĩa. Giữ cookie đăng nhập, lịch sử, dấu trang và tệp tải. Website có thể tải chậm hơn trong lần mở kế tiếp.</span></div><button class="btn btn-secondary" id="clearCacheButton" onclick="sendAction('ClearCache')">Xóa bộ nhớ đệm</button></div>
      <div class="row"><div class="row-info"><span class="row-label">Xóa dữ liệu duyệt web</span><span class="row-desc">Xóa các loại đã chọn trong toàn bộ thời gian của hồ sơ hiện tại. Lịch sử gồm Pip và WebView2; xóa lịch sử tải xuống không xóa tệp hoặc hủy lượt tải đang chạy. Dấu trang luôn được giữ lại.</span></div><div class="row-action" role="group" aria-label="Loại dữ liệu cần xóa"><label><input id="clearDataHistory" type="checkbox"> Lịch sử</label><label><input id="clearDataSite" type="checkbox"> Cookie và dữ liệu website</label><label><input id="clearDataCache" type="checkbox"> Bộ nhớ đệm</label><label><input id="clearDataPasswords" type="checkbox"> Mật khẩu đã lưu</label><label><input id="clearDataAutofill" type="checkbox"> Dữ liệu tự điền</label><label><input id="clearDataDownloads" type="checkbox"> Lịch sử tải xuống</label></div><button class="btn btn-secondary" id="clearSelectedBrowsingDataButton" onclick="clearSelectedBrowsingData()">Xóa dữ liệu đã chọn</button></div>
    </div>

    <div class="section-title">Bộ máy & phiên bản</div>
    <div class="card">
      <div class="row">
        <div class="row-info">
          <span class="row-label">Bộ máy hiển thị</span>
          <span class="row-desc">Microsoft WebView2</span>
        </div>
        <div class="row-action">
          <span class="val-badge" id="engineVersion">Đang kiểm tra...</span>
        </div>
      </div>
      <div class="row">
        <div class="row-info">
          <span class="row-label">Cập nhật bộ máy</span>
          <span class="row-desc">Thành phần của Microsoft dùng chung để hiển thị website; được dịch vụ Microsoft cập nhật riêng với Pip</span>
        </div>
        <div class="row-action">
          <span class="val-badge active">Microsoft quản lý</span>
        </div>
      </div>
      <div class="row">
        <div class="row-info">
          <span class="row-label">Cập nhật Pip</span>
          <span class="row-desc" id="appUpdateStatus" aria-live="polite">Chỉ kết nối GitHub khi bạn bấm kiểm tra.</span>
        </div>
        <div class="row-action">
          <button class="btn btn-secondary" id="checkAppUpdatesButton" onclick="checkAppUpdates()">Kiểm tra phiên bản</button>
          <a class="btn" id="openAppReleaseButton" hidden target="_blank" rel="noopener noreferrer">Mở bản phát hành</a>
        </div>
      </div>
    </div>

    <div class="section-title">Tính năng trình duyệt</div>
    <div class="card">
      <div class="row">
        <div class="row-info">
          <span class="row-label">Tìm trong trang</span>
          <span class="row-desc">Ctrl+F tìm chữ; Enter / Shift+Enter hoặc Ctrl+G / Ctrl+Shift+G (F3 / Shift+F3) chuyển kết quả</span>
        </div>
        <div class="row-action">
          <label class="switch">
            <input type="checkbox" id="featureFindInPageToggle" aria-label="Tìm trong trang" checked onchange="onFeatureToggleChange('enable_find_in_page', this.checked)">
            <span class="slider"></span>
          </label>
        </div>
      </div>
      <div class="row">
        <div class="row-info">
          <span class="row-label">Quản lý tải xuống</span>
          <span class="row-desc">Ctrl+J mở lịch sử tải của hồ sơ, hủy lượt đang chạy, mở tệp hoặc thư mục</span>
        </div>
        <div class="row-action">
          <label class="switch">
            <input type="checkbox" id="featureDownloadsToggle" aria-label="Quản lý tải xuống" checked onchange="onFeatureToggleChange('enable_downloads_manager', this.checked)">
            <span class="slider"></span>
          </label>
        </div>
      </div>
      <div class="row">
        <div class="row-info">
          <span class="row-label">Xem trước địa chỉ liên kết</span>
          <span class="row-desc">Hiện địa chỉ đích ở góc dưới khi rê chuột lên liên kết, giúp kiểm tra trước khi bấm</span>
        </div>
        <div class="row-action">
          <label class="switch">
            <input type="checkbox" id="featureLinkPreviewToggle" aria-label="Xem trước liên kết" checked onchange="onFeatureToggleChange('enable_link_preview', this.checked)">
            <span class="slider"></span>
          </label>
        </div>
      </div>
      <div class="row">
        <div class="row-info">
          <span class="row-label">Thao tác và sắp xếp thẻ</span>
          <span class="row-desc">Bật nhóm thao tác kéo tab; hai lựa chọn trong mục Thẻ và cửa sổ điều khiển từng thao tác</span>
        </div>
        <div class="row-action">
          <label class="switch">
            <input type="checkbox" id="featureTabGesturesToggle" aria-label="Sắp xếp và tách thẻ" checked onchange="onFeatureToggleChange('enable_tab_gestures', this.checked)">
            <span class="slider"></span>
          </label>
        </div>
      </div>
      <div class="row">
        <div class="row-info">
          <span class="row-label">Điều khiển thu phóng</span>
          <span class="row-desc">Ctrl++ phóng to, Ctrl+- thu nhỏ, Ctrl+0 về 100%; tỷ lệ hiển thị cạnh thanh địa chỉ</span>
        </div>
        <div class="row-action">
          <label class="switch">
            <input type="checkbox" id="featureZoomControlsToggle" aria-label="Điều khiển thu phóng" checked onchange="onFeatureToggleChange('enable_zoom_controls', this.checked)">
            <span class="slider"></span>
          </label>
        </div>
      </div>
      <div class="row">
        <div class="row-info">
          <span class="row-label">Yêu cầu quyền truy cập</span>
          <span class="row-desc">Hiện thanh Cho phép / Chặn khi website xin quyền. Tắt sẽ chặn mọi yêu cầu quyền, kể cả quyền đang chọn Cho phép</span>
        </div>
        <div class="row-action">
          <label class="switch">
            <input type="checkbox" id="featurePermissionsToggle" aria-label="Hỏi quyền truy cập" checked onchange="onFeatureToggleChange('enable_permissions_prompt', this.checked)">
            <span class="slider"></span>
          </label>
        </div>
      </div>
    </div>

    <div class="section-title">Tìm kiếm và điều hướng</div>
    <div class="card">
      <div class="row">
        <div class="row-info">
          <span class="row-label">Công cụ tìm kiếm mặc định</span>
          <span class="row-desc">Dùng khi tìm kiếm trên thanh địa chỉ hoặc trang tab mới</span>
        </div>
        <div class="row-action">
          <select id="searchEngineSelect" class="select-box" aria-label="Công cụ tìm kiếm mặc định" onchange="onSearchEngineChange(this.value)">
            <option id="opt_google" value="google">Google</option>
            <option id="opt_bing" value="bing">Bing</option>
            <option id="opt_brave" value="brave">Brave Search</option>
            <option id="opt_ecosia" value="ecosia">Ecosia</option>
          </select>
        </div>
      </div>
    </div>

    <div class="section-title">Tải xuống</div>
    <div class="card">
      <div class="row">
        <div class="row-info">
          <span class="row-label">Hỏi nơi lưu tệp</span>
          <span class="row-desc">Chọn tên và nơi lưu trước khi tải. Tắt để lưu vào thư mục mặc định ở bên dưới</span>
        </div>
        <div class="row-action">
          <label class="switch">
            <input type="checkbox" id="askWhereToSaveToggle" aria-label="Hỏi nơi lưu tệp" checked onchange="onToggleChange('askWhereToSave', this.checked)">
            <span class="slider"></span>
          </label>
        </div>
      </div>
      <div class="row">
        <div class="row-info">
          <span class="row-label">Hiện tiến trình tải xuống</span>
          <span class="row-desc">Hiện tiến trình tải xuống trên thanh công cụ</span>
        </div>
        <div class="row-action">
          <label class="switch">
            <input type="checkbox" id="showProgressToolbarToggle" aria-label="Hiện tiến trình tải xuống" checked onchange="onToggleChange('showProgressToolbar', this.checked)">
            <span class="slider"></span>
          </label>
        </div>
      </div>
      <div class="row">
        <div class="row-info">
          <span class="row-label">Thư mục lưu mặc định</span>
          <span class="row-desc" id="downloadFolderDesc">{{DEFAULT_DOWNLOAD_FOLDER}}</span>
        </div>
        <div class="row-action">
          <button class="btn btn-secondary" onclick="sendAction('ChangeDownloadFolder')">Chọn thư mục...</button>
        </div>
      </div>
    </div>

    <div class="section-title">Thẻ và cửa sổ</div>
    <div class="card">
      <div class="row">
        <div class="row-info"><span class="row-label">Cảnh báo khi đóng nhiều tab</span><span class="row-desc">Hỏi trước khi đóng cửa sổ đang có từ hai tab trở lên</span></div>
        <div class="row-action"><label class="switch"><input type="checkbox" id="warnOnCloseTabsToggle" aria-label="Cảnh báo khi đóng nhiều tab" onchange="onToggleChange('warnOnCloseTabs', this.checked)"><span class="slider"></span></label></div>
      </div>
      <div class="row">
        <div class="row-info">
          <span class="row-label">Kéo để sắp xếp thẻ</span>
          <span class="row-desc">Kéo và thả để đổi vị trí các thẻ đang mở</span>
        </div>
        <div class="row-action">
          <label class="switch">
            <input type="checkbox" id="tabReorderingToggle" aria-label="Kéo để sắp xếp thẻ" checked onchange="onToggleChange('tabReordering', this.checked)">
            <span class="slider"></span>
          </label>
        </div>
      </div>
      <div class="row">
        <div class="row-info">
          <span class="row-label">Tách thẻ sang cửa sổ mới</span>
          <span class="row-desc">Kéo tab ra ngoài để mở cửa sổ riêng. Tab sẽ tải lại; Pip hỏi trước vì nội dung chưa gửi có thể mất</span>
        </div>
        <div class="row-action">
          <label class="switch">
            <input type="checkbox" id="tabTearoffToggle" aria-label="Tách thẻ sang cửa sổ mới" checked onchange="onToggleChange('tabTearoff', this.checked)">
            <span class="slider"></span>
          </label>
        </div>
      </div>
    </div>

    <div class="section-title">Giao diện và thu phóng</div>
    <div class="card">
      <div class="row"><div class="row-info"><span class="row-label">Chủ đề Pip</span><span class="row-desc">Áp dụng cho thanh tab và thanh điều hướng của hồ sơ này</span></div><select id="themeSelect" class="select-box" aria-label="Chủ đề Pip" onchange="onAppearanceSelectChange('theme',this.value)"><option value="system">Theo hệ thống</option><option value="light">Sáng</option><option value="dark">Tối</option></select></div>
      <div class="row"><div class="row-info"><span class="row-label">Mật độ giao diện</span><span class="row-desc">Gọn giảm khoảng đệm trong thanh công cụ</span></div><select id="densitySelect" class="select-box" aria-label="Mật độ giao diện" onchange="onAppearanceSelectChange('density',this.value)"><option value="comfortable">Thoải mái</option><option value="compact">Gọn</option></select></div>
      <div class="row">
        <div class="row-info">
          <span class="row-label">Tỷ lệ trang mặc định</span>
          <span class="row-desc">Chọn kích thước nội dung phù hợp với bạn</span>
        </div>
        <div class="row-action">
          <select id="defaultZoomSelect" class="select-box" aria-label="Tỷ lệ trang mặc định" onchange="onDefaultZoomChange(this.value)">
            <option id="opt_zoom_75" value="0.75">75%</option>
            <option id="opt_zoom_90" value="0.9">90%</option>
            <option id="opt_zoom_100" value="1.0">100% (Mặc định)</option>
            <option id="opt_zoom_110" value="1.1">110%</option>
            <option id="opt_zoom_125" value="1.25">125%</option>
            <option id="opt_zoom_150" value="1.5">150%</option>
          </select>
        </div>
      </div>
      <div class="row">
        <div class="row-info">
          <span class="row-label">Hiện tỷ lệ thu phóng</span>
          <span class="row-desc">Hiện tỷ lệ cạnh thanh địa chỉ khi bạn phóng to hoặc thu nhỏ</span>
        </div>
        <div class="row-action">
          <label class="switch">
            <input type="checkbox" id="showZoomBadgeToggle" aria-label="Hiện tỷ lệ trang" checked onchange="onToggleChange('showZoomBadge', this.checked)">
            <span class="slider"></span>
          </label>
        </div>
      </div>
      </div>
      </div>
    </div>

    <div class="section-title">Quyền truy cập website</div>
    <div class="card">
      <div class="row">
        <div class="row-info">
          <span class="row-label">Vị trí</span>
          <span class="row-desc">Luôn hỏi để quyết định cho từng yêu cầu. Cho phép áp dụng cho mọi website; Chặn từ chối tất cả</span>
        </div>
        <div class="row-action">
          <select id="permLocationSelect" class="select-box" aria-label="Quyền truy cập vị trí" onchange="onPermissionChange('location', this.value)">
            <option id="opt_perm_location_ask" value="ask">Luôn hỏi</option>
            <option id="opt_perm_location_allow" value="allow">Cho phép</option>
            <option id="opt_perm_location_block" value="block">Chặn</option>
          </select>
        </div>
      </div>
      <div class="row">
        <div class="row-info">
          <span class="row-label">Camera</span>
          <span class="row-desc">Website cần camera để gọi video. Chọn Luôn hỏi để quyết định trước mỗi yêu cầu</span>
        </div>
        <div class="row-action">
          <select id="permCameraSelect" class="select-box" aria-label="Quyền sử dụng camera" onchange="onPermissionChange('camera', this.value)">
            <option id="opt_perm_camera_ask" value="ask">Luôn hỏi</option>
            <option id="opt_perm_camera_allow" value="allow">Cho phép</option>
            <option id="opt_perm_camera_block" value="block">Chặn</option>
          </select>
        </div>
      </div>
      <div class="row">
        <div class="row-info">
          <span class="row-label">Micrô</span>
          <span class="row-desc">Website cần micrô để thu âm hoặc gọi điện. Chọn Luôn hỏi để quyết định trước mỗi yêu cầu</span>
        </div>
        <div class="row-action">
          <select id="permMicrophoneSelect" class="select-box" aria-label="Quyền sử dụng micrô" onchange="onPermissionChange('microphone', this.value)">
            <option id="opt_perm_microphone_ask" value="ask">Luôn hỏi</option>
            <option id="opt_perm_microphone_allow" value="allow">Cho phép</option>
            <option id="opt_perm_microphone_block" value="block">Chặn</option>
          </select>
        </div>
      </div>
      <div class="row">
        <div class="row-info">
          <span class="row-label">Thông báo</span>
          <span class="row-desc">Cho phép website xin gửi thông báo; không phải tùy chọn thông báo của Windows</span>
        </div>
        <div class="row-action">
          <select id="permNotificationsSelect" class="select-box" aria-label="Quyền gửi thông báo" onchange="onPermissionChange('notifications', this.value)">
            <option id="opt_perm_notifications_ask" value="ask">Luôn hỏi</option>
            <option id="opt_perm_notifications_allow" value="allow">Cho phép</option>
            <option id="opt_perm_notifications_block" value="block">Chặn</option>
          </select>
        </div>
      </div>
      <div class="row"><div class="row-info"><span class="row-label">Quyền đã cấp cho website</span><span class="row-desc" id="sitePermissionsHint">Danh sách quyền riêng theo hồ sơ. Đặt lại để website hỏi lại lần sau.</span></div><button class="btn btn-secondary" id="refreshSitePermissionsButton" onclick="profileAction('GetSitePermissions')">Tải lại</button></div>
      <div id="sitePermissionsList" class="site-permissions-list" role="list" aria-live="polite"><span class="empty-state" role="listitem">Đang tải quyền đã lưu…</span></div>
    </div>

    <div class="section-title">Kết nối</div>
    <div class="card">
      <div class="row"><div class="row-info"><span class="row-label">Google DNS bảo mật</span><span class="row-desc" id="googleDnsHint">Khi bật, Pip cấu hình WebView2 và tác vụ HTTPS nền dùng proxy DNS qua HTTPS của Google. Tra cứu trước từ WebView2 bị chặn cục bộ, ngoại trừ localhost và 127.0.0.1 dùng cho proxy. Nếu cả truy vấn A và AAAA đều gặp lỗi tạm thời, Pip dùng DNS Windows dự phòng và mạng đó có thể thấy tên miền; phản hồi NXDOMAIN/NODATA hoặc không hợp lệ sẽ không fallback. Nút “Kiểm tra từ WebView2” gửi GET tới YouTube bằng no-cors, không gửi cookie hoặc referrer, qua tuyến mạng hiện hành của chính WebView2 (gồm proxy cục bộ Pip nếu Google DNS đang bật). Kết quả chỉ cho biết yêu cầu hoàn tất, lỗi mạng hoặc timeout; no-cors ẩn mã HTTP và kiểm tra này không bắt gói để xác minh từng truy vấn DNS. Cập nhật WebView2 bằng PowerShell vẫn dùng DNS Windows. Sau khi đổi, hãy khởi động lại để áp dụng.</span></div><input id="googleDnsToggle" type="checkbox" aria-label="Dùng Google DNS" onchange="profileAction('SetGoogleDns',{enabled:this.checked})"><button id="testGoogleDnsWebViewButton" type="button" class="btn btn-secondary" onclick="checkGoogleDnsWebView()">Kiểm tra từ WebView2</button></div>
      <div class="row" id="googleDnsRestartRow" hidden><div class="row-info"><span class="row-label">Cần khởi động lại</span><span class="row-desc">Tab thường được lưu và khôi phục. Cửa sổ ẩn danh sẽ đóng để bảo vệ riêng tư.</span></div><button type="button" class="btn btn-secondary" onclick="if(confirm('Khởi động lại Pip để áp dụng Google DNS? Tab thường sẽ được lưu và khôi phục; cửa sổ ẩn danh sẽ đóng.')) sendAction('RestartBrowser')">Khởi động lại Pip</button></div>
    </div>

    <div class="section-title">Hồ sơ và riêng tư</div>
    <div class="card">
      <div class="row"><div class="row-info"><span class="row-label">Pip Sync mã hóa đầu-cuối</span><span class="row-desc"><span id="syncState">Dữ liệu được mã hóa trên thiết bị trước khi gửi. Dấu trang đồng bộ khi bật Sync.</span> Lịch sử và tab có thể chứa URL nhạy cảm. Mật khẩu chỉ đồng bộ khi bật riêng; cookie và thông tin thanh toán không đồng bộ. Bất kỳ ai có mã khôi phục đều có thể giải mã dữ liệu. Giữ mã ở nơi riêng.</span></div><button class="btn btn-secondary" id="syncCreateButton" onclick="profileAction('EnableBookmarkSync')">Tạo mã đồng bộ</button><button class="btn btn-secondary" id="syncImportButton" onclick="const code=prompt('Nhập mã khôi phục Pip Sync trên thiết bị khác');if(code)profileAction('ImportBookmarkSync',{recovery_code:code})">Nhập mã</button><button class="btn btn-secondary" id="syncCodeButton" onclick="profileAction('ShowBookmarkSyncCode')">Hiện mã khôi phục</button><button class="btn" id="syncNowButton" onclick="profileAction('SyncBookmarks')">Đồng bộ ngay</button><button class="btn btn-secondary" id="syncDisableButton" onclick="if(confirm('Xóa dữ liệu đồng bộ khỏi máy chủ và tắt Pip Sync trong hồ sơ này?'))profileAction('DisableBookmarkSync')">Tắt và xóa</button></div>
      <div class="row"><div class="row-info"><span class="row-label">Chọn dữ liệu đồng bộ</span><span class="row-desc">Bật riêng từng mục. Tab riêng tư và trang Pip nội bộ không được gửi. Cài đặt chỉ gồm các tùy chọn được liệt kê. Mật khẩu dùng kho Pip riêng, được mã hóa đầu-cuối khi gửi và bảo vệ lại theo tài khoản Windows khi nhận; không gồm mật khẩu WebView2, cookie hoặc thẻ thanh toán. Tắt mục chỉ dừng cập nhật; dùng “Tắt và xóa” để xóa toàn bộ kho trên máy chủ.</span></div><label><input id="syncHistoryToggle" type="checkbox" onchange="setSyncCategories()"> Lịch sử</label><label><input id="syncTabsToggle" type="checkbox" onchange="setSyncCategories()"> Tab đang mở</label><label><input id="syncSettingsToggle" type="checkbox" onchange="setSyncCategories()"> Cài đặt cho phép</label><label><input id="syncPasswordsToggle" type="checkbox" onchange="setSyncCategories()"> Mật khẩu Pip</label></div>
      <div class="row"><div class="row-info"><span class="row-label">Tab từ thiết bị khác</span><span class="row-desc">Xuất hiện sau khi đồng bộ; chọn tab để mở trong cửa sổ hiện tại.</span><div id="syncRemoteTabs" class="empty-state">Chưa tải danh sách tab thiết bị khác.</div></div></div>
      <div class="row"><div class="row-info"><span class="row-label">Hồ sơ đang dùng</span><span class="row-desc">Cookie, đăng nhập và dữ liệu website được tách theo hồ sơ. Không đồng bộ tài khoản Chrome.</span></div><strong id="currentProfileName"></strong></div>
      <div class="row"><select id="profileSelect" title="Chọn hồ sơ để mở cửa sổ riêng hoặc đổi tên" aria-label="Hồ sơ"></select><button class="btn btn-secondary" onclick="profileAction('OpenProfile',{id:profileSelect.value})">Mở hồ sơ</button></div>
      <div class="row"><input id="profileName" class="text-input" maxlength="40" placeholder="Tên hồ sơ" aria-label="Tên hồ sơ"><button class="btn btn-secondary" onclick="profileAction('CreateProfile',{name:profileName.value})">Tạo hồ sơ</button><button class="btn btn-secondary" onclick="profileAction('RenameProfile',{id:profileSelect.value,name:profileName.value})">Đổi tên hồ sơ đã chọn</button><button class="btn btn-secondary" id="deleteProfileButton" onclick="if(confirm('Xóa vĩnh viễn hồ sơ đã chọn cùng cookie, dữ liệu website, mật khẩu, dấu trang, lịch sử và phiên tab? Pip sẽ khởi động lại. Lịch sử tải xuống bị xóa, các tệp đã tải vẫn được giữ.'))profileAction('DeleteProfile',{id:profileSelect.value})">Xóa hồ sơ</button></div>
      <div id="profileDeleteHint" class="row-desc" role="status" aria-live="polite"></div>
      <div class="row"><div class="row-info"><span class="row-label">Cửa sổ ẩn danh</span><span class="row-desc">Không ghi lịch sử hoặc lưu phiên tab. Dữ liệu website của phiên ẩn danh được bỏ khi đóng toàn bộ cửa sổ ẩn danh của hồ sơ; tệp đã tải vẫn còn.</span></div><button class="btn btn-secondary" onclick="sendAction('OpenPrivateWindow')">Mở ẩn danh</button></div>
      <div class="row"><div class="row-info"><span class="row-label">Xóa cookie và dữ liệu website</span><span class="row-desc">Xóa cookie và dữ liệu website của hồ sơ hiện tại để đăng xuất; các trang đang mở có thể cần tải lại. Giữ dấu trang, lịch sử, mật khẩu và tệp tải xuống.</span></div><button class="btn btn-secondary" id="clearSiteDataButton" onclick="if(confirm('Xóa cookie và dữ liệu website của hồ sơ hiện tại? Bạn sẽ bị đăng xuất khỏi các website.')) sendAction('ClearSiteData')">Xóa dữ liệu website</button></div>
    </div>
    <div class="section-title">Mật khẩu và tự điền</div>
    <div class="card">
      <div class="row"><div class="row-info"><span class="row-label">Hỏi lưu mật khẩu sau khi đăng nhập</span><span class="row-desc">WebView2 hiển thị lời nhắc Lưu/Cập nhật mật khẩu sau khi bạn đăng nhập. Dữ liệu lưu trong kho WebView2 riêng, không phải kho Pip; tắt mục này sẽ ngừng lời nhắc.</span></div><input id="passwordAutosaveToggle" type="checkbox" aria-label="Hỏi lưu mật khẩu sau khi đăng nhập" onchange="profileAction('SetPasswordAutosave',{enabled:this.checked})"></div>
      <div class="row"><div class="row-info"><span class="row-label">Tự điền thông tin cá nhân của WebView2</span><span class="row-desc">Cho phép gợi ý và lưu tên, địa chỉ, email, số điện thoại cùng dữ liệu biểu mẫu khác. Không ảnh hưởng lời nhắc mật khẩu ở trên.</span></div><input id="autofillToggle" type="checkbox" aria-label="Tự điền thông tin cá nhân" onchange="profileAction('SetAutofill',{enabled:this.checked})"></div>
      <div class="row"><div class="row-info"><span class="row-label">Kho mật khẩu Pip</span><span class="row-desc" id="passwordVaultHint">Được mã hóa theo tài khoản Windows và tách riêng theo hồ sơ. Chọn tab website đang mở để lưu; Pip tự lấy địa chỉ từ tab đó, chỉ điền vào đúng địa chỉ website và không tự gửi biểu mẫu.</span></div></div>
      <form class="password-form" onsubmit="savePasswordFromForm();return false">
        <select id="passwordSaveTargetTab" class="select-box password-wide" aria-label="Tab website lưu mật khẩu"><option value="">Chọn tab website để lưu…</option></select>
        <input id="passwordUsername" class="text-input" autocomplete="username" placeholder="Tên đăng nhập" aria-label="Tên đăng nhập" required>
        <input id="passwordValue" class="text-input password-wide" type="password" autocomplete="new-password" placeholder="Mật khẩu" aria-label="Mật khẩu" required>
        <button class="btn password-wide" id="savePasswordButton" type="submit">Lưu mật khẩu cho tab đã chọn</button>
      </form>
      <div class="row"><label for="passwordTargetTab">Điền vào tab đang mở</label><select id="passwordTargetTab" class="select-box" aria-label="Tab nhận mật khẩu"><option value="">Chọn tab website…</option></select></div>
      <div id="savedPasswordsList" class="password-list" role="list" aria-live="polite"><span class="empty-state" role="listitem">Đang tải mật khẩu đã lưu…</span></div>
      <div class="row"><div class="row-info"><span class="row-label">Xóa mật khẩu đã lưu của hồ sơ này</span><span class="row-desc">Xóa cả mật khẩu Pip và mật khẩu được WebView2 lưu.</span></div><button class="btn btn-secondary" id="clearPasswordsButton" onclick="if(confirm('Xóa toàn bộ mật khẩu Pip và WebView2 đã lưu trong hồ sơ này?')) sendAction('ClearSavedPasswords')">Xóa mật khẩu</button></div>
    </div>

    <div class="section-title">Tiện ích</div>
    <div class="card">
      <div class="row"><div class="row-info"><span class="row-label">Tiện ích mở rộng WebView2</span><span class="row-desc" id="browserExtensionsHint">Cài tiện ích tương thích từ thư mục đã giải nén. Pip không tải tiện ích từ Chrome Web Store.</span><span class="row-desc" id="browserExtensionsCompatibilityHint">WebView2 không có thanh công cụ để hiển thị biểu tượng/badge hoặc mở popup tiện ích; Pip cũng chưa tích hợp mục chuột phải do tiện ích khai báo. Tiện ích vẫn có thể cài, nhưng tiện ích phụ thuộc vào popup có thể không dùng được; script nền/nội dung vẫn có thể hoạt động. Pip không liệt kê quyền riêng từng tiện ích—hãy xem "permissions" và "host_permissions" trong manifest trước khi cài.</span></div><button class="btn" id="addBrowserExtensionButton" onclick="if(confirm('Chỉ cài tiện ích bạn tin cậy. Tiện ích có thể đọc hoặc thay đổi dữ liệu website theo quyền trong manifest. Hãy xem permissions và host_permissions trong manifest.json trước khi cài. Chọn thư mục đã giải nén; giữ nguyên thư mục sau khi cài.'))profileAction('SelectBrowserExtensionFolder')">Thêm tiện ích</button><button class="btn btn-secondary" id="refreshBrowserExtensionsButton" onclick="profileAction('GetBrowserExtensions')">Tải lại</button></div>
      <div id="browserExtensionsList" class="extension-list" role="list" aria-live="polite"><span class="empty-state" role="listitem">Đang tải tiện ích…</span></div>
    </div>

    <div id="statusBanner" class="status-banner info" role="status" aria-live="polite">
      <span id="statusText">Đang xử lý...</span>
    </div>
  </div>

  <script>
    (()=>{
      const nav=document.getElementById('settingsNav');
      document.querySelectorAll('.section-title').forEach((section,index)=>{
        const id='settings-section-'+index;
        section.id=id;
        const link=document.createElement('button');
        link.type='button';link.textContent=section.textContent.trim();
        link.setAttribute('aria-controls',id);
        if(index===0)link.setAttribute('aria-current','location');
        link.addEventListener('click',()=>{
          document.getElementById(id).scrollIntoView({block:'start',behavior:'auto'});
          nav.querySelectorAll('button[aria-current]').forEach(item=>item.removeAttribute('aria-current'));
          link.setAttribute('aria-current','location');
        });
        nav.append(link);
      });
    })();
    document.querySelectorAll('.row').forEach((row,index)=>{
      const description=row.querySelector('.row-desc');
      if(description){description.id ||= 'setting-description-'+index;row.querySelectorAll('input,select,button').forEach(control=>control.setAttribute('aria-describedby',description.id));}
    });
    const currentProfileId = '{{CURRENT_PROFILE}}';
    const isPrivateProfile = {{CURRENT_PRIVATE}};
    const profileSelect = document.getElementById('profileSelect');
    const deleteProfileButton = document.getElementById('deleteProfileButton');
    const profileDeleteHint = document.getElementById('profileDeleteHint');
    function updateDeleteProfileButton() {
      const selected = Array.from(profileSelect.options).find(profile=>profile.value===profileSelect.value);
      let reason='';
      if(isPrivateProfile)reason='Không thể xóa hồ sơ trong cửa sổ ẩn danh.';
      else if(!selected)reason='Chọn một hồ sơ để xem tùy chọn xóa.';
      else if(selected.value==='Default')reason='Không thể xóa hồ sơ mặc định.';
      else if(selected.dataset.syncEnabled==='true')reason='Hãy tắt và xóa Pip Sync của hồ sơ trước.';
      else if(selected.dataset.open!=='true')reason=selected.dataset.privateOpen==='true'?'Mở hồ sơ trong cửa sổ Pip bình thường trước khi xóa; cửa sổ ẩn danh không thể xóa dữ liệu hồ sơ.':'Mở hồ sơ trong cửa sổ Pip bình thường trước khi xóa.';
      deleteProfileButton.disabled=!!reason;
      deleteProfileButton.title=reason;
      profileDeleteHint.textContent=reason;
    }
    profileSelect.addEventListener('change',updateDeleteProfileButton);
    const sitePermissionsList = document.getElementById('sitePermissionsList');
    document.getElementById('refreshSitePermissionsButton').disabled=isPrivateProfile;
    if(isPrivateProfile){document.getElementById('sitePermissionsHint').textContent='Quyền hồ sơ đã lưu không thể quản lý trong cửa sổ ẩn danh.';sitePermissionsList.replaceChildren(Object.assign(document.createElement('span'),{className:'empty-state',role:'listitem',textContent:'Không khả dụng trong ẩn danh.'}));}
    window.__renderSitePermissions = function(permissions) {
      sitePermissionsList.replaceChildren();
      if(!Array.isArray(permissions)||permissions.length===0){sitePermissionsList.append(Object.assign(document.createElement('span'),{className:'empty-state',role:'listitem',textContent:'Chưa có quyền website nào được lưu.'}));return;}
      for(const permission of permissions){
        const row=document.createElement('div');row.className='site-permission-item';row.setAttribute('role','listitem');
        const detail=document.createElement('div');detail.className='site-permission-detail';
        const origin=document.createElement('strong');origin.textContent=permission.origin;
        const state=document.createElement('span');state.textContent=`${permission.kind_name} · ${permission.state_name}`;
        detail.append(origin,state);
        const reset=document.createElement('button');reset.type='button';reset.className='btn btn-secondary';reset.textContent='Đặt lại';reset.setAttribute('aria-label',`Đặt lại quyền ${permission.kind_name} cho ${permission.origin}`);
        reset.addEventListener('click',()=>{if(confirm(`Đặt lại quyền ${permission.kind_name} của ${permission.origin}? Website sẽ hỏi lại lần sau.`))profileAction('ResetSitePermission',{origin:permission.origin,kind:permission.kind_id});});
        row.append(detail,reset);sitePermissionsList.append(row);
      }
    };
    function profileAction(action,payload) {
      window.ipc.postMessage(JSON.stringify({action,payload}));
    }
    window.__renderProfiles = function(profiles) {
      const select = document.getElementById('profileSelect');
      select.replaceChildren();
      for (const profile of profiles) {
        const option = new Option(profile.name,profile.id);
        option.selected = profile.id === currentProfileId;
        select.add(option);
      }
      const current = profiles.find(p=>p.id===currentProfileId);
      window.__syncPasswordsEnabled=!!current?.syncPasswords;
      document.getElementById('currentProfileName').textContent=current?.name || currentProfileId;
      document.getElementById('autofillToggle').checked=!isPrivateProfile && !!current?.autofill;
      document.getElementById('autofillToggle').disabled=isPrivateProfile;
      document.getElementById('passwordAutosaveToggle').checked=!isPrivateProfile && !!current?.passwordAutosave;
      document.getElementById('passwordAutosaveToggle').disabled=isPrivateProfile;
      document.getElementById('clearPasswordsButton').disabled=isPrivateProfile;
      for(const id of ['passwordSaveTargetTab','passwordUsername','passwordValue','savePasswordButton','passwordTargetTab'])document.getElementById(id).disabled=isPrivateProfile;
      for(const id of ['addBrowserExtensionButton','refreshBrowserExtensionsButton'])document.getElementById(id).disabled=isPrivateProfile;
      document.getElementById('clearCacheButton').disabled=isPrivateProfile;
      document.getElementById('clearSelectedBrowsingDataButton').disabled=isPrivateProfile;
      for(const id of ['clearDataHistory','clearDataSite','clearDataCache','clearDataPasswords','clearDataAutofill','clearDataDownloads'])document.getElementById(id).disabled=isPrivateProfile;
      document.getElementById('syncCreateButton').disabled=isPrivateProfile || !!current?.syncEnabled;
      document.getElementById('syncImportButton').disabled=isPrivateProfile || !!current?.syncEnabled;
      document.getElementById('syncNowButton').disabled=isPrivateProfile || !current?.syncEnabled || !!current?.syncBusy;
      document.getElementById('syncCodeButton').disabled=isPrivateProfile || !current?.syncEnabled;
      document.getElementById('syncDisableButton').disabled=isPrivateProfile || !current?.syncEnabled;
      for(const [id,key] of [['syncHistoryToggle','syncHistory'],['syncTabsToggle','syncOpenTabs'],['syncSettingsToggle','syncSettings'],['syncPasswordsToggle','syncPasswords']]){const control=document.getElementById(id);control.checked=!isPrivateProfile&&!!current?.[key];control.disabled=isPrivateProfile||!current?.syncEnabled||!!current?.syncBusy;}
      for(const option of select.options){const profile=profiles.find(p=>p.id===option.value);option.dataset.syncEnabled=profile?.syncEnabled?'true':'false';option.dataset.open=profile?.isOpen?'true':'false';option.dataset.privateOpen=profile?.isPrivateOpen?'true':'false';}
      updateDeleteProfileButton();
      document.getElementById('syncState').textContent=isPrivateProfile?'Đồng bộ bị tắt trong cửa sổ ẩn danh.':current?.syncEnabled?'Đã thiết lập mã hóa. Chọn dữ liệu muốn đồng bộ rồi bấm Đồng bộ ngay.':'Mã hóa trên thiết bị trước khi gửi.';
    };
    function setSyncCategories(){const passwords=document.getElementById('syncPasswordsToggle');if(passwords.checked&&!window.__syncPasswordsEnabled&&!confirm('Pip sẽ gửi riêng mật khẩu trong Kho mật khẩu Pip đã mã hóa. Người có mã khôi phục có thể đọc chúng. Không bao gồm mật khẩu WebView2, cookie hoặc thông tin thẻ. Tiếp tục bật?')){passwords.checked=false;return}profileAction('SetSyncCategories',{history:document.getElementById('syncHistoryToggle').checked,open_tabs:document.getElementById('syncTabsToggle').checked,settings:document.getElementById('syncSettingsToggle').checked,passwords:passwords.checked});}
    window.__renderSyncedTabs=function(devices,localDeviceId){const target=document.getElementById('syncRemoteTabs');target.replaceChildren();const remote=(Array.isArray(devices)?devices:[]).filter(device=>device.device_id!==localDeviceId);if(!remote.length){target.textContent='Chưa có tab từ thiết bị khác.';return}for(const device of remote){const group=document.createElement('div');group.className='site-permission-item';const heading=document.createElement('strong');heading.textContent=`Thiết bị ${String(device.device_id||'').slice(0,8)}`;group.append(heading);for(const tab of (Array.isArray(device.tabs)?device.tabs:[])){const button=document.createElement('button');button.type='button';button.className='btn btn-secondary';button.textContent=tab.title||tab.url;button.title=tab.url;button.addEventListener('click',()=>profileAction('CreateTab',{url:tab.url}));group.append(button)}target.append(group)}};
    window.__renderProfiles({{PROFILES_JSON}});
    if(!isPrivateProfile)profileAction('GetSitePermissions');
    if(!isPrivateProfile)profileAction('GetSavedPasswords');
    const browserExtensionsList=document.getElementById('browserExtensionsList');
    window.__renderBrowserExtensions=function(extensions){
      browserExtensionsList.replaceChildren();
      if(!Array.isArray(extensions)||extensions.length===0){browserExtensionsList.append(Object.assign(document.createElement('span'),{className:'empty-state',role:'listitem',textContent:'Chưa cài tiện ích nào trong hồ sơ này.'}));return}
      for(const extension of extensions){
        const row=document.createElement('div');row.className='extension-item';row.setAttribute('role','listitem');
        const detail=document.createElement('div');detail.className='extension-detail';
        const name=document.createElement('strong');name.textContent=extension.name||'Tiện ích không tên';
        const id=document.createElement('span');id.textContent=`${extension.enabled?'Đang bật':'Đang tắt'} · ${extension.id}`;
        detail.append(name,id);
        const actions=document.createElement('div');actions.className='extension-actions';
        const toggle=document.createElement('button');toggle.type='button';toggle.className='btn btn-secondary';toggle.textContent=extension.enabled?'Tắt':'Bật';toggle.setAttribute('aria-label',`${extension.enabled?'Tắt':'Bật'} ${extension.name}`);toggle.addEventListener('click',()=>profileAction('SetBrowserExtensionEnabled',{id:extension.id,enabled:!extension.enabled}));
        const remove=document.createElement('button');remove.type='button';remove.className='btn btn-secondary';remove.textContent='Gỡ';remove.setAttribute('aria-label',`Gỡ ${extension.name}`);remove.addEventListener('click',()=>{if(confirm(`Gỡ ${extension.name} khỏi hồ sơ này?`))profileAction('RemoveBrowserExtension',{id:extension.id})});
        actions.append(toggle,remove);row.append(detail,actions);browserExtensionsList.append(row);
      }
    };
    window.__renderBrowserExtensionsError=function(message){browserExtensionsList.replaceChildren(Object.assign(document.createElement('span'),{className:'empty-state',role:'listitem',textContent:message||'WebView2 chưa thể đọc tiện ích.'}))};
    if(isPrivateProfile){document.getElementById('browserExtensionsHint').textContent='Quản lý tiện ích không khả dụng trong cửa sổ ẩn danh.';browserExtensionsList.replaceChildren(Object.assign(document.createElement('span'),{className:'empty-state',role:'listitem',textContent:'Không khả dụng trong ẩn danh.'}));}
    else profileAction('GetBrowserExtensions');
    const savedPasswordsList=document.getElementById('savedPasswordsList');
    window.__renderPasswords=function(entries,targets){
      const tabSelect=document.getElementById('passwordTargetTab');
      const saveTabSelect=document.getElementById('passwordSaveTargetTab');
      const previous=tabSelect.value;
      const previousSaveTarget=saveTabSelect.value;
      tabSelect.replaceChildren(new Option('Chọn tab website…',''));
      saveTabSelect.replaceChildren(new Option('Chọn tab website để lưu…',''));
      for(const target of (Array.isArray(targets)?targets:[])){const label=`${target.title||target.origin} — ${target.origin}`;tabSelect.add(new Option(label,String(target.id)));const saveOption=new Option(label,String(target.id));saveOption.dataset.origin=target.origin;saveTabSelect.add(saveOption)}
      if([...tabSelect.options].some(option=>option.value===previous))tabSelect.value=previous;
      if([...saveTabSelect.options].some(option=>option.value===previousSaveTarget))saveTabSelect.value=previousSaveTarget;
      savedPasswordsList.replaceChildren();
      if(!Array.isArray(entries)||entries.length===0){savedPasswordsList.append(Object.assign(document.createElement('span'),{className:'empty-state',role:'listitem',textContent:'Chưa có mật khẩu Pip nào. Nhập website, tên đăng nhập và mật khẩu ở trên.'}));return}
      for(const entry of entries){
        const row=document.createElement('div');row.className='password-item';row.dataset.passwordId=String(entry.id);row.setAttribute('role','listitem');
        const detail=document.createElement('div');detail.className='password-detail';
        const site=document.createElement('strong');site.textContent=entry.origin;
        const user=document.createElement('span');user.textContent=entry.username;
        const reveal=document.createElement('input');reveal.type='password';reveal.readOnly=true;reveal.autocomplete='off';reveal.setAttribute('aria-label',`Mật khẩu cho ${entry.origin}`);reveal.hidden=true;
        detail.append(site,user,reveal);
        const actions=document.createElement('div');actions.className='password-actions';
        const revealButton=document.createElement('button');revealButton.type='button';revealButton.className='btn btn-secondary';revealButton.textContent='Hiện';revealButton.addEventListener('click',()=>{if(reveal.hidden){profileAction('RevealPassword',{id:entry.id});}else{reveal.value='';reveal.hidden=true;revealButton.textContent='Hiện';}});
        revealButton.setAttribute('aria-label',`Hiện mật khẩu ${entry.id}`);
        const fill=document.createElement('button');fill.type='button';fill.className='btn btn-secondary';fill.textContent='Điền';fill.addEventListener('click',()=>{const target=Number(tabSelect.value);if(!Number.isSafeInteger(target)||target<=0){showStatus('Chọn tab website cần điền trước.');return}profileAction('FillPassword',{id:entry.id,tab_id:target})});
        const remove=document.createElement('button');remove.type='button';remove.className='btn btn-secondary';remove.textContent='Xóa';remove.setAttribute('aria-label',`Xóa mật khẩu ${entry.username} tại ${entry.origin}`);remove.addEventListener('click',()=>{if(confirm(`Xóa mật khẩu ${entry.username} tại ${entry.origin}?`))profileAction('DeletePassword',{id:entry.id})});
        actions.append(revealButton,fill,remove);row.append(detail,actions);savedPasswordsList.append(row);
      }
    };
    window.__showSavedPassword=function(id,password){const row=[...savedPasswordsList.querySelectorAll('.password-item')].find(item=>item.dataset.passwordId===String(id));if(row){const field=row.querySelector('input');field.value=password;field.hidden=false;const button=row.querySelector('.password-actions button');button.textContent='Ẩn';button.setAttribute('aria-label',`Ẩn mật khẩu ${id}`)}};
    if(isPrivateProfile){document.getElementById('passwordVaultHint').textContent='Kho mật khẩu bị tắt trong cửa sổ ẩn danh.';savedPasswordsList.replaceChildren(Object.assign(document.createElement('span'),{className:'empty-state',role:'listitem',textContent:'Không khả dụng trong ẩn danh.'}));}
    function savePasswordFromForm(){const tabId=Number(document.getElementById('passwordSaveTargetTab').value);const username=document.getElementById('passwordUsername').value.trim();const password=document.getElementById('passwordValue').value;if(!Number.isSafeInteger(tabId)||tabId<=0||!username||!password){showStatus('Chọn tab website rồi nhập tên đăng nhập và mật khẩu.');return}const target=[...document.getElementById('passwordSaveTargetTab').selectedOptions][0];const origin=target?.dataset.origin||target?.textContent?.split(' — ').at(-1);const exists=[...savedPasswordsList.querySelectorAll('.password-item')].some(row=>row.querySelector('strong')?.textContent===origin&&row.querySelector('.password-detail span')?.textContent===username);if(exists&&!confirm('Mật khẩu cùng website và tên đăng nhập sẽ được cập nhật. Tiếp tục?'))return;profileAction('SavePasswordForTab',{tab_id:tabId,username,password});document.getElementById('passwordValue').value=''}
    window.__renderGoogleDns = (enabled, active=enabled, failed=false, routeOk=null, fallbackUsed=false, resolutionFailed=false) => {
      document.getElementById('googleDnsToggle').checked=enabled;
      const applied=active||failed;
      const pending=enabled!==applied;
      document.getElementById('googleDnsRestartRow').hidden=!pending;
      const status=pending
          ? 'Đã lưu lựa chọn nhưng proxy chưa được áp dụng. Nhấn “Khởi động lại Pip” để áp dụng, sau đó kiểm tra lại.'
          : failed&&applied
            ? 'Pip không khởi động được proxy DNS riêng; các trang theo tên miền đang bị chặn để tránh dùng DNS Windows. Mở Cài đặt > Kết nối, tắt Google DNS rồi khởi động lại nếu muốn dùng DNS Windows.'
            : fallbackUsed&&resolutionFailed
            ? 'Google DoH đã lỗi trong một số yêu cầu; Pip đã dùng DNS Windows dự phòng cho một số tên miền và cả hai bộ phân giải cùng lỗi với ít nhất một yêu cầu. Google DNS không áp dụng cho toàn bộ yêu cầu và một số trang có thể không mở được.'
            : resolutionFailed
            ? 'Google DoH và DNS Windows đều không phân giải được ít nhất một tên miền được yêu cầu. Một số trang có thể không mở được; hãy kiểm tra mạng hoặc tường lửa.'
            : fallbackUsed
            ? 'Google DoH đã lỗi khi phân giải ít nhất một tên miền; Pip đã dùng DNS Windows dự phòng cho yêu cầu đó. Google DNS không áp dụng cho toàn bộ yêu cầu trong phiên này, và DNS Windows có thể làm lộ tên miền cho mạng đang dùng.'
            : active&&routeOk===false
            ? 'Proxy Google DNS đang chạy nhưng yêu cầu HTTPS nền thất bại. Kết quả này không thay cho phép thử WebView2; bấm “Kiểm tra từ WebView2” để thử YouTube theo cấu hình hiện hành. DNS Windows không đổi.'
            : active&&routeOk===null
              ? 'Proxy Google DNS đang chạy. Pip đang kiểm tra HTTPS nền qua proxy; nút “Kiểm tra từ WebView2” thử riêng yêu cầu tới YouTube từ trang cài đặt.'
              : active
                ? 'Kiểm tra proxy DoH nền thành công: một yêu cầu HTTPS đã tải qua proxy Google. Nút “Kiểm tra từ WebView2” thử riêng YouTube trong WebView2; kết quả no-cors không cho biết mã HTTP và không bắt gói để xác minh từng truy vấn DNS. DNS Windows không bị thay đổi; địa chỉ IP không được ẩn.'
                : 'Đang dùng DNS Windows. Bật Google DNS để dùng tuyến HTTPS riêng cho website và tác vụ HTTPS nền trong Pip.';
      document.getElementById('googleDnsHint').textContent=status+' Cập nhật WebView2 bằng PowerShell vẫn dùng mạng Windows.';
    };
    window.__renderGoogleDns({{GOOGLE_DNS_ENABLED}},{{GOOGLE_DNS_ACTIVE}},{{GOOGLE_DNS_FAILED}},{{GOOGLE_DNS_ROUTE_OK}},{{GOOGLE_DNS_FALLBACK_USED}},{{GOOGLE_DNS_RESOLUTION_FAILED}});

    function probeGoogleDnsWebView(fetchImpl = window.fetch, timeoutMs = 5000) {
      const controller = new AbortController();
      const timeoutDelay = Number.isFinite(timeoutMs) ? Math.max(1, timeoutMs) : 5000;

      return new Promise(resolve => {
        let settled = false;
        let timer;
        const finish = result => {
          if (settled) return;
          settled = true;
          clearTimeout(timer);
          resolve(result);
        };

        timer = setTimeout(() => {
          controller.abort();
          finish({ ok: false, kind: 'timeout' });
        }, timeoutDelay);

        try {
          Promise.resolve(fetchImpl.call(window, 'https://www.youtube.com/generate_204', {
            method: 'GET',
            mode: 'no-cors',
            credentials: 'omit',
            referrer: '',
            referrerPolicy: 'no-referrer',
            cache: 'no-store',
            signal: controller.signal
          })).then(
            () => finish({ ok: true, kind: 'response' }),
            () => finish({ ok: false, kind: controller.signal.aborted ? 'timeout' : 'network' })
          );
        } catch (_) {
          finish({ ok: false, kind: controller.signal.aborted ? 'timeout' : 'network' });
        }
      });
    }

    window.__pipGoogleDnsWebViewProbe = probeGoogleDnsWebView;

    async function checkGoogleDnsWebView() {
      const button = document.getElementById('testGoogleDnsWebViewButton');
      if (button) button.disabled = true;
      showStatus('Đang thử yêu cầu HTTPS từ WebView2 tới YouTube...');
      let result;
      try {
        result = await probeGoogleDnsWebView();
      } catch (_) {
        result = { ok: false, kind: 'network' };
      }
      if (result.ok) {
        showStatus('WebView2 đã nhận phản hồi HTTPS từ YouTube theo tuyến mạng hiện hành (proxy cục bộ Pip nếu Google DNS đang bật). no-cors không cho biết mã HTTP hay khả năng phát video.');
      } else if (result.kind === 'timeout') {
        showStatus('WebView2 chưa nhận kết quả từ YouTube sau 5 giây. Hãy kiểm tra kết nối hoặc tuyến DNS rồi thử lại.');
      } else {
        showStatus('WebView2 không thể hoàn tất yêu cầu tới YouTube theo tuyến mạng hiện hành. Hãy kiểm tra proxy, DNS, mạng hoặc tường lửa.');
      }
      if (button) button.disabled = false;
    }

    function sendAction(action) {
      if (window.ipc) {
        window.ipc.postMessage(JSON.stringify({ action: action, payload: null }));
        showStatus('Đang xử lý...');
      } else {
        console.log('[IPC OUT]', action);
        showStatus('Mở trang này trong Pip để sử dụng cài đặt.');
      }
    }

    function clearSelectedBrowsingData() {
      const selected={
        history:document.getElementById('clearDataHistory').checked,
        site_data:document.getElementById('clearDataSite').checked,
        cache:document.getElementById('clearDataCache').checked,
        passwords:document.getElementById('clearDataPasswords').checked,
        autofill:document.getElementById('clearDataAutofill').checked,
        download_history:document.getElementById('clearDataDownloads').checked,
      };
      if(!Object.values(selected).some(Boolean)){showStatus('Hãy chọn ít nhất một loại dữ liệu cần xóa.');return;}
      const effects=[];
      if(selected.history)effects.push('lịch sử duyệt web');
      if(selected.site_data)effects.push('cookie và dữ liệu website (bạn sẽ bị đăng xuất)');
      if(selected.cache)effects.push('bộ nhớ đệm');
      if(selected.passwords)effects.push('mật khẩu đã lưu');
      if(selected.autofill)effects.push('dữ liệu tự điền');
      if(selected.download_history)effects.push('lịch sử tải xuống (tệp vẫn được giữ)');
      if(!confirm('Xóa '+effects.join(', ')+' của hồ sơ hiện tại? Dấu trang, tệp đã tải và các lượt tải đang chạy vẫn được giữ.'))return;
      profileAction('ClearSelectedBrowsingData',selected);
      showStatus('Đang xóa dữ liệu đã chọn...');
    }

    window.__focusClearData = function() {
      const control=document.getElementById('clearDataHistory');
      control?.closest('.row')?.scrollIntoView({block:'center'});
      control?.focus();
    };
    if(new URLSearchParams(location.search).has('clear-data'))requestAnimationFrame(()=>window.__focusClearData());

    function showStatus(text) {
      const banner = document.getElementById('statusBanner');
      const textEl = document.getElementById('statusText');
      textEl.textContent = text;
      banner.classList.add('show');
      setTimeout(() => banner.classList.remove('show'), 4000);
    }

    function onSearchEngineChange(engine) {
      if (window.ipc) {
        window.ipc.postMessage(JSON.stringify({ action: 'SetSearchEngine', payload: { engine: engine } }));
        showStatus('Đã chọn công cụ tìm kiếm ' + engine);
      }
    }

    function onFeatureToggleChange(feature, value) {
      if (window.ipc) {
        window.ipc.postMessage(JSON.stringify({
          action: 'SaveSettings',
          payload: { settings_json: JSON.stringify({ features: { [feature]: value }, ...(feature === 'enable_link_preview' ? {appearance:{enable_link_preview:value,show_status_preview:value}} : {}) }) }
        }));
        showStatus('Đang lưu lựa chọn...');
      }
    }

    function onToggleChange(setting, value) {
      if (window.ipc) {
        let payloadObj = {};
        if (setting === 'backgroundThrottling') {
          payloadObj = { performance: { background_throttling: value } };
          const delay=document.getElementById('sleepAfterSelect');
          if(delay)delay.disabled=!value;
        } else if (setting === 'askWhereToSave' || setting === 'showProgressToolbar') {
          payloadObj = { downloads: { [setting === 'askWhereToSave' ? 'ask_where_to_save' : 'show_progress_toolbar']: value } };
        } else if (setting === 'tabReordering' || setting === 'tabTearoff' || setting === 'warnOnCloseTabs') {
          payloadObj = { tabs: { [setting === 'tabReordering' ? 'enable_tab_reordering' : setting === 'tabTearoff' ? 'enable_tab_tearoff' : 'warn_on_close_tabs']: value } };
        } else if (setting === 'showZoomBadge') {
          payloadObj = { appearance: { show_zoom_badge: value } };
        } else {
          payloadObj = { [setting]: value };
        }
        window.ipc.postMessage(JSON.stringify({
          action: 'SaveSettings',
          payload: { settings_json: JSON.stringify(payloadObj) }
        }));
        showStatus('Đang lưu lựa chọn...');
      }
    }

    function onIdleSleepChange(seconds) {
      if (!window.ipc) return;
      window.ipc.postMessage(JSON.stringify({action:'SaveSettings',payload:{settings_json:JSON.stringify({performance:{sleep_after_secs:Number(seconds)}})}}));
      showStatus(Number(seconds)===0?'Đã tắt tự ngủ tab nền.':'Đã cập nhật thời gian tự ngủ tab nền.');
    }

    function onAdblockModeChange(mode) {
      if (!window.ipc) return;
      window.ipc.postMessage(JSON.stringify({action:'SaveSettings',payload:{settings_json:JSON.stringify({adblock_mode:mode})}}));
      showStatus('Đang lưu mức chặn quảng cáo…');
    }

    function onDefaultZoomChange(zoom) {
      if (window.ipc) {
        window.ipc.postMessage(JSON.stringify({
          action: 'SaveSettings',
          payload: { settings_json: JSON.stringify({ default_zoom_level: parseFloat(zoom) }) }
        }));
        showStatus('Đã chọn tỷ lệ trang ' + Math.round(parseFloat(zoom) * 100) + '%');
      }
    }

    function onAppearanceSelectChange(key, value) {
      if (!window.ipc) return;
      window.ipc.postMessage(JSON.stringify({action:'SaveSettings',payload:{settings_json:JSON.stringify({appearance:{[key]:value}})}}));
      showStatus('Đang lưu giao diện cho hồ sơ này...');
    }

    function onPermissionChange(perm, value) {
      if (window.ipc) {
        window.ipc.postMessage(JSON.stringify({
          action: 'SaveSettings',
          payload: { settings_json: JSON.stringify({ permissions: { [perm]: value } }) }
        }));
        showStatus('Đang lưu quyền truy cập...');
      }
    }

    window.__syncSearchEngine = function(engine) {
      const select = document.getElementById('searchEngineSelect');
      if (select && engine) {
        select.value = engine.toLowerCase();
      }
    };

    window.__syncEngineInfo = function(version) {
      const el = document.getElementById('engineVersion');
      if (el && version) {
        const cleanVer = version.startsWith('v') ? version : 'v' + version;
        el.textContent = cleanVer.includes('(Active)') ? cleanVer : cleanVer + ' (Active)';
      }
    };

    function checkAppUpdates() {
      document.getElementById('appUpdateStatus').textContent='Đang kiểm tra GitHub…';
      document.getElementById('openAppReleaseButton').hidden=true;
      profileAction('CheckAppUpdates');
    }
    window.__renderAppUpdate = function(result) {
      const status=document.getElementById('appUpdateStatus');
      const link=document.getElementById('openAppReleaseButton');
      if(!status||!link)return;
      if(result?.error){status.textContent=result.error;link.hidden=true;return}
      const current='v'+result.currentVersion, latest='v'+result.latestVersion;
      if(result.updateAvailable)status.textContent=`Có bản Pip mới ${latest}; bạn đang dùng ${current}.`;
      else if(result.currentVersion===result.latestVersion)status.textContent=`Pip ${current} đang là bản phát hành mới nhất.`;
      else status.textContent=`Pip ${current} mới hơn bản phát hành công khai ${latest}.`;
      try {
        const url=new URL(result.releaseUrl);
        link.href=url.protocol==='https:'&&url.hostname==='github.com'&&url.pathname.startsWith('/giangocquoc-art/pip-browser/releases/tag/')?url.href:'#';
        link.hidden=!result.updateAvailable;
      } catch {link.hidden=true}
    };
    function updateAdblockFilters() {
      document.getElementById('adblockUpdateStatus').textContent='Đang tải và kiểm tra bốn danh sách bộ lọc…';
      const button=document.getElementById('updateAdblockFiltersButton');
      button.disabled=true;
      profileAction('UpdateAdblockFilters');
    }
    window.__renderAdblockFilterUpdate = function(result) {
      const status=document.getElementById('adblockUpdateStatus');
      const button=document.getElementById('updateAdblockFiltersButton');
      if(!status||!button)return;
      button.disabled=false;
      status.textContent=result?.error||`Đã lưu ${result.listCount} danh sách với khoảng ${result.ruleCount.toLocaleString('vi-VN')} luật. Đóng và mở lại Pip để áp dụng.`;
    };

    if (window.__runtimeVersion) {
      window.__syncEngineInfo(window.__runtimeVersion);
    }
  </script>
</body>
</html>
"#;

use evergreen_core::settings::Settings;

pub fn get_settings_html(
    runtime_ver: &str,
    settings: &Settings,
    google_dns_active: bool,
    google_dns_failed: bool,
    google_dns_route_ok: Option<bool>,
    google_dns_fallback_used: bool,
    google_dns_resolution_failed: bool,
) -> String {
    let download_dir_str = settings.downloads.default_folder.to_string_lossy();
    let public_profiles = settings
        .profiles
        .iter()
        .map(|profile| {
            serde_json::json!({
                "id": profile.id,
                "name": profile.name,
                "autofill": profile.autofill,
                "passwordAutosave": profile.password_autosave,
                "syncEnabled": profile.sync_credentials_protected.is_some(),
                "syncHistory": profile.sync_history,
                "syncOpenTabs": profile.sync_open_tabs,
                "syncSettings": profile.sync_settings,
            })
        })
        .collect::<Vec<_>>();
    let mut html = SETTINGS_TEMPLATE
        .replace("{{LOGO_BASE64}}", LOGO_BASE64.trim())
        .replace("{{MASCOT_BASE64}}", MASCOT_BASE64.trim())
        .replace(
            "{{PROFILES_JSON}}",
            &serde_json::to_string(&public_profiles)
                .unwrap()
                .replace('<', "\\u003c"),
        )
        .replace("Đang kiểm tra...", &format!("v{} (Active)", runtime_ver))
        .replace("{{DEFAULT_DOWNLOAD_FOLDER}}", &download_dir_str)
        .replace(
            "{{GOOGLE_DNS_ENABLED}}",
            if settings.google_dns { "true" } else { "false" },
        )
        .replace(
            "{{GOOGLE_DNS_ACTIVE}}",
            if google_dns_active { "true" } else { "false" },
        )
        .replace(
            "{{GOOGLE_DNS_FAILED}}",
            if google_dns_failed { "true" } else { "false" },
        )
        .replace(
            "{{GOOGLE_DNS_ROUTE_OK}}",
            &google_dns_route_ok
                .map(|ok| ok.to_string())
                .unwrap_or_else(|| "null".into()),
        )
        .replace(
            "{{GOOGLE_DNS_FALLBACK_USED}}",
            if google_dns_fallback_used {
                "true"
            } else {
                "false"
            },
        )
        .replace(
            "{{GOOGLE_DNS_RESOLUTION_FAILED}}",
            if google_dns_resolution_failed {
                "true"
            } else {
                "false"
            },
        );

    let selected_adblock_mode = match settings.adblock_mode {
        evergreen_core::settings::AdblockMode::Balanced => "balanced",
        evergreen_core::settings::AdblockMode::Strong => "strong",
        evergreen_core::settings::AdblockMode::VideoFirst => "video_first",
    };
    html = html.replace(
        &format!("value=\"{selected_adblock_mode}\""),
        &format!("value=\"{selected_adblock_mode}\" selected"),
    );

    let engine = settings.search_engine.to_lowercase();
    html = html.replace(
        &format!("id=\"opt_{}\"", engine),
        &format!("id=\"opt_{}\" selected", engine),
    );

    let theme = match settings.appearance.theme.as_str() {
        "light" => "light",
        "dark" => "dark",
        _ => "system",
    };
    html = html.replace(
        &format!("<option value=\"{theme}\">"),
        &format!("<option value=\"{theme}\" selected>"),
    );
    let density = if settings.appearance.density == "compact" {
        "compact"
    } else {
        "comfortable"
    };
    html = html.replace(
        &format!("<option value=\"{density}\">"),
        &format!("<option value=\"{density}\" selected>"),
    );

    for (id, enabled) in [
        ("askWhereToSaveToggle", settings.downloads.ask_where_to_save),
        (
            "showProgressToolbarToggle",
            settings.downloads.show_progress_toolbar,
        ),
        ("tabReorderingToggle", settings.tabs.enable_tab_reordering),
        ("tabTearoffToggle", settings.tabs.enable_tab_tearoff),
        ("warnOnCloseTabsToggle", settings.tabs.warn_on_close_tabs),
        ("showZoomBadgeToggle", settings.appearance.show_zoom_badge),
        (
            "backgroundThrottlingToggle",
            settings.performance.background_throttling,
        ),
        (
            "featureFindInPageToggle",
            settings.features.enable_find_in_page,
        ),
        (
            "featureDownloadsToggle",
            settings.features.enable_downloads_manager,
        ),
        (
            "featureLinkPreviewToggle",
            settings.features.enable_link_preview
                && settings.appearance.enable_link_preview
                && settings.appearance.show_status_preview,
        ),
        (
            "featureTabGesturesToggle",
            settings.features.enable_tab_gestures,
        ),
        (
            "featureZoomControlsToggle",
            settings.features.enable_zoom_controls,
        ),
        (
            "featurePermissionsToggle",
            settings.features.enable_permissions_prompt,
        ),
    ] {
        if !enabled {
            let start = html
                .find(&format!("id=\"{id}\""))
                .expect("settings control exists");
            let end = start + html[start..].find('>').expect("settings input closes");
            let input = html[start..end].replace(" checked", "");
            html.replace_range(start..end, &input);
        }
    }

    html = html
        .replace(
            "{{BACKGROUND_TAB_SLEEP_ENABLED}}",
            if settings.performance.background_throttling {
                "checked"
            } else {
                ""
            },
        )
        .replace(
            "{{SLEEP_DELAY_DISABLED}}",
            if settings.performance.background_throttling {
                ""
            } else {
                "disabled"
            },
        )
        .replace(
            &format!("value=\"{}\"", settings.performance.sleep_after_secs),
            &format!(
                "value=\"{}\" selected",
                settings.performance.sleep_after_secs
            ),
        );

    // Zoom level
    let zoom_id = if (settings.appearance.default_zoom_level - 0.75).abs() < 0.01 {
        "opt_zoom_75"
    } else if (settings.appearance.default_zoom_level - 0.9).abs() < 0.01 {
        "opt_zoom_90"
    } else if (settings.appearance.default_zoom_level - 1.1).abs() < 0.01 {
        "opt_zoom_110"
    } else if (settings.appearance.default_zoom_level - 1.25).abs() < 0.01 {
        "opt_zoom_125"
    } else if (settings.appearance.default_zoom_level - 1.5).abs() < 0.01 {
        "opt_zoom_150"
    } else {
        "opt_zoom_100"
    };
    html = html.replace(
        &format!("id=\"{}\"", zoom_id),
        &format!("id=\"{}\" selected", zoom_id),
    );

    // Site permissions
    let perms = [
        ("location", &settings.permissions.location),
        ("camera", &settings.permissions.camera),
        ("microphone", &settings.permissions.microphone),
        ("notifications", &settings.permissions.notifications),
    ];
    for (perm_name, perm_val) in perms {
        let val_lower = perm_val.to_lowercase();
        let target_id = format!("opt_perm_{}_{}", perm_name, val_lower);
        html = html.replace(
            &format!("id=\"{}\"", target_id),
            &format!("id=\"{}\" selected", target_id),
        );
    }

    html
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_settings_are_available_without_exposing_protected_profile_credentials() {
        let mut settings = Settings::default();
        settings.profiles[0].sync_credentials_protected = Some("local-dpapi-secret-marker".into());
        let html = get_settings_html("155.0", &settings, false, false, None, false, false);
        assert!(html.contains("EnableBookmarkSync"));
        assert!(html.contains("ImportBookmarkSync"));
        assert!(html.contains("SyncBookmarks"));
        assert!(html.contains("\"syncEnabled\":true"));
        assert!(html.contains("DeleteProfile"));
        assert!(html.contains("GetSitePermissions"));
        assert!(html.contains("ResetSitePermission"));
        assert!(html.contains("GetSavedPasswords"));
        assert!(html.contains("CheckAppUpdates"));
        assert!(html.contains("UpdateAdblockFilters"));
        assert!(html.contains("Cập nhật bộ lọc"));
        assert!(html.contains("Kiểm tra phiên bản"));
        assert!(html.contains("SelectBrowserExtensionFolder"));
        assert!(html.contains("SetBrowserExtensionEnabled"));
        assert!(html.contains("RemoveBrowserExtension"));
        assert!(html.contains("browserExtensionsList"));
        assert!(html.contains("SavePasswordForTab"));
        assert!(html.contains("passwordSaveTargetTab"));
        assert!(!html.contains("passwordOrigin"));
        assert!(html.contains("RevealPassword"));
        assert!(html.contains("FillPassword"));
        assert!(html.contains("DeletePassword"));
        assert!(html.contains("Kho mật khẩu Pip"));
        assert!(html.contains("id=\"passwordAutosaveToggle\""));
        assert!(html.contains("SetPasswordAutosave"));
        assert!(html.contains("id=\"autofillToggle\""));
        assert!(html.contains("Không ảnh hưởng lời nhắc mật khẩu ở trên"));
        assert!(html.contains("kho WebView2 riêng, không phải kho Pip"));
        assert!(html.contains("passwordAutosaveToggle').disabled=isPrivateProfile"));
        assert!(html.contains("isPrivateProfile"));
        assert!(html.contains("clearDataHistory"));
        assert!(html.contains("clearDataSite"));
        assert!(html.contains("clearDataCache"));
        assert!(html.contains("clearDataPasswords"));
        assert!(html.contains("clearDataAutofill"));
        assert!(html.contains("clearDataDownloads"));
        assert!(html.contains("ClearSelectedBrowsingData"));
        assert!(html.contains("Ctrl+Shift+Delete"));
        assert!(html.contains("__focusClearData"));
        assert!(!html.contains("local-dpapi-secret-marker"));
    }

    #[test]
    fn sync_allowlisted_appearance_and_close_warning_are_editable_and_reflect_profile_values() {
        let mut settings = Settings::default();
        settings.appearance.theme = "dark".into();
        settings.appearance.density = "compact".into();
        settings.tabs.warn_on_close_tabs = false;
        let html = get_settings_html("155.0", &settings, false, false, None, false, false);

        assert!(html.contains("id=\"themeSelect\""));
        assert!(html.contains("<option value=\"dark\" selected>"));
        assert!(html.contains("id=\"densitySelect\""));
        assert!(html.contains("<option value=\"compact\" selected>"));
        assert!(html.contains("id=\"warnOnCloseTabsToggle\""));
        let start = html.find("id=\"warnOnCloseTabsToggle\"").unwrap();
        let input = &html[start..html[start..].find('>').unwrap() + start];
        assert!(!input.contains(" checked"));
        assert!(html.contains("onAppearanceSelectChange('theme',this.value)"));
        assert!(html.contains("warn_on_close_tabs"));
    }

    #[test]
    fn extension_install_control_discloses_missing_webview2_action_ui() {
        let html = get_settings_html(
            "155.0",
            &Settings::default(),
            false,
            false,
            None,
            false,
            false,
        );
        let warning = html
            .find("id=\"browserExtensionsCompatibilityHint\"")
            .expect("extension compatibility warning is shown");
        let install = html
            .find("id=\"addBrowserExtensionButton\"")
            .expect("extension install control is shown");
        let adjacent_content = &html[warning..install];

        assert!(
            warning < install,
            "warning should appear beside install control"
        );
        assert!(adjacent_content.contains("biểu tượng/badge hoặc mở popup tiện ích"));
        assert!(adjacent_content.contains("mục chuột phải do tiện ích khai báo"));
        assert!(adjacent_content.contains("có thể không dùng được"));
        assert!(adjacent_content.contains("script nền/nội dung vẫn có thể hoạt động"));
        assert!(adjacent_content.contains("permissions"));
        assert!(adjacent_content.contains("host_permissions"));
        assert_eq!(
            html[html[..warning].rfind("<div class=\"row\">").unwrap()..install]
                .matches("<div class=\"row\">")
                .count(),
            1,
            "compatibility warning should share the install row"
        );
    }

    #[test]
    fn adblock_mode_selection_reflects_saved_setting() {
        let mut settings = Settings::default();
        settings.adblock_mode = evergreen_core::settings::AdblockMode::VideoFirst;
        let html = get_settings_html("155.0", &settings, false, false, None, false, false);
        assert!(html.contains("value=\"video_first\" selected"));
        assert!(html.contains("Ưu tiên video bỏ qua luật chặn"));
    }

    #[test]
    fn profile_deletion_explains_persistent_window_requirement() {
        let html = get_settings_html(
            "155.0",
            &Settings::default(),
            false,
            false,
            None,
            false,
            false,
        );
        assert!(html.contains("id=\"profileDeleteHint\""));
        assert!(html.contains("option.dataset.privateOpen=profile?.isPrivateOpen"));
        assert!(html.contains("Mở hồ sơ trong cửa sổ Pip bình thường trước khi xóa"));
        assert!(html.contains("cửa sổ ẩn danh không thể xóa dữ liệu hồ sơ"));
    }

    #[test]
    fn google_dns_status_explains_the_direct_webview_probe_limits() {
        let html = get_settings_html(
            "155.0",
            &Settings::default(),
            true,
            false,
            Some(true),
            false,
            false,
        );
        assert!(html.contains("Kiểm tra từ WebView2"));
        assert!(html.contains("window.__pipGoogleDnsWebViewProbe"));
        assert!(html.contains("mode: 'no-cors'"));
        assert!(html.contains("credentials: 'omit'"));
        assert!(html.contains("referrerPolicy: 'no-referrer'"));
        assert!(html.contains("kind: 'timeout'"));
        assert!(html.contains("kind: controller.signal.aborted ? 'timeout' : 'network'"));
        assert!(html.contains("no-cors ẩn mã HTTP"));
        assert!(html.contains("không bắt gói để xác minh từng truy vấn DNS"));
        assert!(html.contains(
            "Nếu cả truy vấn A và AAAA đều gặp lỗi tạm thời, Pip dùng DNS Windows dự phòng"
        ));
        assert!(html.contains("NXDOMAIN/NODATA hoặc không hợp lệ sẽ không fallback"));
        assert!(html.contains("Tra cứu trước từ WebView2 bị chặn cục bộ, ngoại trừ localhost và 127.0.0.1 dùng cho proxy"));
        assert!(html.contains("Kiểm tra proxy DoH nền thành công"));
        assert!(!html.contains("Google DNS đã được xác minh:"));
    }

    #[test]
    fn google_dns_startup_failure_explains_that_web_traffic_is_blocked_until_disabled() {
        let html = get_settings_html(
            "155.0",
            &Settings::default(),
            false,
            true,
            None,
            false,
            false,
        );
        assert!(html.contains("các trang theo tên miền đang bị chặn để tránh dùng DNS Windows"));
        assert!(html.contains("tắt Google DNS rồi khởi động lại nếu muốn dùng DNS Windows"));
        assert!(!html.contains("hiện tab vẫn dùng DNS Windows"));
    }

    #[test]
    fn google_dns_status_discloses_system_dns_fallback_and_resolution_failure() {
        let settings = Settings {
            google_dns: true,
            ..Settings::default()
        };
        let html = get_settings_html("155.0", &settings, true, false, Some(true), true, true);
        assert!(!html.contains("{{GOOGLE_DNS_FALLBACK_USED}}"));
        assert!(html.contains("đã dùng DNS Windows dự phòng cho một số tên miền"));
        assert!(html.contains("cả hai bộ phân giải cùng lỗi với ít nhất một yêu cầu"));
        assert!(html.contains("window.__renderGoogleDns(true,true,false,true,true,true)"));
    }
}
