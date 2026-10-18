//! Sidebar / Side-Panel UI for Evergreen Browser
//! Provides modern WinUI 3 Fluent slide-out panel for both:
//! 1) 3-Dot Menu trình duyệt (New Tab, New Window, DevTools, Settings)
//! 2) Thông tin trang & Certificate Inspector (HTTPS status, encryption, ephemeral storage)

use std::sync::LazyLock;

pub const LOGO_BASE64: &str = include_str!("../ui/logo.b64");
pub const MASCOT_BASE64: &str = include_str!("../ui/assets/mascot-clean.webp.b64");
pub const DOWNLOAD_MASCOT_BASE64: &str = include_str!("../ui/assets/mascot-download.webp.b64");

pub const SIDEBAR_TEMPLATE: &str = r#"<!DOCTYPE html>
<html lang="vi">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>Pip · Menu</title>
  <link rel="icon" type="image/png" href="data:image/png;base64,{{LOGO_BASE64}}">
  <style>
    :root {
      color-scheme: dark;
      --bg-panel: #1e1e26;
      --bg-card: #272733;
      --bg-card-hover: #313142;
      --border-subtle: rgba(255, 255, 255, 0.08);
      --border-focus: #4e8cff;
      --text-main: #f3f3f8;
      --text-muted: #9595a6;
      --accent-green: #34d399;
      --accent-amber: #f59e0b;
      --accent-blue: #4e8cff;
      --danger-color: #ef4444;
      --font-family: "Segoe UI Variable Text", "Segoe UI", system-ui, -apple-system, sans-serif;
    }

    * {
      box-sizing: border-box;
      margin: 0;
      padding: 0;
      user-select: none;
    }

    body {
      background-color: var(--bg-panel);
      color: var(--text-main);
      font-family: var(--font-family);
      font-size: 13px;
      height: 100vh;
      display: flex;
      flex-direction: column;
      border-left: 1px solid var(--border-subtle);
      overflow-y: auto;
      scrollbar-width: thin;
      scrollbar-color: rgba(255, 255, 255, 0.2) transparent;
    }

    svg {
      stroke: currentColor;
      stroke-width: 2;
      stroke-linecap: round;
      stroke-linejoin: round;
      fill: none;
      flex-shrink: 0;
    }

    /* Panel Header */
    .panel-header {
      height: 52px;
      display: flex;
      align-items: center;
      justify-content: space-between;
      padding: 0 16px;
      border-bottom: 1px solid var(--border-subtle);
      background: rgba(30, 30, 38, 0.95);
      position: sticky;
      top: 0;
      z-index: 10;
    }

    .header-title-group {
      display: flex;
      align-items: center;
      gap: 10px;
    }

    .header-icon {
      width: 20px;
      height: 20px;
      color: var(--accent-blue);
    }

    .header-title {
      font-size: 14px;
      font-weight: 600;
      letter-spacing: -0.2px;
    }

    .btn-close {
      width: 28px;
      height: 28px;
      display: flex;
      align-items: center;
      justify-content: center;
      background: transparent;
      border: none;
      color: var(--text-muted);
      border-radius: 6px;
      cursor: pointer;

    }

    .btn-close:hover {
      background: rgba(255, 255, 255, 0.1);
      color: var(--text-main);
    }

    .btn-close svg {
      width: 16px;
      height: 16px;
    }

    /* Mode Navigation Tabs */
    .mode-nav {
      display: flex;
      overflow-x: auto;
      padding: 8px 16px 0;
      gap: 6px;
      border-bottom: 1px solid var(--border-subtle);
    }

    .mode-tab {
      padding: 6px 12px;
      font-size: 12px;
      font-weight: 500;
      color: var(--text-muted);
      background: transparent;
      border: none;
      border-bottom: 2px solid transparent;
      cursor: pointer;
      white-space: nowrap;

    }

    .mode-tab.active {
      color: var(--text-main);
      border-bottom-color: var(--accent-blue);
    }

    .mode-tab:hover:not(.active) {
      color: var(--text-main);
    }

    /* Content Area */
    .panel-content {
      padding: 16px;
      display: flex;
      flex-direction: column;
      gap: 14px;
      flex: 1;
    }

    .view-section {
      display: none;
      flex-direction: column;
      gap: 14px;
    }

    .view-section.active {
      display: flex;
    }

    /* Card Containers */
    .card {
      background: var(--bg-card);
      border: 1px solid var(--border-subtle);
      border-radius: 8px;
      padding: 12px 14px;
      display: flex;
      flex-direction: column;
      gap: 8px;
    }

    .card-title {
      font-size: 11px;
      text-transform: uppercase;
      letter-spacing: 0.5px;
      color: var(--text-muted);
      font-weight: 600;
    }

    /* Action List Items (Menu Mode) */
    .action-list {
      display: flex;
      flex-direction: column;
      gap: 4px;
    }

    .action-item {
      width: 100%;
      border: 0;
      background: transparent;
      font: inherit;
      text-align: left;
      display: flex;
      align-items: center;
      gap: 12px;
      padding: 9px 12px;
      border-radius: 6px;
      color: var(--text-main);
      cursor: pointer;

    }

    .action-item:hover {
      background: var(--bg-card-hover);
    }

    .action-item svg {
      width: 16px;
      height: 16px;
      color: var(--text-muted);
    }

    .action-item:hover svg {
      color: var(--text-main);
    }

      .action-label {
      flex: 1;
      font-size: 13px;
      min-width: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    .action-shortcut {
      font-size: 10px;
      color: var(--text-muted);
      flex: 0 0 auto;
      white-space: nowrap;
    }

    .action-divider {
      height: 1px;
      background: var(--border-subtle);
      margin: 4px 0;
    }

    .zoom-action-row {
      display: flex;
      align-items: center;
      justify-content: space-between;
      padding: 6px 12px;
      height: 36px;
    }

    .zoom-controls {
      display: flex;
      align-items: center;
      gap: 6px;
    }

    .zoom-btn {
      width: 24px;
      height: 24px;
      display: flex;
      align-items: center;
      justify-content: center;
      background: var(--bg-card);
      border: 1px solid var(--border-subtle);
      border-radius: 6px;
      color: var(--text-main);
      cursor: pointer;
      font-size: 13px;
      font-weight: 600;

    }

    .zoom-btn:hover {
      background: var(--bg-card-hover);
    }

    .zoom-val {
      font-size: 11px;
      font-weight: 500;
      color: var(--text-muted);
      min-width: 38px;
      text-align: center;
      cursor: pointer;
      padding: 2px 4px;
      border-radius: 4px;

    }

    .zoom-val:hover {
      background: var(--bg-card-hover);
      color: var(--text-main);
    }

    /* Security Status Banner */
    .security-status-card {
      padding: 14px;
      border-radius: 8px;
      display: flex;
      flex-direction: column;
      gap: 6px;
    }

    .security-status-card.secure {
      background: rgba(52, 211, 153, 0.1);
      border: 1px solid rgba(52, 211, 153, 0.25);
    }

    .security-status-card.insecure {
      background: rgba(245, 158, 11, 0.1);
      border: 1px solid rgba(245, 158, 11, 0.25);
    }

    .security-status-card.internal {
      background: rgba(78, 140, 255, 0.1);
      border: 1px solid rgba(78, 140, 255, 0.25);
    }

    .status-badge-row {
      display: flex;
      align-items: center;
      gap: 8px;
      font-weight: 600;
      font-size: 13px;
    }

    .security-status-card.secure .status-badge-row { color: var(--accent-green); }
    .security-status-card.insecure .status-badge-row { color: var(--accent-amber); }
    .security-status-card.internal .status-badge-row { color: var(--accent-blue); }

    .status-desc {
      font-size: 12px;
      color: var(--text-muted);
      line-height: 1.4;
    }

    :root{color-scheme:light;--bg-panel:#fffaf6;--bg-card:#fff;--bg-card-hover:#fff0e5;--border-subtle:#f0e2d7;--border-focus:#f97316;--text-main:#17202e;--text-muted:#687589;--accent-green:#16a34a;--accent-amber:#c66a12;--accent-blue:#f97316;--danger-color:#dc2626}
    body{border-left-color:var(--border-subtle)}.panel-header{background:#fff;border-bottom-color:var(--border-subtle)}.mode-nav{border-bottom-color:var(--border-subtle)}.mode-tab.active{color:#b83d08;border-bottom-color:#f97316}.card{border-radius:12px;box-shadow:0 4px 16px rgba(98,57,28,.045)}.action-item:hover{color:#b83d08}.privacy-pill{color:#15803d!important;background:#ecfdf3!important;border-color:#bbf7d0!important}
    .pip-welcome{padding:13px;margin:12px 16px 0;border:1px solid #f1e2d5;border-radius:13px;background:linear-gradient(110deg,#fff 0%,#fff 68%,#fff0e5 100%);display:flex;align-items:center;gap:12px}.pip-welcome img{width:53px;height:53px;object-fit:contain}.pip-welcome b{display:block;font-size:13px;color:#17202e}.pip-welcome span{display:block;margin-top:2px;color:#687589;font-size:11px}

    /* Property Row */
    .prop-row {
      display: flex;
      justify-content: space-between;
      align-items: center;
      padding: 4px 0;
      font-size: 12px;
    }

    .prop-label {
      color: var(--text-muted);
    }

    #viewShortcuts .prop-row { gap: 6px; }
    #viewShortcuts .prop-label { flex: 1; }
    #viewShortcuts kbd {
      flex: 0 0 auto;
      padding: 2px 5px;
      border: 1px solid var(--border-subtle);
      border-radius: 5px;
      background: var(--bg-panel);
      color: var(--text-main);
      font: 10px ui-monospace, Consolas, monospace;
      white-space: nowrap;
    }

    .prop-value {
      font-weight: 500;
      color: var(--text-main);
      text-align: right;
      max-width: 180px;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    .btn-cert-details {
      width: 100%;
      margin-top: 10px;
      padding: 8px 12px;
      background: var(--bg-card-hover);
      border: 1px solid var(--border-subtle);
      border-radius: 6px;
      color: var(--text-main);
      font-size: 12px;
      font-family: inherit;
      font-weight: 500;
      display: flex;
      align-items: center;
      justify-content: center;
      gap: 8px;
      cursor: pointer;

    }

    .btn-cert-details:hover {
      background: rgba(255, 255, 255, 0.1);
      border-color: var(--border-focus);
    }

    .btn-cert-details:active {
      transform: scale(0.98);
    }

    /* Privacy Guarantee Pill */
    .privacy-pill {
      display: inline-flex;
      align-items: center;
      gap: 6px;
      background: rgba(52, 211, 153, 0.1);
      border: 1px solid rgba(52, 211, 153, 0.25);
      color: var(--accent-green);
      font-size: 11px;
      font-weight: 500;
      padding: 4px 10px;
      border-radius: 12px;
      width: fit-content;
    }

    button:focus-visible,[role="button"]:focus-visible{outline:2px solid #f97316;outline-offset:2px}
    @media(prefers-color-scheme:dark){
      :root{color-scheme:dark;--bg-panel:#171719;--bg-card:#222327;--bg-card-hover:#2b2c31;--border-subtle:#39383a;--border-focus:#fb923c;--text-main:#f5f2ef;--text-muted:#c1b8b0;--accent-green:#4ade80;--accent-amber:#fbbf24;--accent-blue:#fb923c;--danger-color:#f87171}
      body{background:#171719}.panel-header{background:#222327;border-color:#39383a}.mode-nav{border-color:#39383a}.mode-tab:hover:not(.active){color:#fdba74}.card{background:#222327;border-color:#39383a}.action-item:hover{background:#3a291f;color:#fdba74}.action-item:hover svg{color:#fdba74}.pip-welcome{background:linear-gradient(110deg,#222327 0%,#222327 68%,#3a291f 100%);border-color:#493629}.pip-welcome b{color:#f5f2ef}.pip-welcome span{color:#c1b8b0}.security-status-card.secure{background:#183523;border-color:#275a38}.security-status-card.insecure{background:#392d17;border-color:#6b5125}.security-status-card.internal{background:#3a291f;border-color:#704426}.privacy-pill{color:#86efac!important;background:#183523!important;border-color:#275a38!important}.btn-close:hover,.zoom-btn:hover,.btn-cert-details:hover{background:#3a291f;color:#fdba74}
    }

    @media(max-width:460px){
      .panel-header{padding-inline:12px}
      .mode-nav{min-width:0;padding:8px 8px 0;overflow-x:auto;scrollbar-width:none}
      .mode-nav::-webkit-scrollbar{display:none}
      .mode-tab{flex:0 0 auto;padding-inline:9px}
      .panel-content,.view-section{min-width:0;width:100%}
      .panel-content{padding:12px}
      .pip-welcome{min-width:0;margin:0}
      .pip-welcome>span{min-width:0}
      .card.action-list{min-width:0;padding-inline:8px}
      .action-item{min-width:0;gap:8px;padding-inline:8px}
      .action-shortcut{font-size:9px}
    }
    @media(max-width:360px){
      .panel-content{padding:10px}
      .mode-nav{gap:2px}
      .mode-tab{padding-inline:7px;font-size:11px}
      .action-shortcut{display:none}
    }

    *,*::before,*::after { animation:none!important; transition:none!important; scroll-behavior:auto!important; backdrop-filter:none!important; }
</style>
</head>
<body>
  <!-- Header -->
  <div class="panel-header">
    <div class="header-title-group">
      <svg class="header-icon" id="panelHeaderIcon" viewBox="0 0 24 24">
        <circle cx="12" cy="12" r="1.5"/>
        <circle cx="12" cy="5" r="1.5"/>
        <circle cx="12" cy="19" r="1.5"/>
      </svg>
      <span class="header-title" id="panelHeaderTitle">Menu Pip</span>
    </div>
    <button class="btn-close" onclick="sendAction('CloseSidebar')" title="Đóng bảng" aria-label="Đóng">
      <svg viewBox="0 0 24 24"><path d="M18 6L6 18M6 6l12 12"/></svg>
    </button>
  </div>

  <!-- Mode Switcher Tabs -->
  <div class="mode-nav">
    <button class="mode-tab active" id="tabMenu" onclick="switchMode('menu')">Menu trình duyệt</button>
    <button class="mode-tab" id="tabSecurity" onclick="switchMode('security')">Thông tin trang</button>
    <button class="mode-tab" id="tabDownloads" onclick="switchMode('downloads')">Tải xuống</button>
    <button class="mode-tab" id="tabShortcuts" onclick="switchMode('shortcuts')">Phím tắt</button>
  </div>

  <div class="panel-content">
    <!-- VIEW 1: Menu trình duyệt Mode -->
    <div class="view-section active" id="viewMenu">
      <button class="pip-welcome" type="button" onclick="sendAction('OpenSettings')" style="text-align:left;cursor:pointer"><img src="data:image/webp;base64,{{MASCOT_BASE64}}" alt=""><span><b>Hồ sơ của bạn</b><span>Cá nhân hóa Pip trên thiết bị này</span></span></button>
      <p class="status-desc" style="margin:0 0 12px">Rê chuột vào chức năng để xem giải thích. Các lựa chọn chi tiết nằm trong Cài đặt.</p>
      <div class="card action-list">
        <button type="button" class="action-item" title="Mở trang tab mới trong cửa sổ hiện tại." onclick="sendAction('CreateTab')">
          <svg viewBox="0 0 24 24"><path d="M12 5v14M5 12h14"/></svg>
          <span class="action-label">Tab mới</span>
          <span class="action-shortcut">Ctrl+T</span>
        </button>
        <button type="button" class="action-item" title="Mở cửa sổ khác dùng chung hồ sơ hiện tại." onclick="sendAction('OpenNewWindow')">
          <svg viewBox="0 0 24 24"><rect x="3" y="3" width="18" height="18" rx="2"/><path d="M3 9h18"/></svg>
          <span class="action-label">Cửa sổ mới</span>
          <span class="action-shortcut">Ctrl+N</span>
        </button>
        <button type="button" class="action-item" title="Mở cửa sổ không ghi lịch sử; tệp tải xuống vẫn được giữ." onclick="sendAction('OpenPrivateWindow')"><svg viewBox="0 0 24 24"><path d="M3 12h3m12 0h3M6 12a4 4 0 1 1 8 0m4 0a4 4 0 1 0-8 0"/><path d="M2 8h20"/></svg><span class="action-label">Cửa sổ ẩn danh</span><span class="action-shortcut">Ctrl+Shift+N</span></button>
        <button type="button" class="action-item" title="Mở một tab mới từ địa chỉ hiện tại; nội dung đang nhập không được sao chép." onclick="sendAction('DuplicateTab')"><svg viewBox="0 0 24 24"><rect x="8" y="8" width="13" height="13" rx="2"/><path d="M16 8V5a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h3"/></svg><span class="action-label">Nhân đôi tab</span></button>
        <button type="button" class="action-item" title="Giữ tab ở đầu thanh tab. Ghim không giảm bộ nhớ của website." onclick="sendAction('TogglePinTab')"><svg viewBox="0 0 24 24"><path d="M8 4h8l-1 6 3 3v1h-5l-1 6-1-6H6v-1l3-3z"/></svg><span class="action-label">Ghim / bỏ ghim tab</span></button>
        <button type="button" class="action-item" id="keepAwakeTabButton" aria-pressed="false" title="Giữ tab hiện tại hoạt động thay vì tự ngủ. Dùng cho cuộc gọi hoặc tác vụ nền." onclick="sendAction('ToggleKeepAwakeTab')"><svg viewBox="0 0 24 24"><path d="M12 3 5 6v5c0 4.4 2.9 8.1 7 10 4.1-1.9 7-5.6 7-10V6z"/><path d="m9 12 2 2 4-4"/></svg><span class="action-label" id="keepAwakeTabLabel">Giữ tab này thức</span></button>
        <button type="button" class="action-item" title="Bật hoặc tắt âm thanh của tab đang chọn." onclick="sendAction('ToggleMuteTab')"><svg viewBox="0 0 24 24"><path d="M4 10v4h4l5 4V6l-5 4zM17 9l4 6m0-6-4 6"/></svg><span class="action-label">Tắt / bật tiếng tab</span></button>
        <button type="button" class="action-item" title="Mở lại địa chỉ của tab vừa đóng; nội dung chưa lưu không được khôi phục." onclick="sendAction('ReopenClosedTab')"><svg viewBox="0 0 24 24"><path d="M3 12a9 9 0 1 0 2.6-6.4L3 8"/><path d="M3 3v5h5m4-1v5l3 2"/></svg><span class="action-label">Mở lại tab đã đóng</span><span class="action-shortcut">Ctrl+Shift+T</span></button>
        <button type="button" class="action-item" title="Mở lại địa chỉ các tab đã lưu của hồ sơ. Tab chỉ tải khi bạn chọn." onclick="sendAction('RestoreSession')"><svg viewBox="0 0 24 24"><rect x="4" y="4" width="16" height="13" rx="2"/><path d="M8 21h8m-4-4v4m-4-9 3-3 3 3m-3-3v7"/></svg><span class="action-label">Mở lại phiên đã lưu</span></button>
        <button type="button" class="action-item" onclick="sendAction('Reload')">
          <svg viewBox="0 0 24 24">
            <path d="M21 2v6h-6"/>
            <path d="M3 12a9 9 0 0 1 15-6.7L21 8"/>
            <path d="M3 22v-6h6"/>
            <path d="M21 12a9 9 0 0 1-15 6.7L3 16"/>
          </svg>
          <span class="action-label">Tải lại trang</span>
          <span class="action-shortcut">Ctrl+R</span>
        </button>
        <div class="action-divider"></div>
        <!-- Zoom Controls -->
        <div class="zoom-action-row">
          <span class="action-label">Thu phóng</span>
          <div class="zoom-controls">
            <button class="zoom-btn" id="sidebarZoomOut" onclick="sendAction('ZoomOut')" title="Thu nhỏ (Ctrl+-)">−</button>
            <span class="zoom-val" id="sidebarZoomVal" onclick="sendAction('ZoomReset')" title="Đặt lại (Ctrl+0)">100%</span>
            <button class="zoom-btn" id="sidebarZoomIn" onclick="sendAction('ZoomIn')" title="Phóng to (Ctrl++)">+</button>
          </div>
        </div>
        <button type="button" class="action-item" title="Xem các trang bạn đã lưu bằng nút ngôi sao (Ctrl+D)." onclick="postIpc('OpenLibrary',{view:'bookmarks'})"><svg viewBox="0 0 24 24"><path d="M6 4h12v17l-6-4-6 4z"/></svg><span class="action-label">Dấu trang</span><span class="action-shortcut">Ctrl+Shift+O</span></button>
        <button type="button" class="action-item" title="Xem tối đa 500 trang gần nhất của hồ sơ; không ghi trong ẩn danh." onclick="postIpc('OpenLibrary',{view:'history'})"><svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/></svg><span class="action-label">Lịch sử</span><span class="action-shortcut">Ctrl+H</span></button>
        <button type="button" class="action-item" title="Đổi chặn quảng cáo cho trang hiện tại; nhấn F5 sau khi đổi." onclick="sendAction('ToggleAdblock')"><svg viewBox="0 0 24 24"><path d="M12 3 4 6v6c0 5 8 9 8 9s8-4 8-9V6z"/><path d="m8 12 3 3 5-6"/></svg><span class="action-label">Chặn quảng cáo trên trang</span><span class="action-shortcut">Ctrl+Shift+A</span></button>
        <button type="button" class="action-item" title="Nếu video bị lỗi, xác nhận để tắt bộ lọc cho website hiện tại rồi tải lại trang. Nội dung chưa gửi có thể mất." onclick="if(confirm('Sửa lỗi phát video? Pip sẽ tắt chặn quảng cáo cho website hiện tại và tải lại trang. Website khác không đổi; nội dung chưa gửi trên trang này có thể mất.'))sendAction('RepairPlayback')"><svg viewBox="0 0 24 24"><path d="M12 3a9 9 0 1 0 8.5 6M12 7v5l3 2"/><path d="M15 3h6v6"/><path d="m21 3-6 6"/></svg><span class="action-label">Sửa lỗi phát video</span></button>
        <button type="button" class="action-item" onclick="sendAction('OpenFindInPage')">
          <svg viewBox="0 0 24 24"><circle cx="11" cy="11" r="8"/><path d="m21 21-4.35-4.35"/></svg>
          <span class="action-label">Tìm trong trang</span>
          <span class="action-shortcut">Ctrl+F</span>
        </button>
        <button type="button" class="action-item" title="Lưu trang hiện tại bằng hộp thoại Save As của WebView2." onclick="sendAction('SavePage')">
          <svg viewBox="0 0 24 24"><path d="M5 3h12l4 4v14H3V3z"/><path d="M7 3v6h10V3M7 21v-7h10v7"/></svg>
          <span class="action-label">Lưu trang thành…</span>
          <span class="action-shortcut">Ctrl+S</span>
        </button>
        <button type="button" class="action-item" title="Mở hộp thoại in trang hiện tại." onclick="sendAction('PrintPage')">
          <svg viewBox="0 0 24 24"><path d="M7 8V3h10v5M7 17H4V9h16v8h-3M7 14h10v7H7z"/><path d="M17 11h.01"/></svg>
          <span class="action-label">In trang</span>
          <span class="action-shortcut">Ctrl+P</span>
        </button>
        <button type="button" class="action-item" title="Dịch nội dung trang sang tiếng Việt bằng mô hình trên thiết bị; nhấn lại để khôi phục văn bản gốc." onclick="sendAction('TranslatePage')">
          <svg viewBox="0 0 24 24"><path d="M4 5h10M9 3v2m2 0c-.8 4-3.2 7-7 9m1-6c1.2 2.1 3.1 3.8 5.4 4.8M14 20l4.2-10 4.2 10m-7-3h5.6"/></svg>
          <span class="action-label">Dịch trang sang tiếng Việt</span>
        </button>
        <button type="button" class="action-item" title="Xem tiến trình và mở các tệp tải trong phiên của cửa sổ này." onclick="sendAction('OpenDownloads')">
          <svg viewBox="0 0 24 24"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4M7 10l5 5 5-5M12 15V3"/></svg>
          <span class="action-label">Tải xuống</span>
          <span class="action-shortcut">Ctrl+J</span>
        </button>
        <div class="action-divider"></div>
        <button type="button" class="action-item" title="Mở công cụ dành cho người phát triển website (F12)." onclick="sendAction('OpenDevTools')">
          <svg viewBox="0 0 24 24"><polyline points="16 18 22 12 16 6"/><polyline points="8 6 2 12 8 18"/></svg>
          <span class="action-label">Công cụ nhà phát triển</span>
          <span class="action-shortcut">F12 · Ctrl+Shift+J</span>
        </button>
        <button type="button" class="action-item" title="Mở cài đặt và hướng dẫn về hồ sơ, DNS, quyền riêng tư và tài nguyên." onclick="sendAction('OpenSettings')">
          <svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>
          <span class="action-label">Cài đặt</span>
        </button>
      </div>

      <!-- Ephemeral Mode Status Card -->
      <div class="card">
        <span class="card-title">Bảo vệ phiên</span>
        <div class="privacy-pill">
          <svg viewBox="0 0 24 24" style="width: 12px; height: 12px;"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/></svg>
          <span id="privacyState">Hồ sơ trên thiết bị</span>
        </div>
        <p class="status-desc" id="privacyDescription">Dữ liệu web được lưu riêng trong hồ sơ Pip của bạn.</p>
      </div>
    </div>

    <!-- VIEW 2: Site Security & Certificate Mode -->
    <div class="view-section" id="viewSecurity">
      <!-- Status Banner -->
      <div class="security-status-card secure" id="securityBanner">
        <div class="status-badge-row">
          <svg id="statusIcon" viewBox="0 0 24 24" style="width: 18px; height: 18px;">
            <rect x="3" y="11" width="18" height="11" rx="2" ry="2"/>
            <path d="M7 11V7a5 5 0 0 1 10 0v4"/>
          </svg>
          <span id="statusTitle">Kết nối an toàn</span>
        </div>
        <p class="status-desc" id="statusDesc">Trang sử dụng HTTPS. Pip giữ cơ chế kiểm tra chứng chỉ của WebView2.</p>
      </div>

      <!-- Certificate & Mã hóa Details -->
      <div class="card">
        <span class="card-title">Chứng chỉ và mã hóa</span>
        <div class="prop-row">
          <span class="prop-label">Máy chủ</span>
          <span class="prop-value" id="propHost">—</span>
        </div>
        <div class="prop-row">
          <span class="prop-label">Trạng thái</span>
          <span class="prop-value" id="propCert">—</span>
        </div>
        <div class="prop-row">
          <span class="prop-label">Cấp cho</span>
          <span class="prop-value" id="propSubject">—</span>
        </div>
        <div class="prop-row">
          <span class="prop-label">Đơn vị cấp</span>
          <span class="prop-value" id="propIssuer">—</span>
        </div>
        <div class="prop-row">
          <span class="prop-label">Thời hạn</span>
          <span class="prop-value" id="propValid">—</span>
        </div>
        <div class="prop-row">
          <span class="prop-label">Thuật toán</span>
          <span class="prop-value" id="propSigAlg">—</span>
        </div>
        <div class="prop-row">
          <span class="prop-label">Dấu vân tay</span>
          <span class="prop-value" id="propThumbprint" style="font-family: monospace; font-size: 11px;">—</span>
        </div>
        <div class="prop-row">
          <span class="prop-label">Giao thức</span>
          <span class="prop-value" id="propProtocol">—</span>
        </div>
        <div class="prop-row">
          <span class="prop-label">Mã hóa</span>
          <span class="prop-value" id="propCipher">—</span>
        </div>

        <button class="btn-cert-details" id="btnOpenCertDialog" onclick="openCertificateDialog()">
          <svg viewBox="0 0 24 24" style="width: 14px; height: 14px;"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/><line x1="16" y1="13" x2="8" y2="13"/><line x1="16" y1="17" x2="8" y2="17"/><polyline points="10 9 9 9 8 9"/></svg>
          Xem chứng chỉ máy chủ
        </button>
      </div>

      <div class="card"><span class="card-title">Quyền truy cập</span><button class="action-item" title="Mở cài đặt và hướng dẫn về hồ sơ, DNS, quyền riêng tư và tài nguyên." onclick="sendAction('OpenSettings')">Quản lý trong Cài đặt</button></div>
    </div>

    <!-- VIEW 3: Downloads Mode -->
    <div class="view-section" id="viewDownloads">
      <div style="display:flex;align-items:center;gap:12px;margin-bottom:14px"><img src="data:image/webp;base64,{{DOWNLOAD_MASCOT_BASE64}}" alt="" width="72" height="88" style="object-fit:contain"><p class="status-desc">Lịch sử tải xuống của hồ sơ này được giữ sau khi đóng Pip. Bấm Mở tệp hoặc Mở thư mục để tìm trên máy. Xóa danh sách vẫn giữ tệp đã tải và không hủy lượt đang chạy.</p></div>
      <div class="card" style="padding: 12px;">
        <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 10px;">
          <span class="card-title">Tải xuống</span>
          <button id="sidebarClearDownloadsBtn" onclick="clearDownloads()" style="background: transparent; border: 1px solid var(--border-subtle); color: var(--text-muted); font-size: 11px; padding: 3px 8px; border-radius: 4px; cursor: pointer;">Xóa mục đã xong</button>
        </div>
        <div id="sidebarDownloadsList" style="display: flex; flex-direction: column; gap: 8px; max-height: 520px; overflow-y: auto;">
          <div id="emptyDownloadsNotice" style="color: var(--text-muted); font-size: 12px; text-align: center; padding: 30px 0;">Chưa có tệp tải xuống</div>
        </div>
      </div>
    </div>

    <!-- VIEW 4: Keyboard shortcuts -->
    <div class="view-section" id="viewShortcuts">
      <div class="card"><span class="card-title">Tab và cửa sổ</span>
        <div class="prop-row"><span class="prop-label">Tab mới</span><kbd>Ctrl+T</kbd></div>
        <div class="prop-row"><span class="prop-label">Cửa sổ mới</span><kbd>Ctrl+N</kbd></div>
        <div class="prop-row"><span class="prop-label">Cửa sổ ẩn danh</span><kbd>Ctrl+Shift+N</kbd></div>
        <div class="prop-row"><span class="prop-label">Đóng tab</span><kbd>Ctrl+W</kbd></div>
        <div class="prop-row"><span class="prop-label">Đóng cửa sổ</span><kbd>Ctrl+Shift+W</kbd></div>
        <div class="prop-row"><span class="prop-label">Mở lại tab vừa đóng</span><kbd>Ctrl+Shift+T</kbd></div>
        <div class="prop-row"><span class="prop-label">Chuyển tab kế / trước</span><kbd>Ctrl+Tab</kbd> <kbd>Ctrl+Shift+Tab</kbd></div>
        <div class="prop-row"><span class="prop-label">Chuyển đến tab 1–8 / cuối</span><kbd>Ctrl+1…8</kbd> <kbd>Ctrl+9</kbd></div>
      </div>
      <div class="card"><span class="card-title">Trang và điều hướng</span>
        <div class="prop-row"><span class="prop-label">Thanh địa chỉ / tìm kiếm</span><kbd>Ctrl+L</kbd> <kbd>Ctrl+K</kbd></div>
        <div class="prop-row"><span class="prop-label">Tìm trong trang</span><kbd>Ctrl+F</kbd></div>
        <div class="prop-row"><span class="prop-label">Lịch sử</span><kbd>Ctrl+H</kbd></div>
        <div class="prop-row"><span class="prop-label">Xóa dữ liệu duyệt web</span><kbd>Ctrl+Shift+Delete</kbd></div>
        <div class="prop-row"><span class="prop-label">Quản lý dấu trang</span><kbd>Ctrl+Shift+O</kbd></div>
        <div class="prop-row"><span class="prop-label">Tải xuống</span><kbd>Ctrl+J</kbd></div>
        <div class="prop-row"><span class="prop-label">Thêm / bỏ dấu trang</span><kbd>Ctrl+D</kbd></div>
        <div class="prop-row"><span class="prop-label">In trang hiện tại</span><kbd>Ctrl+P</kbd></div>
        <div class="prop-row"><span class="prop-label">Lưu trang web</span><kbd>Ctrl+S</kbd></div>
        <div class="prop-row"><span class="prop-label">Quay lại / tiến tới</span><kbd>Alt+←</kbd> <kbd>Alt+→</kbd></div>
        <div class="prop-row"><span class="prop-label">Tải lại / tải lại bỏ bộ nhớ đệm</span><kbd>Ctrl+R</kbd> <kbd>Ctrl+Shift+R</kbd></div>
        <div class="prop-row"><span class="prop-label">Về trang chủ Pip</span><kbd>Alt+Home</kbd></div>
        <div class="prop-row"><span class="prop-label">Dừng tải trang</span><kbd>Esc</kbd></div>
      </div>
      <div class="card"><span class="card-title">Thu phóng và Pip</span>
        <div class="prop-row"><span class="prop-label">Phóng to / thu nhỏ / đặt lại</span><kbd>Ctrl++</kbd> <kbd>Ctrl+-</kbd> <kbd>Ctrl+0</kbd></div>
        <div class="prop-row"><span class="prop-label">Mở menu hồ sơ và chuyển tài khoản</span><kbd>Ctrl+Shift+M</kbd></div>
        <div class="prop-row"><span class="prop-label">Bật / tắt chặn quảng cáo cho trang</span><kbd>Ctrl+Shift+A</kbd></div>
        <div class="prop-row"><span class="prop-label">Điền mật khẩu đã lưu khi chỉ có một tài khoản khớp</span><kbd>Alt+Shift+P</kbd></div>
        <div class="prop-row"><span class="prop-label">Công cụ nhà phát triển</span><kbd>F12</kbd> <kbd>Ctrl+Shift+J</kbd></div>
        <p class="status-desc">Phím tắt áp dụng khi cửa sổ Pip đang hoạt động. Khi đang nhập trong ô văn bản, Pip giữ nguyên các tổ hợp chỉnh sửa thông dụng của trang.</p>
      </div>
    </div>
  </div>

  <script>
    let currentMode = 'menu';

    function postIpc(action, payload = null) {
      if (window.ipc) {
        window.ipc.postMessage(JSON.stringify({ action, payload }));
      } else {
        console.log('[SIDEBAR IPC]', action, payload);
      }
    }

    function sendAction(action, payload = null) {
      if (action === 'CreateTab') {
        postIpc('CreateTab', { url: null });
      } else if (action === 'OpenSettings') {
        postIpc('CloseSidebar');
        postIpc('OpenSettings');
      } else {
        postIpc(action, payload);
      }
    }

    window.__renderPrivacy = privateWindow => {
      document.getElementById('privacyState').textContent=privateWindow?'Cửa sổ ẩn danh':'Hồ sơ trên thiết bị';
      document.getElementById('privacyDescription').textContent=privateWindow?'Pip không lưu lịch sử của cửa sổ này. Tệp tải xuống và dấu trang bạn chủ động lưu vẫn được giữ.':'Cookie, lịch sử và dữ liệu web được lưu riêng trong hồ sơ Pip của bạn.';
    };
    const activeDownloads = new Map();

    function switchMode(mode) {
      currentMode = mode;
      const tabMenu = document.getElementById('tabMenu');
      const tabSec = document.getElementById('tabSecurity');
      const tabDl = document.getElementById('tabDownloads');
      const tabShortcuts = document.getElementById('tabShortcuts');
      const viewMenu = document.getElementById('viewMenu');
      const viewSec = document.getElementById('viewSecurity');
      const viewDl = document.getElementById('viewDownloads');
      const viewShortcuts = document.getElementById('viewShortcuts');
      const title = document.getElementById('panelHeaderTitle');
      const icon = document.getElementById('panelHeaderIcon');

      [tabMenu, tabSec, tabDl, tabShortcuts].forEach(t => t && t.classList.remove('active'));
      [viewMenu, viewSec, viewDl, viewShortcuts].forEach(v => v && v.classList.remove('active'));

      if (mode === 'security') {
        if (tabSec) tabSec.classList.add('active');
        if (viewSec) viewSec.classList.add('active');
        title.textContent = 'Thông tin trang';
        icon.innerHTML = '<rect x="3" y="11" width="18" height="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/>';
      } else if (mode === 'downloads') {
        if (tabDl) tabDl.classList.add('active');
        if (viewDl) viewDl.classList.add('active');
        title.textContent = 'Tải xuống';
        icon.innerHTML = '<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4M7 10l5 5 5-5M12 15V3"/>';
        renderSidebarDownloads();
      } else if (mode === 'shortcuts') {
        if (tabShortcuts) tabShortcuts.classList.add('active');
        if (viewShortcuts) viewShortcuts.classList.add('active');
        title.textContent = 'Phím tắt Pip';
        icon.innerHTML = '<path d="M4 5h16v14H4zM8 9h1m3 0h1m3 0h1M8 13h1m3 0h1m3 0h1M8 16h8"/>';
      } else {
        if (tabMenu) tabMenu.classList.add('active');
        if (viewMenu) viewMenu.classList.add('active');
        title.textContent = 'Menu Pip';
        icon.innerHTML = '<circle cx="12" cy="12" r="1.5"/><circle cx="12" cy="5" r="1.5"/><circle cx="12" cy="19" r="1.5"/>';
      }
    }

    window.__syncDownloads = function(dl) {
      if (Array.isArray(dl)) {
        activeDownloads.clear();dl.forEach(item => activeDownloads.set(item.download_id, item));
      } else if (dl && dl.download_id) {
        activeDownloads.set(dl.download_id, dl);
      }
      if (currentMode === 'downloads') renderSidebarDownloads();
    };

    function renderSidebarDownloads() {
      const list = document.getElementById('sidebarDownloadsList');
      if (!list) return;
      if (activeDownloads.size === 0) {
        list.innerHTML = '<div id="emptyDownloadsNotice" style="color: var(--text-muted); font-size: 12px; text-align: center; padding: 30px 0;">Chưa có tệp tải xuống</div>';
        return;
      }
      let html = '';
      for (const [id, item] of activeDownloads.entries()) {
        // Filenames come from websites; never interpolate them as privileged page markup.
        const filename = String(item.filename ?? '').replace(/[&<>"']/g, ch => ({'&':'&amp;', '<':'&lt;', '>':'&gt;', '"':'&quot;', "'":'&#39;'}[ch]));
        if (!Number.isSafeInteger(id) || id < 0) continue;
        const isDone = item.state === 'Completed';
        const pct = isDone ? 100 : (item.total_bytes > 0 ? Math.min(100, Math.round((item.received_bytes / item.total_bytes) * 100)) : 0);
        const mbReceived = (item.received_bytes / (1024 * 1024)).toFixed(1);
        const mbTotal = (item.total_bytes / (1024 * 1024)).toFixed(1);
        const barColor = isDone ? 'var(--accent-green)' : 'var(--accent-blue)';
        const statusHtml = isDone
          ? '<span style="color: var(--accent-green); font-weight: 500;">Hoàn tất</span>'
          : (item.state === 'Interrupted' ? '<span style="color: var(--danger-color);">Đã gián đoạn</span>' : `<span>${pct}% · Đang tải</span>`);
        const actionHtml = isDone
          ? `<button onclick="postIpc('OpenDownload',{download_id:${id},folder:true})" style="border:0;background:transparent;color:#b84b08;cursor:pointer">Mở thư mục</button><button onclick="postIpc('OpenDownload',{download_id:${id},folder:false})" style="border:0;border-radius:6px;background:#f97316;color:white;padding:4px 9px;cursor:pointer">Mở tệp</button>`
          : item.state === 'Interrupted'
            ? `<button onclick="postIpc('OpenDownload',{download_id:${id},folder:true})" style="border:0;background:transparent;color:#b84b08;cursor:pointer">Mở thư mục</button>`
            : `<button onclick="sendAction('CancelDownload', { download_id: ${id} })" style="background: transparent; border: none; color: var(--danger-color); cursor: pointer; font-size: 11px;">Hủy</button>`;
        html += `
          <div class="download-card" id="sidebar-dl-${id}" style="background: var(--bg-card); border: 1px solid var(--border-subtle); border-radius: 8px; padding: 10px; display: flex; flex-direction: column; gap: 6px;">

            <div style="display: flex; justify-content: space-between; font-size: 12px; font-weight: 500;">
              <span style="overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 180px;" title="${filename}">${filename}</span>
              <span style="color: var(--text-muted); font-size: 11px;">${mbReceived} / ${mbTotal} MB</span>
            </div>
            <div style="height: 4px; background: rgba(255,255,255,0.1); border-radius: 2px; overflow: hidden;">
              <div style="height: 100%; width: ${pct}%; background: ${barColor}; "></div>
            </div>
            <div style="display: flex; justify-content: space-between; font-size: 11px; color: var(--text-muted);">
              ${statusHtml}
              ${actionHtml}
            </div>
          </div>
        `;
      }
      list.innerHTML = html;
    }

    function clearDownloads() {
      postIpc('ClearDownloads');
    }

    function openCertificateDialog() {
      const host = document.getElementById('propHost').textContent;
      postIpc('OpenCertificateDialog', { host });
    }

    window.__setKeepAwake = function(enabled) {
      const button = document.getElementById('keepAwakeTabButton');
      const label = document.getElementById('keepAwakeTabLabel');
      if (!button || !label) return;
      button.setAttribute('aria-pressed', String(!!enabled));
      label.textContent = enabled ? 'Cho tab này tự ngủ' : 'Giữ tab này thức';
      button.title = enabled
        ? 'Tab hiện tại được giữ thức. Bấm để cho phép Pip tự ngủ tab này.'
        : 'Giữ tab hiện tại hoạt động thay vì tự ngủ. Dùng cho cuộc gọi hoặc tác vụ nền.';
    };

    // Called by host to update sidebar data
    window.__sidebarSync = function(msg) {
      if (!msg) return;
      const data = msg.data || msg;
      if (data.mode) {
        switchMode(data.mode);
      }

      const sec = data.security_info;
      if (sec) {
        document.getElementById('propHost').textContent = sec.host || 'Chưa có thông tin';
        document.getElementById('propCert').textContent = sec.certificate_status || 'Chưa có thông tin';
        document.getElementById('propProtocol').textContent = sec.protocol || 'Chưa có thông tin';
        document.getElementById('propCipher').textContent = sec.cipher || 'Chưa có thông tin';

        if (document.getElementById('propSubject')) {
          document.getElementById('propSubject').textContent = sec.subject || '—';
        }
        if (document.getElementById('propIssuer')) {
          document.getElementById('propIssuer').textContent = sec.issuer || '—';
        }
        if (document.getElementById('propValid')) {
          document.getElementById('propValid').textContent = (sec.valid_from && sec.valid_to) ? (sec.valid_from + ' đến ' + sec.valid_to) : 'Chưa có thông tin';
        }
        if (document.getElementById('propThumbprint')) {
          document.getElementById('propThumbprint').textContent = sec.thumbprint || '—';
        }
        if (document.getElementById('propSigAlg')) {
          document.getElementById('propSigAlg').textContent = sec.signature_algorithm || '—';
        }

        const banner = document.getElementById('securityBanner');
        const icon = document.getElementById('statusIcon');
        const title = document.getElementById('statusTitle');
        const desc = document.getElementById('statusDesc');

        banner.className = 'security-status-card';
        if (sec.host.startsWith('evergreen://') || sec.protocol.includes('Sandbox') || sec.protocol.includes('Local')) {
          banner.classList.add('internal');
          icon.innerHTML = '<path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/>';
          title.textContent = 'Trang nội bộ Pip';
          desc.textContent = 'Đây là trang nội bộ của Pip.';
        } else if (sec.is_secure) {
          banner.classList.add('secure');
          icon.innerHTML = '<rect x="3" y="11" width="18" height="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/>';
          title.textContent = 'Kết nối an toàn';
          desc.textContent = 'Trang sử dụng HTTPS. Pip giữ cơ chế kiểm tra chứng chỉ của WebView2.';
        } else {
          banner.classList.add('insecure');
          icon.innerHTML = '<rect x="3" y="11" width="18" height="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 9.9-1"/>';
          title.textContent = 'Kết nối chưa được mã hóa';
          desc.textContent = 'Bạn nên tránh nhập mật khẩu hoặc dữ liệu nhạy cảm trên kết nối HTTP.';
        }
      }
    };

    addEventListener('DOMContentLoaded',()=>postIpc('SidebarReady'));
    window.__syncZoom = function(factor) {
      const el = document.getElementById('sidebarZoomVal');
      if (el) el.textContent = Math.round(factor * 100) + '%';
    };
  </script>
</body>
</html>
"#;

pub fn get_sidebar_html() -> String {
    SIDEBAR_TEMPLATE
        .replace("{{LOGO_BASE64}}", LOGO_BASE64.trim())
        .replace("{{MASCOT_BASE64}}", MASCOT_BASE64.trim())
        .replace("{{DOWNLOAD_MASCOT_BASE64}}", DOWNLOAD_MASCOT_BASE64.trim())
}

pub static SIDEBAR_HTML: LazyLock<String> = LazyLock::new(get_sidebar_html);

#[cfg(test)]
mod tests {
    use super::SIDEBAR_HTML;

    #[test]
    fn menu_exposes_keep_awake_with_toggle_state() {
        assert!(SIDEBAR_HTML.contains("ToggleKeepAwakeTab"));
        assert!(SIDEBAR_HTML.contains("keepAwakeTabButton"));
        assert!(SIDEBAR_HTML.contains("aria-pressed"));
        assert!(SIDEBAR_HTML.contains("window.__setKeepAwake"));
    }
}
