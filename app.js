(() => {
  const $ = (selector) => document.querySelector(selector);
  const $$ = (selector) => [...document.querySelectorAll(selector)];
  const menu = $('#menu-pop');
  const settings = $('#settings-pop');
  const toast = $('#toast');
  const storageKey = 'pip-showcase-settings';
  let toastTimer;

  function notify(message) {
    toast.textContent = message;
    toast.classList.add('visible');
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => toast.classList.remove('visible'), 1800);
  }

  function showView(view) {
    menu.hidden = view !== 'menu';
    settings.hidden = view !== 'settings';
    $$('.show-tab').forEach((tab) => {
      const active = tab.dataset.view === view;
      tab.classList.toggle('active', active);
      tab.setAttribute('aria-selected', String(active));
    });
    $('#demo-caption').textContent = ({
      browser: 'Trang tab mới Pip — tìm kiếm Google, lối tắt và mascot PShareHub.',
      menu: 'Menu trình duyệt Pip — các thao tác quen thuộc được gom gọn.',
      settings: 'Bản xem trước cài đặt Pip; các tùy chọn bên dưới chỉ minh họa giao diện.'
    })[view] || '';
  }

  const preferences = (() => {
    try { return JSON.parse(localStorage.getItem(storageKey) || '{}'); }
    catch { return {}; }
  })();
  const provider = $('#search-provider');
  const motionToggle = $('#motion-toggle');
  const suggestionsToggle = $('#suggestions-toggle');
  const save = (values) => {
    try { localStorage.setItem(storageKey, JSON.stringify({ ...preferences, ...values })); }
    catch { /* Preview remains usable when storage is disabled. */ }
    Object.assign(preferences, values);
  };
  const shortcuts = $('.demo-tiles');
  provider.value = preferences.provider === 'Bing' ? 'Bing' : 'Google';
  motionToggle.checked = preferences.reduceMotion !== false;
  suggestionsToggle.checked = preferences.showSuggestions !== false;
  document.documentElement.classList.toggle('reduce-motion', motionToggle.checked);
  shortcuts.hidden = !suggestionsToggle.checked;

  $$('.show-tab').forEach((tab) => tab.addEventListener('click', () => showView(tab.dataset.view)));
  $('.avatar-button').addEventListener('click', () => showView(menu.hidden ? 'menu' : 'browser'));
  $('.dots').addEventListener('click', () => showView(menu.hidden ? 'menu' : 'browser'));
  $('.close-menu').addEventListener('click', () => showView('browser'));
  $('.plus-tab').addEventListener('click', () => notify('Đây là bản xem trước giao diện Pip.'));
  $('#demo-omnibox').addEventListener('click', () => $('#download').scrollIntoView({ block: 'center' }));
  $('.bookmark-toggle').addEventListener('click', (event) => {
    const button = event.currentTarget;
    const saved = button.getAttribute('aria-pressed') !== 'true';
    button.setAttribute('aria-pressed', String(saved));
    button.textContent = saved ? '★' : '☆';
    button.setAttribute('aria-label', saved ? 'Xóa khỏi dấu trang' : 'Thêm vào dấu trang');
    notify(saved ? 'Đã thêm trang xem trước vào dấu trang.' : 'Đã xóa dấu trang xem trước.');
  });
  $$('[data-toolbar]').forEach((button) => button.addEventListener('click', () => {
    const messages = { back: 'Bạn đang ở trang đầu tiên trong bản xem trước.', forward: 'Chưa có trang tiếp theo trong bản xem trước.', reload: 'Đã tải lại bản xem trước Pip.' };
    notify(messages[button.dataset.toolbar]);
  }));

  const actions = {
    newtab: 'Đây là bản xem trước trang tab mới Pip.',
    private: 'Cửa sổ ẩn danh là tính năng của ứng dụng Pip.',
    bookmarks: 'Dấu trang được quản lý trong ứng dụng Pip.',
    history: 'Lịch sử duyệt web thuộc ứng dụng Pip.',
    downloads: 'Trình quản lý tải xuống thuộc ứng dụng Pip.',
    window: 'Tạo cửa sổ mới trong ứng dụng Pip.',
    help: 'Trợ giúp Pip có trong ứng dụng.'
  };
  document.addEventListener('click', (event) => {
    const button = event.target.closest('[data-demo]');
    if (!button) return;
    const action = button.dataset.demo;
    if (action === 'settings') showView('settings');
    else if (action === 'newtab') showView('browser');
    else notify(actions[action] || 'Tính năng này chạy trong ứng dụng Pip.');
  });
  document.addEventListener('keydown', (event) => {
    if (event.key === 'Escape') showView('browser');
  });

  provider.addEventListener('change', () => {
    save({ provider: provider.value });
    $('#settings-save').textContent = `Đã lưu ${provider.value} cho bản demo.`;
  });
  motionToggle.addEventListener('change', () => {
    document.documentElement.classList.toggle('reduce-motion', motionToggle.checked);
    save({ reduceMotion: motionToggle.checked });
    $('#settings-save').textContent = 'Đã lưu tùy chọn chuyển động cho bản demo.';
  });
  suggestionsToggle.addEventListener('change', () => {
    shortcuts.hidden = !suggestionsToggle.checked;
    save({ showSuggestions: suggestionsToggle.checked });
    $('#settings-save').textContent = 'Đã lưu tùy chọn lối tắt cho bản demo.';
  });

  const currentVersion = $('#version').textContent.trim();
  fetch('/version.json', { cache: 'no-store' }).then((response) => response.ok ? response.json() : null).then((release) => {
    if (!release?.version) return;
    if (release.downloadAvailable && release.url) {
      try {
        const url = new URL(release.url, location.origin);
        if (url.protocol !== 'https:' && url.origin !== location.origin) return;
        const link = document.createElement('a');
        link.className = 'download-button';
        link.id = 'download-link';
        link.href = url.href;
        link.textContent = `Tải Pip ${release.version} cho Windows ↓`;
        $('#download-link').replaceWith(link);
      } catch { return; }
    }
    const newer = release.version.split('.').map(Number).some((part, index, latest) =>
      part > (Number(currentVersion.split('.')[index]) || 0) && latest.slice(0, index).every((value, i) => value === (Number(currentVersion.split('.')[i]) || 0))
    );
    if (!newer) return;
    const note = $('#update-note');
    note.append(`Đã có Pip ${release.version}. `);
    const link = document.createElement('a');
    link.textContent = 'Tải bản mới';
    try {
      const url = new URL(release.url || '/#download', location.origin);
      if (url.protocol !== 'https:' && url.origin !== location.origin) return;
      link.href = url.href;
    } catch { return; }
    note.append(link);
  }).catch(() => {});
})();
