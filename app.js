(() => {
  const $ = (selector) => document.querySelector(selector);
  const $$ = (selector) => [...document.querySelectorAll(selector)];
  const previewImage = $('#app-preview-image');
  const previewDialog = $('#preview-dialog');
  const previews = {
    browser: ['assets/app-home.webp', 'Ảnh chụp giao diện Pip cho Windows: thanh điều hướng, mascot, ô tìm kiếm Google và các lối tắt trang web.', 'Trang chủ Pip với tìm kiếm Google và các lối tắt quen thuộc.'],
    private: ['assets/app-private.webp', 'Ảnh chụp cửa sổ ẩn danh Pip với mascot riêng, ô tìm kiếm ẩn danh và nút gợi ý Google DNS.', 'Cửa sổ ẩn danh Pip với nút tùy chọn Google DNS.'],
    menu: ['assets/app-profiles.webp', 'Ảnh chụp menu hồ sơ Pip để chuyển đổi các hồ sơ duyệt web riêng.', 'Menu hồ sơ để chuyển giữa các hồ sơ Pip.'],
    settings: ['assets/app-settings-current.webp', 'Ảnh chụp cửa sổ Pip đang mở trang Cài đặt trên Windows, gồm thanh công cụ và các tùy chọn trình duyệt.', 'Cài đặt thực tế đang mở trong cửa sổ Pip.']
  };
  const showTabs = $$('.show-tab');
  const selectPreview = (tab) => {
    const view = tab.dataset.view;
    const preview = previews[view];
    if (!preview) return false;
    previewImage.src = preview[0];
    previewImage.alt = preview[1];
    $('#preview-dialog-image').src = previewImage.src;
    $('#preview-dialog-image').alt = preview[1];
    $('#demo-caption').textContent = preview[2];
    showTabs.forEach((item) => {
      const active = item === tab;
      item.classList.toggle('active', active);
      item.setAttribute('aria-selected', String(active));
      item.tabIndex = active ? 0 : -1;
    });
    $('#app-preview').setAttribute('aria-labelledby', tab.id);
    return true;
  };
  showTabs.forEach((tab, index) => {
    if (!tab.id) tab.id = `show-tab-${tab.dataset.view || index}`;
    tab.tabIndex = tab.classList.contains('active') ? 0 : -1;
    tab.addEventListener('click', () => selectPreview(tab));
    tab.addEventListener('keydown', (event) => {
      let targetIndex;
      if (event.key === 'ArrowRight') targetIndex = (index + 1) % showTabs.length;
      else if (event.key === 'ArrowLeft') targetIndex = (index + showTabs.length - 1) % showTabs.length;
      else if (event.key === 'Home') targetIndex = 0;
      else if (event.key === 'End') targetIndex = showTabs.length - 1;
      else return;
      event.preventDefault();
      const target = showTabs[targetIndex];
      if (selectPreview(target)) target.focus();
    });
  });
  const initialTab = showTabs.find((tab) => tab.classList.contains('active'));
  if (initialTab) $('#app-preview').setAttribute('aria-labelledby', initialTab.id);

  $('#preview-zoom').addEventListener('click', () => {
    $('#preview-dialog-image').src = previewImage.src;
    $('#preview-dialog-image').alt = previewImage.alt;
    previewDialog.showModal();
  });
  $('#preview-close').addEventListener('click', () => previewDialog.close());
  previewDialog.addEventListener('click', (event) => {
    if (event.target === previewDialog) previewDialog.close();
  });

  const pageVersion = $('#version').textContent.trim();
  const directDownloadUrl = 'https://download.pip.psharehub.com/';
  async function fetchLatestRelease() {
    const cacheKey = 'pip-latest-release-v2';
    try {
      const cached = JSON.parse(localStorage.getItem(cacheKey) || 'null');
      if (cached?.release && Date.now() - cached.checkedAt < 600000) return cached.release;
    } catch { /* Fetch the release when browser storage is unavailable. */ }
    try {
      const response = await fetch('https://api.github.com/repos/giangocquoc-art/pip-browser/releases/latest', {
        headers: { Accept: 'application/vnd.github+json' },
        cache: 'no-store'
      });
      if (!response.ok) throw new Error('GitHub release lookup failed');
      const metadata = await response.json();
      const version = metadata.tag_name?.replace(/^v/, '');
      if (!version) throw new Error('Latest release has no version');
      const release = { version, downloadAvailable: true, url: directDownloadUrl };
      try { localStorage.setItem(cacheKey, JSON.stringify({ checkedAt: Date.now(), release })); }
      catch { /* The current page still uses the release response. */ }
      return release;
    } catch {
      try {
        const response = await fetch('/version.json', { cache: 'no-store' });
        return response.ok ? response.json() : null;
      } catch { return null; }
    }
  }
  const compareVersions = (left, right) => {
    const a = left.split('.').map(Number);
    const b = right.split('.').map(Number);
    for (let i = 0; i < Math.max(a.length, b.length); i++) {
      const difference = (a[i] || 0) - (b[i] || 0);
      if (difference) return difference;
    }
    return 0;
  };
  fetchLatestRelease().then((release) => {
    if (!release?.version) return;
    $('#version').textContent = release.version;
    if (!release.downloadAvailable || !release.url) return;
    let downloadUrl;
    try {
      downloadUrl = new URL(release.url, location.origin);
      if (downloadUrl.protocol !== 'https:') return;
    } catch { return; }
    const link = $('#download-link');
    link.href = directDownloadUrl;
    link.textContent = `Tải Pip ${release.version} cho Windows ↓`;
    if (compareVersions(release.version, pageVersion) <= 0) return;
    const note = $('#update-note');
    note.append(`Đã có Pip ${release.version}. `);
    const updateLink = document.createElement('a');
    updateLink.href = directDownloadUrl;
    updateLink.textContent = 'Tải bản mới';
    note.append(updateLink);
  }).catch(() => {});
})();
