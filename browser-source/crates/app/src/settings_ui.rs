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
      <details><summary>Chặn quảng cáo hoạt động thế nào?</summary><p>Brave Adblock Rust lọc yêu cầu mạng bằng bộ lọc tích hợp, kèm quy tắc ẩn quảng cáo và scriptlet cho YouTube. Nếu trang lỗi, thử tắt khiên cho trang đó rồi tải lại. Quảng cáo YouTube có thể thay đổi và vẫn xuất hiện; bộ lọc được cập nhật cùng bản Pip mới.</p></details>
      <details><summary>Hồ sơ và ẩn danh khác nhau thế nào?</summary><p>Hồ sơ giữ đăng nhập và dữ liệu website riêng trên thiết bị; chưa có đồng bộ đám mây. Ẩn danh không ghi lịch sử. Đóng toàn bộ cửa sổ ẩn danh của hồ sơ để kết thúc phiên; tệp tải và dấu trang chủ động lưu vẫn còn. Website và nhà mạng vẫn có thể nhận biết kết nối.</p></details>
      <details><summary>Các phím tắt thường dùng</summary><p>Ctrl+T: tab mới · Ctrl+Tab: tab kế · Ctrl+W: đóng tab · Ctrl+Shift+T: mở lại tab đã đóng · Ctrl+L: thanh địa chỉ · Ctrl+H: lịch sử · Ctrl+Shift+O: dấu trang · Ctrl+J: tải xuống · Ctrl+F: tìm chữ · Ctrl+Shift+N: ẩn danh. Xem danh sách đầy đủ trong Menu → Phím tắt.</p></details>
    </div>
    <div class="section-title">Bộ nhớ và dữ liệu</div>
    <div class="card help-card">
      <details><summary>Mở nhiều tab mà nhẹ hơn</summary><p>Tab được khôi phục từ phiên trước chỉ tải khi bạn chọn. Tab đang ẩn được WebView2 yêu cầu giảm bộ nhớ đệm; trang vẫn giữ nội dung khi chuyển tab. Đóng các tab không cần để giải phóng thêm RAM; ghim tab chỉ giữ vị trí, không làm tab nhẹ hơn. Pip không tự hủy trang đang nhập liệu hoặc cuộc gọi.</p></details>
      <details><summary>RAM, dung lượng đĩa và Internet</summary><p>RAM là bộ nhớ của các trang đang mở. Dung lượng đĩa gồm bộ nhớ đệm, cookie, dữ liệu website và tệp đã tải, có thể tăng theo cách dùng. Mỗi hồ sơ lưu một bộ dữ liệu riêng; chỉ tạo khi cần tách tài khoản. Pip dùng chung WebView2 của Microsoft. Không có trình duyệt nhiều tab nào sử dụng bằng không các tài nguyên này.</p></details>
      <div class="row"><div class="row-info"><span class="row-label">Dọn bộ nhớ đệm website</span><span class="row-desc">Xóa bản sao tạm của ảnh và tệp website thuộc hồ sơ hiện tại để thu hồi dung lượng đĩa. Giữ cookie đăng nhập, lịch sử, dấu trang và tệp tải. Website có thể tải chậm hơn trong lần mở kế tiếp.</span></div><button class="btn btn-secondary" id="clearCacheButton" onclick="sendAction('ClearCache')">Xóa bộ nhớ đệm</button></div>
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
    </div>

    <div class="section-title">Tính năng trình duyệt</div>
    <div class="card">
      <div class="row">
        <div class="row-info">
          <span class="row-label">Tìm trong trang</span>
          <span class="row-desc">Ctrl+F tìm chữ trong trang đang xem; Enter và Shift+Enter chuyển kết quả</span>
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
          <span class="row-desc">Ctrl+J mở tiến trình, hủy tải, mở tệp hoặc thư mục. Danh sách chỉ thuộc phiên của cửa sổ này</span>
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
    </div>

    <div class="section-title">Kết nối</div>
    <div class="card">
      <div class="row"><div class="row-info"><span class="row-label">Google DNS bảo mật</span><span class="row-desc" id="googleDnsHint">Tra cứu tên miền qua HTTPS tới Google trong Pip. Sau khi đổi, hãy khởi động lại để áp dụng.</span></div><input id="googleDnsToggle" type="checkbox" aria-label="Dùng Google DNS" onchange="profileAction('SetGoogleDns',{enabled:this.checked})"><button type="button" class="btn btn-secondary" onclick="sendAction('TestGoogleDns')">Kiểm tra tuyến đang dùng</button></div>
      <div class="row" id="googleDnsRestartRow" hidden><div class="row-info"><span class="row-label">Cần khởi động lại</span><span class="row-desc">Tab thường được lưu và khôi phục. Cửa sổ ẩn danh sẽ đóng để bảo vệ riêng tư.</span></div><button type="button" class="btn btn-secondary" onclick="if(confirm('Khởi động lại Pip để áp dụng Google DNS? Tab thường sẽ được lưu và khôi phục; cửa sổ ẩn danh sẽ đóng.')) sendAction('RestartBrowser')">Khởi động lại Pip</button></div>
    </div>

    <div class="section-title">Hồ sơ và riêng tư</div>
    <div class="card">
      <div class="row"><div class="row-info"><span class="row-label">Hồ sơ đang dùng</span><span class="row-desc">Cookie, đăng nhập và dữ liệu website được tách theo hồ sơ. Không đồng bộ tài khoản Chrome.</span></div><strong id="currentProfileName"></strong></div>
      <div class="row"><select id="profileSelect" title="Chọn hồ sơ để mở cửa sổ riêng hoặc đổi tên" aria-label="Hồ sơ"></select><button class="btn btn-secondary" onclick="profileAction('OpenProfile',{id:profileSelect.value})">Mở hồ sơ</button></div>
      <div class="row"><input id="profileName" class="text-input" maxlength="40" placeholder="Tên hồ sơ" aria-label="Tên hồ sơ"><button class="btn btn-secondary" onclick="profileAction('CreateProfile',{name:profileName.value})">Tạo hồ sơ</button><button class="btn btn-secondary" onclick="profileAction('RenameProfile',{id:profileSelect.value,name:profileName.value})">Đổi tên hồ sơ đã chọn</button></div>
      <div class="row"><div class="row-info"><span class="row-label">Cửa sổ ẩn danh</span><span class="row-desc">Không ghi lịch sử hoặc lưu phiên tab. Dữ liệu website của phiên ẩn danh được bỏ khi đóng toàn bộ cửa sổ ẩn danh của hồ sơ; tệp đã tải vẫn còn.</span></div><button class="btn btn-secondary" onclick="sendAction('OpenPrivateWindow')">Mở ẩn danh</button></div>
      <div class="row"><div class="row-info"><span class="row-label">Xóa cookie và dữ liệu website</span><span class="row-desc">Xóa cookie và dữ liệu website của hồ sơ hiện tại để đăng xuất; các trang đang mở có thể cần tải lại. Giữ dấu trang, lịch sử, mật khẩu và tệp tải xuống.</span></div><button class="btn btn-secondary" id="clearSiteDataButton" onclick="if(confirm('Xóa cookie và dữ liệu website của hồ sơ hiện tại? Bạn sẽ bị đăng xuất khỏi các website.')) sendAction('ClearSiteData')">Xóa dữ liệu website</button></div>
    </div>
    <div class="section-title">Mật khẩu và tự điền</div>
    <div class="card">
      <div class="row"><div class="row-info"><span class="row-label">Lưu mật khẩu và tự điền của WebView2</span><span class="row-desc">Áp dụng cho hồ sơ hiện tại, luôn tắt trong ẩn danh. Pip không có kho mật khẩu riêng.</span></div><input id="autofillToggle" type="checkbox" aria-label="Lưu mật khẩu và tự điền" onchange="profileAction('SetAutofill',{enabled:this.checked})"></div>
      <div class="row"><span>Xóa mật khẩu đã lưu của hồ sơ này</span><button class="btn btn-secondary" id="clearPasswordsButton" onclick="if(confirm('Xóa toàn bộ mật khẩu đã lưu trong hồ sơ này?')) sendAction('ClearSavedPasswords')">Xóa mật khẩu</button></div>
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
      document.getElementById('currentProfileName').textContent=current?.name || currentProfileId;
      document.getElementById('autofillToggle').checked=!isPrivateProfile && !!current?.autofill;
      document.getElementById('autofillToggle').disabled=isPrivateProfile;
      document.getElementById('clearPasswordsButton').disabled=isPrivateProfile;
      document.getElementById('clearCacheButton').disabled=isPrivateProfile;
    };
    window.__renderProfiles({{PROFILES_JSON}});
    window.__renderGoogleDns = (enabled, active=enabled, failed=false) => {
      document.getElementById('googleDnsToggle').checked=enabled;
      document.getElementById('googleDnsRestartRow').hidden=enabled===active;
      document.getElementById('googleDnsHint').textContent=failed&&enabled&&!active
        ? 'Pip không khởi động được proxy DNS riêng; hiện tab vẫn dùng DNS Windows. Kiểm tra kết nối rồi thử khởi động lại Pip.'
        : enabled!==active
          ? 'Đã lưu lựa chọn nhưng tuyến chưa đổi. Nhấn “Khởi động lại Pip” để áp dụng, sau đó kiểm tra lại.'
          : active
            ? 'Google DNS đang chạy qua proxy HTTPS riêng trong Pip. Kiểm tra sẽ xác nhận tra cứu tên miền và tải trang qua đúng tuyến. DNS Windows không đổi; địa chỉ IP không được ẩn.'
            : 'Đang dùng DNS Windows. Bật Google DNS để dùng tuyến HTTPS riêng cho kết nối web của Pip.';
    };
    window.__renderGoogleDns({{GOOGLE_DNS_ENABLED}},{{GOOGLE_DNS_ACTIVE}},{{GOOGLE_DNS_FAILED}});

    function sendAction(action) {
      if (window.ipc) {
        window.ipc.postMessage(JSON.stringify({ action: action, payload: null }));
        showStatus('Đang xử lý...');
      } else {
        console.log('[IPC OUT]', action);
        showStatus('Mở trang này trong Pip để sử dụng cài đặt.');
      }
    }

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
        if (setting === 'askWhereToSave' || setting === 'showProgressToolbar') {
          payloadObj = { downloads: { [setting === 'askWhereToSave' ? 'ask_where_to_save' : 'show_progress_toolbar']: value } };
        } else if (setting === 'tabReordering' || setting === 'tabTearoff') {
          payloadObj = { tabs: { [setting === 'tabReordering' ? 'enable_tab_reordering' : 'enable_tab_tearoff']: value } };
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

    function onDefaultZoomChange(zoom) {
      if (window.ipc) {
        window.ipc.postMessage(JSON.stringify({
          action: 'SaveSettings',
          payload: { settings_json: JSON.stringify({ default_zoom_level: parseFloat(zoom) }) }
        }));
        showStatus('Đã chọn tỷ lệ trang ' + Math.round(parseFloat(zoom) * 100) + '%');
      }
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
) -> String {
    let download_dir_str = settings.downloads.default_folder.to_string_lossy();
    let mut html = SETTINGS_TEMPLATE
        .replace("{{LOGO_BASE64}}", LOGO_BASE64.trim())
        .replace("{{MASCOT_BASE64}}", MASCOT_BASE64.trim())
        .replace(
            "{{PROFILES_JSON}}",
            &serde_json::to_string(&settings.profiles)
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
        );

    let engine = settings.search_engine.to_lowercase();
    html = html.replace(
        &format!("id=\"opt_{}\"", engine),
        &format!("id=\"opt_{}\" selected", engine),
    );

    for (id, enabled) in [
        ("askWhereToSaveToggle", settings.downloads.ask_where_to_save),
        (
            "showProgressToolbarToggle",
            settings.downloads.show_progress_toolbar,
        ),
        ("tabReorderingToggle", settings.tabs.enable_tab_reordering),
        ("tabTearoffToggle", settings.tabs.enable_tab_tearoff),
        ("showZoomBadgeToggle", settings.appearance.show_zoom_badge),
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
