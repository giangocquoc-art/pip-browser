//! HTML/CSS/JS Fluent UI Template for Cài đặt Pip & Uninstaller.
//! Styled with the exact same Fluent dark theme, Segoe UI Variable fonts, and SVG icons as Pip.

pub const INSTALLER_TEMPLATE: &str = r#"<!DOCTYPE html>
<html lang="vi">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>Cài đặt Pip</title>
  <style>
    :root {
      color-scheme: dark;
      --bg-gradient: radial-gradient(circle at 50% 25%, #22222e 0%, #16161d 100%);
      --surface-card: rgba(255, 255, 255, 0.04);
      --surface-card-hover: rgba(255, 255, 255, 0.06);
      --border-subtle: rgba(255, 255, 255, 0.08);
      --border-focus: rgba(78, 140, 255, 0.5);
      --text-main: #f8fafc;
      --text-secondary: #94a3b8;
      --text-muted: #64748b;
      --accent-green: #34d399;
      --accent-blue: #4e8cff;
      --accent-blue-hover: #3b76e1;
      --accent-red: #ef4444;
    }

    * {
      box-sizing: border-box;
      margin: 0;
      padding: 0;
      user-select: none;
      -webkit-user-select: none;
    }

    body {
      background: var(--bg-gradient);
      color: var(--text-main);
      font-family: -apple-system, BlinkMacSystemFont, "Segoe UI Variable Display", "Segoe UI", system-ui, sans-serif;
      height: 100vh;
      display: flex;
      flex-direction: column;
      justify-content: space-between;
      padding: 32px 36px 28px 36px;
      overflow: hidden;
    }

    /* Top Brand Lockup */
    .brand-header {
      display: flex;
      flex-direction: column;
      align-items: center;
      text-align: center;
    }

    .brand-logo {
      width: 240px;
      height: auto;
      max-height: 80px;
      object-fit: contain;
      margin-bottom: 12px;
    }

    .installer-title {
      font-size: 19px;
      font-weight: 600;
      color: var(--text-main);
      letter-spacing: -0.3px;
    }

    /* Wizard Content Area */
    .wizard-container {
      flex: 1;
      display: flex;
      flex-direction: column;
      justify-content: center;
      margin: 18px 0;
    }

    .step-view {
      display: none;
      flex-direction: column;
      animation: fadeIn 0.2s ease-out forwards;
    }

    .step-view.active {
      display: flex;
    }

    @keyframes fadeIn {
      from { opacity: 0; transform: translateY(4px); }
      to { opacity: 1; transform: translateY(0); }
    }

    /* Fluent Card */
    .card {
      background: var(--surface-card);
      border: 1px solid var(--border-subtle);
      border-radius: 12px;
      padding: 18px 20px;
      backdrop-filter: blur(16px);
      box-shadow: 0 10px 30px rgba(0, 0, 0, 0.35);
    }

    .card-title {
      display: flex;
      align-items: center;
      gap: 10px;
      font-size: 14px;
      font-weight: 600;
      color: var(--accent-green);
      margin-bottom: 14px;
    }

    /* Prerequisite List */
    .prereq-list {
      display: flex;
      flex-direction: column;
      gap: 12px;
    }

    .prereq-item {
      display: flex;
      align-items: flex-start;
      gap: 12px;
      font-size: 13px;
      line-height: 1.4;
    }

    .badge-icon {
      width: 20px;
      height: 20px;
      border-radius: 50%;
      background: rgba(52, 211, 153, 0.15);
      border: 1px solid rgba(52, 211, 153, 0.3);
      display: flex;
      align-items: center;
      justify-content: center;
      flex-shrink: 0;
      margin-top: 1px;
    }

    .badge-icon svg {
      width: 12px;
      height: 12px;
      color: var(--accent-green);
      stroke-width: 2.5;
    }

    .prereq-text {
      flex: 1;
    }

    .prereq-label {
      font-weight: 600;
      color: #e2e8f0;
      margin-right: 6px;
    }

    .prereq-desc {
      color: var(--text-secondary);
    }

    .step-note {
      font-size: 12px;
      color: var(--text-muted);
      margin-top: 14px;
      text-align: center;
    }

    /* Scrollable License Card */
    .license-box {
      max-height: 140px;
      overflow-y: auto;
      background: rgba(0, 0, 0, 0.2);
      border: 1px solid rgba(255, 255, 255, 0.05);
      border-radius: 8px;
      padding: 12px 14px;
      font-size: 12px;
      line-height: 1.5;
      color: #cbd5e1;
      margin-bottom: 16px;
      user-select: text;
      -webkit-user-select: text;
    }

    .license-box::-webkit-scrollbar {
      width: 6px;
    }
    .license-box::-webkit-scrollbar-track {
      background: rgba(255, 255, 255, 0.02);
    }
    .license-box::-webkit-scrollbar-thumb {
      background: rgba(255, 255, 255, 0.15);
      border-radius: 3px;
    }

    .license-heading {
      font-weight: 600;
      color: #f1f5f9;
      margin-bottom: 4px;
    }

    /* Checkbox Rows */
    .checkbox-group {
      display: flex;
      flex-direction: column;
      gap: 10px;
      margin-bottom: 14px;
    }

    .checkbox-label {
      display: flex;
      align-items: center;
      gap: 10px;
      font-size: 13px;
      color: #e2e8f0;
      cursor: pointer;
    }

    .checkbox-custom {
      width: 18px;
      height: 18px;
      border-radius: 5px;
      border: 1px solid rgba(255, 255, 255, 0.2);
      background: rgba(255, 255, 255, 0.05);
      display: flex;
      align-items: center;
      justify-content: center;
      transition: all 0.15s ease;
      flex-shrink: 0;
    }

    .checkbox-label input {
      display: none;
    }

    .checkbox-label input:checked + .checkbox-custom {
      background: var(--accent-blue);
      border-color: var(--accent-blue);
    }

    .checkbox-custom svg {
      width: 12px;
      height: 12px;
      stroke: white;
      stroke-width: 2.5;
      display: none;
    }

    .checkbox-label input:checked + .checkbox-custom svg {
      display: block;
    }

    .acceptance-text {
      font-size: 12px;
      font-weight: 500;
      color: #cbd5e1;
      text-align: center;
      margin-bottom: 4px;
    }

    /* Progress View */
    .progress-container {
      display: flex;
      flex-direction: column;
      gap: 14px;
      padding: 10px 0;
    }

    .progress-header {
      display: flex;
      justify-content: space-between;
      align-items: center;
      font-size: 13px;
    }

    .progress-step-text {
      color: #e2e8f0;
      font-weight: 500;
    }

    .progress-pct {
      font-weight: 600;
      color: var(--accent-green);
    }

    .progress-bar-track {
      width: 100%;
      height: 8px;
      border-radius: 4px;
      background: rgba(255, 255, 255, 0.08);
      overflow: hidden;
      position: relative;
    }

    .progress-bar-fill {
      height: 100%;
      width: 0%;
      border-radius: 4px;
      background: linear-gradient(90deg, var(--accent-green) 0%, var(--accent-blue) 100%);
      transition: width 0.25s ease-out;
      box-shadow: 0 0 12px rgba(52, 211, 153, 0.4);
    }

    /* Complete View */
    .complete-box {
      display: flex;
      flex-direction: column;
      align-items: center;
      text-align: center;
      padding: 10px 0;
    }

    .complete-icon {
      width: 48px;
      height: 48px;
      border-radius: 50%;
      background: rgba(52, 211, 153, 0.15);
      border: 1px solid rgba(52, 211, 153, 0.3);
      display: flex;
      align-items: center;
      justify-content: center;
      margin-bottom: 14px;
    }

    .complete-icon svg {
      width: 24px;
      height: 24px;
      stroke: var(--accent-green);
      stroke-width: 2.5;
    }

    .complete-desc {
      font-size: 13px;
      color: var(--text-secondary);
      max-width: 420px;
      line-height: 1.5;
      margin-bottom: 16px;
    }

    /* Footer Action Buttons */
    .button-row {
      display: flex;
      justify-content: flex-end;
      gap: 12px;
      margin-top: 10px;
    }

    .btn {
      padding: 10px 22px;
      border-radius: 8px;
      font-size: 13px;
      font-weight: 600;
      cursor: pointer;
      outline: none;
      transition: all 0.15s ease;
      display: inline-flex;
      align-items: center;
      justify-content: center;
    }

    .btn-secondary {
      background: rgba(255, 255, 255, 0.05);
      border: 1px solid var(--border-subtle);
      color: #cbd5e1;
    }

    .btn-secondary:hover {
      background: rgba(255, 255, 255, 0.09);
      color: #f8fafc;
      border-color: rgba(255, 255, 255, 0.15);
    }

    .btn-primary {
      background: var(--accent-blue);
      border: 1px solid transparent;
      color: white;
      box-shadow: 0 4px 14px rgba(78, 140, 255, 0.3);
    }

    .btn-primary:hover {
      background: var(--accent-blue-hover);
      box-shadow: 0 6px 18px rgba(78, 140, 255, 0.45);
      transform: translateY(-1px);
    }

    .btn-accept {
      background: linear-gradient(135deg, #10b981 0%, #059669 100%);
      border: 1px solid transparent;
      color: white;
      box-shadow: 0 4px 14px rgba(16, 185, 129, 0.35);
    }

    .btn-accept:hover {
      background: linear-gradient(135deg, #059669 0%, #047857 100%);
      box-shadow: 0 6px 18px rgba(16, 185, 129, 0.5);
      transform: translateY(-1px);
    }

    .btn-danger {
      background: var(--accent-red);
      border: 1px solid transparent;
      color: white;
      box-shadow: 0 4px 14px rgba(239, 68, 68, 0.35);
    }

    .btn-danger:hover {
      background: #dc2626;
      box-shadow: 0 6px 18px rgba(239, 68, 68, 0.5);
      transform: translateY(-1px);
    }

    :root{color-scheme:light;--bg-gradient:linear-gradient(130deg,#fffaf6,#fff0e5);--surface-card:#fff;--surface-card-hover:#fff4ec;--border-subtle:#f1e2d5;--border-focus:#f97316;--text-main:#17202e;--text-secondary:#596779;--text-muted:#596779;--accent-blue:#f97316;--accent-blue-hover:#e85d0d;--accent-green:#15803d;--accent-red:#dc2626}
    .brand-logo{width:72px;height:72px;max-height:72px;margin-bottom:8px}.brand-header{flex-direction:row;gap:18px;text-align:left}.installer-title{color:#17202e}.card{background:#fff;border-color:#f1e2d5;box-shadow:0 4px 16px rgba(98,57,28,.06)}.card-title,.license-heading,.prereq-label,.checkbox-label,.progress-step-text{color:var(--text-main)}.license-box{max-height:160px;overflow:auto;background:#fff8f2;border:1px solid #f1e2d5;color:#4b586b}.license-box p{color:#4b586b}.license-heading{color:#17202e}.license-box::-webkit-scrollbar-track{background:#fff0e5}.license-box::-webkit-scrollbar-thumb{background:#e7cbb5}.acceptance-text,.step-note{color:#596779}.btn{min-height:38px;border-radius:9px}.btn-secondary{background:#fff;color:#344054;border-color:#e8d8ca}.btn-secondary:hover{background:#fff4ec;color:#17202e;border-color:#e8c7ae}.btn-accept,.btn-primary{background:#f97316;border-color:#f97316;color:#fff;box-shadow:none}.btn-accept:hover,.btn-primary:hover{background:#e85d0d;border-color:#e85d0d;box-shadow:none;transform:none}.btn-danger{background:#dc2626;box-shadow:none}.btn-danger:hover{background:#b91c1c;box-shadow:none;transform:none}.complete-box h3{color:var(--text-main)!important}.progress-bar-track{background:#ffe8d8}.progress-bar-fill{box-shadow:none;background:#f97316}.progress-pct{color:#b84b08}.badge-icon{background:#ecfdf3;border-color:#bbf7d0}.badge-icon svg,.complete-icon svg{color:#15803d;stroke:#15803d}.complete-icon{background:#ecfdf3;border-color:#bbf7d0}#errorMessage{color:#b91c1c!important;background:#fef2f2;border-color:#fecaca}*:focus-visible{outline:2px solid #f97316;outline-offset:3px}*,*::before,*::after{animation:none!important;transition:none!important;backdrop-filter:none!important}
  </style>
</head>
<body>

  <!-- Brand Header -->
  <div class="brand-header">
    <img src="data:image/png;base64,{{LOGO_BASE64}}" class="brand-logo" alt="Pip" />
    <h2 class="installer-title" id="titleText" data-uninstall="{{UNINSTALL_MODE}}">{{INSTALLER_TITLE}}</h2>
  </div>

  <!-- Wizard Steps Container -->
  <div class="wizard-container"><p id="errorMessage" role="alert" hidden style="color:#c2410c;font-size:12px;margin-bottom:12px"></p>

    <!-- Step 0: Prerequisite Inspection -->
    <div class="step-view {{STEP0_ACTIVE}}" id="step0">
      <div class="card">
        <div class="card-title">
          <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/></svg>
          Pip đã sẵn sàng để cài đặt
        </div>
        <div class="prereq-list">
          <div class="prereq-item">
            <div class="badge-icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor"><polyline points="20 6 9 17 4 12"></polyline></svg></div>
            <div class="prereq-text">
              <span class="prereq-label">Hệ điều hành:</span>
              <span class="prereq-desc">Windows 10/11 64 bit</span>
            </div>
          </div>
          <div class="prereq-item">
            <div class="badge-icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor"><polyline points="20 6 9 17 4 12"></polyline></svg></div>
            <div class="prereq-text">
              <span class="prereq-label">Quyền cài đặt:</span>
              <span class="prereq-desc">Dành cho tài khoản hiện tại, không cần quyền quản trị</span>
            </div>
          </div>
          <div class="prereq-item">
            <div class="badge-icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor"><polyline points="20 6 9 17 4 12"></polyline></svg></div>
            <div class="prereq-text">
              <span class="prereq-label">Bộ máy hiển thị:</span>
              <span class="prereq-desc">Microsoft WebView2 đã có trên máy</span>
            </div>
          </div>
          <div class="prereq-item">
            <div class="badge-icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor"><polyline points="20 6 9 17 4 12"></polyline></svg></div>
            <div class="prereq-text">
              <span class="prereq-label">Nơi cài đặt:</span>
              <span class="prereq-desc">%LOCALAPPDATA%\Programs\Pip</span>
            </div>
          </div>
        </div>
      </div>
      <div class="step-note">Bấm Tiếp tục để xem giấy phép và chọn lối tắt.</div>
      <div class="button-row">
        <button class="btn btn-secondary" onclick="postAction('cancel')">Hủy</button>
        <button class="btn btn-primary" onclick="showStep(1)">Tiếp tục →</button>
      </div>
    </div>

    <!-- Step 1: Giấy phép và tùy chọn -->
    <div class="step-view" id="step1">
      <div class="card">
        <div class="license-box">
          <div class="license-heading">Phần mềm nguồn mở</div>
          <p>Pip dựa trên Evergreen Browser. Mã nguồn, giấy phép và ghi nhận các thành phần đi kèm được lưu trong thư mục cài đặt.</p>
          <br>
          <div class="license-heading">Các thành phần đi kèm:</div>
          <p>Bao gồm Brave Adblock Rust, bộ lọc EasyList và scriptlet uBlock/Brave theo giấy phép tương ứng. Phần mềm được cung cấp nguyên trạng. WebView2 được Microsoft cập nhật.</p>
        </div>

        <div class="checkbox-group">
          <label class="checkbox-label">
            <input type="checkbox" id="chkDesktop" checked />
            <div class="checkbox-custom"><svg viewBox="0 0 24 24" fill="none"><polyline points="20 6 9 17 4 12"></polyline></svg></div>
            <span>Tạo lối tắt trên màn hình</span>
          </label>
          <label class="checkbox-label">
            <input type="checkbox" id="chkStartMenu" checked />
            <div class="checkbox-custom"><svg viewBox="0 0 24 24" fill="none"><polyline points="20 6 9 17 4 12"></polyline></svg></div>
            <span>Thêm vào Start Menu</span>
          </label>
        </div>

        <div class="acceptance-text">Cài đặt cho tài khoản hiện tại. Các giấy phép đầy đủ đi kèm Pip.</div>
      </div>

      <div class="button-row">
        <button class="btn btn-secondary" onclick="postAction('cancel')">Hủy</button>
        <button class="btn btn-accept" onclick="startInstall()">Cài đặt Pip</button>
      </div>
    </div>

    <!-- Step 2: Live Worker Progress -->
    <div class="step-view" id="step2">
      <div class="card">
        <div class="card-title">
          <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
          Đang cài Pip
        </div>
        <div class="progress-container">
          <div class="progress-header">
            <span class="progress-step-text" id="progressStatus">Đang giải nén Pip...</span>
            <span class="progress-pct" id="progressPct">0%</span>
          </div>
          <div class="progress-bar-track">
            <div class="progress-bar-fill" id="progressBar"></div>
          </div>
        </div>
      </div>
      <div class="step-note">Đang chép tệp và tạo lối tắt. Bạn chờ một chút nhé.</div>
    </div>

    <!-- Step 3: Cài đặt hoàn tất -->
    <div class="step-view" id="step3">
      <div class="card complete-box">
        <div class="complete-icon">
          <svg viewBox="0 0 24 24" fill="none"><polyline points="20 6 9 17 4 12"></polyline></svg>
        </div>
        <h3 style="font-size: 16px; margin-bottom: 6px; color: #f8fafc;">Cài đặt xong rồi!</h3>
        <p class="complete-desc">Pip đã sẵn sàng. Mở trình duyệt để bắt đầu lướt web nhé.</p>

        <label class="checkbox-label" style="margin-bottom: 6px;">
          <input type="checkbox" id="chkLaunch" checked />
          <div class="checkbox-custom"><svg viewBox="0 0 24 24" fill="none"><polyline points="20 6 9 17 4 12"></polyline></svg></div>
          <span>Mở Pip ngay</span>
        </label>
      </div>

      <div class="button-row">
        <button class="btn btn-primary" onclick="finishInstall()">Bắt đầu thôi</button>
      </div>
    </div>

    <!-- Uninstaller Step 0: Confirm -->
    <div class="step-view {{STEP_UNINSTALL0_ACTIVE}}" id="stepUninstall0">
      <div class="card">
        <div class="card-title" style="color: #f87171;">
          <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2"><polyline points="3 6 5 6 21 6"></polyline><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"></path></svg>
          Gỡ cài đặt Pip
        </div>
        <p style="font-size: 13px; color: var(--text-secondary); line-height: 1.5; margin-bottom: 16px;">
          Gỡ Pip và các lối tắt khỏi máy? Hãy đóng các cửa sổ Pip trước khi tiếp tục.
        </p>

        <label class="checkbox-label">
          <input type="checkbox" id="chkRemoveUserData" />
          <div class="checkbox-custom"><svg viewBox="0 0 24 24" fill="none"><polyline points="20 6 9 17 4 12"></polyline></svg></div>
          <span>Xóa cả hồ sơ, lịch sử, cookie và bộ nhớ đệm của Pip</span>
        </label>
      </div>

      <div class="button-row">
        <button class="btn btn-secondary" onclick="postAction('cancel')">Hủy</button>
        <button class="btn btn-danger" onclick="startUninstall()">Gỡ cài đặt</button>
      </div>
    </div>

    <!-- Uninstaller Step 1: In Progress -->
    <div class="step-view" id="stepUninstall1">
      <div class="card">
        <div class="card-title">
          <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/></svg>
          Đang gỡ Pip
        </div>
        <div class="progress-container">
          <div class="progress-header">
            <span class="progress-step-text" id="uninstStatus">Đang xóa các tệp và lối tắt của Pip...</span>
            <span class="progress-pct" id="uninstPct">50%</span>
          </div>
          <div class="progress-bar-track">
            <div class="progress-bar-fill" id="uninstBar" style="width: 50%;"></div>
          </div>
        </div>
      </div>
    </div>

    <!-- Uninstaller Step 2: Complete -->
    <div class="step-view" id="stepUninstall2">
      <div class="card complete-box">
        <div class="complete-icon">
          <svg viewBox="0 0 24 24" fill="none"><polyline points="20 6 9 17 4 12"></polyline></svg>
        </div>
        <h3 style="font-size: 16px; margin-bottom: 6px; color: #f8fafc;">Đã gỡ Pip</h3>
        <p class="complete-desc">Đã gỡ Pip và các lối tắt. Dữ liệu được giữ lại nếu bạn không chọn xóa.</p>
      </div>

      <div class="button-row">
        <button class="btn btn-secondary" onclick="postAction('cancel')">Đóng</button>
      </div>
    </div>

  </div>

  <script>
    function postAction(action, payload = {}) {
      if (window.ipc) {
        window.ipc.postMessage(JSON.stringify({ action, ...payload }));
      }
    }

    function showStep(stepIndex) {
      document.querySelectorAll('.step-view').forEach(el => el.classList.remove('active'));
      const target = document.getElementById('step' + stepIndex);
      if (target) target.classList.add('active');

      if (stepIndex === 1) {
        document.getElementById('titleText').textContent = 'Giấy phép và tùy chọn';
      } else if (stepIndex === 2) {
        document.getElementById('titleText').textContent = 'Đang cài Pip';
      } else if (stepIndex === 3) {
        document.getElementById('titleText').textContent = 'Cài đặt hoàn tất';
      }
    }

    function startInstall() {
      const desktop = document.getElementById('chkDesktop').checked;
      const startMenu = document.getElementById('chkStartMenu').checked;
      showStep(2);
      postAction('accept', { desktop, start_menu: startMenu });
    }

    function finishInstall() {
      const launch = document.getElementById('chkLaunch').checked;
      postAction('finish', { launch });
    }

    function startUninstall() {
      const removeData = document.getElementById('chkRemoveUserData').checked;
      document.querySelectorAll('.step-view').forEach(el => el.classList.remove('active'));
      document.getElementById('stepUninstall1').classList.add('active');
      postAction('start_uninstall', { remove_data: removeData });
    }

    window.updateProgress = function(pct, text) {
      const bar = document.getElementById('progressBar');
      const pctEl = document.getElementById('progressPct');
      const textEl = document.getElementById('progressStatus');
      if (bar) bar.style.width = pct + '%';
      if (pctEl) pctEl.textContent = pct + '%';
      if (textEl && text) textEl.textContent = text;
    };

    window.installFailed = function(message) {
      const error=document.getElementById('errorMessage');error.textContent=message;error.hidden=false;
      if (document.getElementById('titleText').dataset.uninstall==='true') window.initUninstallMode(); else showStep(1);
    };
    window.installComplete = function() {
      showStep(3);
    };

    window.uninstallComplete = function() {
      document.querySelectorAll('.step-view').forEach(el => el.classList.remove('active'));
      document.getElementById('stepUninstall2').classList.add('active');
    };

    window.initUninstallMode = function() {
      document.getElementById('titleText').dataset.uninstall='true';
      document.querySelectorAll('.step-view').forEach(el => el.classList.remove('active'));
      document.getElementById('stepUninstall0').classList.add('active');
      document.getElementById('titleText').textContent = 'Gỡ cài đặt Pip';
    };
  </script>
</body>
</html>
"#;

pub fn get_installer_html(logo_base64: &str, is_uninstall: bool) -> String {
    let (title, step0_active, uninst0_active) = if is_uninstall {
        ("Gỡ cài đặt Pip", "", "active")
    } else {
        ("Chào mừng bạn đến với Pip", "active", "")
    };

    INSTALLER_TEMPLATE
        .replace("{{LOGO_BASE64}}", logo_base64.trim())
        .replace("{{INSTALLER_TITLE}}", title)
        .replace(
            "{{UNINSTALL_MODE}}",
            if is_uninstall { "true" } else { "false" },
        )
        .replace("{{STEP0_ACTIVE}}", step0_active)
        .replace("{{STEP_UNINSTALL0_ACTIVE}}", uninst0_active)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_installer_html_install_mode() {
        let html = get_installer_html("test_logo", false);
        assert!(html.contains("Chào mừng bạn đến với Pip"));
        assert!(html.contains(r#"id="step0""#));
        assert!(html.contains(r#"class="step-view active" id="step0""#));
        assert!(html.contains(r#"class="step-view " id="stepUninstall0""#));
    }

    #[test]
    fn test_installer_html_uninstall_mode() {
        let html = get_installer_html("test_logo", true);
        assert!(html.contains("Gỡ cài đặt Pip"));
        assert!(html.contains(r#"id="stepUninstall0""#));
        assert!(html.contains(r#"class="step-view active" id="stepUninstall0""#));
        assert!(html.contains(r#"class="step-view " id="step0""#));
    }
}
