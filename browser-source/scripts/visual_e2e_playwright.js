/**
 * Headless UI smoke and interaction checks for Pip's current built-in pages.
 * Run with: node scripts/visual_e2e_playwright.js
 * Outputs stay under ui-review/qa; no installed browser profile or live site is used.
 */
const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');

const root = path.resolve(__dirname, '..');
const reviewDir = path.join(root, 'ui-review');
const outputDir = path.join(reviewDir, 'qa');
const npxCache = path.join(process.env.LOCALAPPDATA || '', 'npm-cache', '_npx');
const axePath = fs.existsSync(npxCache) && fs.readdirSync(npxCache)
  .map(name => path.join(npxCache, name, 'node_modules', 'axe-core', 'axe.min.js'))
  .find(location => fs.existsSync(location));

let playwright;
let activeBrowser;
try {
  playwright = require('playwright');
} catch (error) {
  const localAppData = process.env.LOCALAPPDATA || path.join(process.env.USERPROFILE || '', 'AppData', 'Local');
  const cache = path.join(localAppData, 'npm-cache', '_npx');
  const candidates = fs.existsSync(cache) ? fs.readdirSync(cache)
    .map(name => path.join(cache, name, 'node_modules', 'playwright'))
    .filter(location => fs.existsSync(location)) : [];
  for (const candidate of candidates) {
    try { playwright = require(candidate); break; } catch (_) { /* Skip incomplete npx cache entries. */ }
  }
  if (!playwright) throw new Error(`Playwright unavailable: ${error.message}`);
}

function check(condition, message) {
  if (!condition) throw new Error(message);
}

function ipc(page) {
  return page.evaluate(() => window.__pipQaMessages || []);
}

async function main() {
  const render = spawnSync('python', ['tools/render_ui_review.py'], { cwd: root, encoding: 'utf8' });
  check(render.status === 0, `Could not render current Pip templates: ${render.stderr || render.stdout}`);
  check(Boolean(axePath), 'axe-core is required for the accessibility audit');
  fs.mkdirSync(outputDir, { recursive: true });

  const edgePath = process.env.PIP_QA_BROWSER || 'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe';
  const browser = await playwright.chromium.launch({
    headless: true,
    ...(fs.existsSync(edgePath) ? { executablePath: edgePath } : {}),
  });
  activeBrowser = browser;
  const errors = [];
  const accessibility = [];
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  await context.addInitScript(() => {
    window.__pipQaMessages = [];
    window.ipc = { postMessage(raw) { window.__pipQaMessages.push(JSON.parse(raw)); } };
  });
  const page = await context.newPage();
  page.on('pageerror', error => errors.push(error.message));
  page.on('console', message => { if (message.type() === 'error') errors.push(message.text()); });

  const html = name => fs.readFileSync(path.join(reviewDir, `${name}.html`), 'utf8');
  async function loadSurface(name) {
    await page.goto('about:blank'); // Each native surface owns a separate JavaScript realm.
    await page.setContent(html(name));
    check(await page.evaluate(() => [...document.querySelectorAll('*')].every(el => { const style=getComputedStyle(el); return style.animationName==='none' && style.transitionDuration==='0s' && style.backdropFilter==='none'; })), `Motion or blur remains in ${name}`);
  }
  async function auditAccessibility(name) {
    if (!axePath) return;
    await page.addScriptTag({ path: axePath });
    const result = await page.evaluate(async () => {
      const scan = await axe.run(document, { runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'] } });
      return scan.violations.map(item => ({
        id: item.id,
        impact: item.impact,
        help: item.help,
        nodes: item.nodes.slice(0, 5).map(node => ({
          target: node.target.join(' '),
          html: node.html,
          summary: node.failureSummary,
        })),
      }));
    });
    accessibility.push({ page: name, violations: result });
  }

  await loadSurface('home');
  check(await page.locator('#homeNote').count() === 0, 'Redundant new-tab footnote should not be rendered');
  const homeText = await page.locator('body').innerText();
  check(!homeText.includes('Trình duyệt của bạn, theo cách của bạn'), 'Redundant new-tab slogan should not be rendered');
  check(!homeText.includes('Dùng Google để tìm kiếm'), 'Search-engine footnote should not be rendered');
  check(!homeText.includes('Trang tab mới gọn nhẹ, không ảnh động'), 'New-tab implementation note should not be rendered');
  check(await page.title().then(() => page.locator('#homeSearch').count()) === 1, 'Pip new tab search field missing');
  check(await page.evaluate(() => {
    const hero = document.querySelector('.hero').getBoundingClientRect();
    const search = document.querySelector('.search').getBoundingClientRect();
    return search.height >= 66 && search.width >= 700 &&
      Math.abs(search.left + search.width / 2 - hero.left - hero.width / 2) < 2;
  }), 'Search must be large and centered over the mascot background');
  await page.locator('#homeSearch').fill('rust browser');
  await page.locator('#searchForm button[type="submit"]').click();
  let messages = await ipc(page);
  check(messages.some(item => item.action === 'Navigate' && item.payload.url === 'rust browser'), 'New tab search did not reach the shared native URL/search handler');
  await page.locator('.shortcut').first().click();
  messages = await ipc(page);
  check(messages.some(item => item.action === 'Navigate' && item.payload.url === 'https://www.google.com/'), 'Google quick link did not navigate');
  await page.locator('#homeSearch').fill('');
  await page.locator('#homeView h1').click();
  await page.screenshot({ path: path.join(outputDir, 'home-desktop.png'), fullPage: true });

  await page.setViewportSize({ width: 375, height: 760 });
  const homeOverflow = await page.evaluate(() => document.documentElement.scrollWidth > innerWidth + 1);
  check(!homeOverflow, 'New tab has horizontal overflow at 375px');
  await page.screenshot({ path: path.join(outputDir, 'home-mobile.png'), fullPage: true });
  await page.locator('.guide summary').click();
  check(await page.locator('.guide-content').isVisible(), 'Quick guide did not open');
  await page.locator('.guide summary').focus();
  await page.keyboard.press('Enter');
  check(!await page.locator('.guide-content').isVisible(), 'Quick guide is not keyboard operable');
  await auditAccessibility('home-mobile');

  await page.emulateMedia({ colorScheme: 'dark' });
  await page.screenshot({ path: path.join(outputDir, 'home-dark.png'), fullPage: true });
  await auditAccessibility('home-dark');
  await page.emulateMedia({ colorScheme: 'light' });

  for (const kind of ['bookmarks','history']) {
    await page.locator(`.rail-link[data-view="${kind}"]`).click();
    await page.waitForFunction(selector => { const img=document.querySelector(selector); return img?.complete && img.naturalWidth>0; }, `#${kind}View .state-card img`);
    check(!await page.evaluate(() => document.documentElement.scrollWidth > innerWidth+1), `Empty ${kind} view overflows`);
    await page.screenshot({path:path.join(outputDir, `empty-${kind}.png`),fullPage:true});
  }

  await page.setViewportSize({ width: 1280, height: 800 });
  await loadSurface('private-home');
  check(await page.locator('.private-home').isVisible() && !(await page.locator('.rail').isVisible()), 'Private home must replace the regular browser home');
  check((await page.locator('.private-home').innerText()).includes('Pip ẩn danh.') && (await page.locator('.private-home').innerText()).includes('Google DNS'), 'Private home heading or DNS recommendation is missing');
  check((await page.locator('.private-recommendation').innerText()).includes('Khuyến nghị nên bật') && (await page.locator('.private-recommendation').innerText()).includes('nút G DNS trên thanh công cụ'), 'Incognito must recommend the existing toolbar DNS control');
  check(await page.locator('#privateDnsToggle,#privateDnsStatus').count() === 0, 'Incognito page must not add a duplicate DNS toggle');
  check(!(await page.locator('.private-home').innerText()).includes('Ẩn danh không lưu lịch sử'), 'Private home must stay free of the removed explanatory footnote');
  check(await page.evaluate(() => { const url=getComputedStyle(document.querySelector('.private-home')).backgroundImage.match(/url\(["']?(.*?)["']?\)/)?.[1]; if(!url)return false;const image=new Image();image.src=url;return image.complete&&image.naturalWidth>=1600; }), 'Private mascot background did not load at full size');
  await page.locator('#privateSearch').fill('youtube.com');
  await page.locator('#privateSearchForm button[type="submit"]').click();
  check((await ipc(page)).some(item => item.action === 'Navigate' && item.payload.url === 'youtube.com'), 'Private search did not use the native navigation handler');
  await page.screenshot({path:path.join(outputDir,'private-home.png'),fullPage:true});
  await auditAccessibility('private-home');

  await page.setViewportSize({ width: 1280, height: 800 });
  await loadSurface('settings');
  await page.locator('#searchEngineSelect').selectOption('bing');
  messages = await ipc(page);
  check(messages.some(item => item.action === 'SetSearchEngine' && item.payload.engine === 'bing'), 'Search-engine setting did not emit its IPC action');
  await page.locator('#googleDnsToggle').check();
  messages = await ipc(page);
  check(messages.some(item => item.action === 'SetGoogleDns' && item.payload.enabled === true), 'Google DNS setting did not emit its IPC action');
  await page.locator('#askWhereToSaveToggle').uncheck();
  messages = await ipc(page);
  check(messages.some(item => item.action === 'SaveSettings' && (item.payload.settings_json || '').includes('ask_where_to_save')), 'Download preference did not save through IPC');
  check(await page.locator('#featureLinkPreviewToggle').count() === 1 && await page.locator('#enableLinkPreviewToggle,#showStatusPreviewToggle').count() === 0, 'Link preview should have one clear setting');
  await page.locator('#featureLinkPreviewToggle').uncheck();
  check((await ipc(page)).some(item => item.action === 'SaveSettings' && JSON.parse(item.payload.settings_json).appearance?.show_status_preview === false), 'Link preview setting does not control the effective native configuration');
  await page.locator('#clearCacheButton').click();
  check((await ipc(page)).some(item => item.action === 'ClearCache'), 'Disk cache cleanup did not reach the host');
  for (const disclosure of await page.locator('.help-card details').all()) {
    await disclosure.locator('summary').click();
    check(await disclosure.locator('p').isVisible(), 'A settings explanation did not open');
  }
  await page.screenshot({ path: path.join(outputDir, 'settings-desktop.png'), fullPage: true });

  await page.setViewportSize({ width: 640, height: 800 });
  const settingsOverflow = await page.evaluate(() => document.documentElement.scrollWidth > innerWidth + 1);
  check(!settingsOverflow, 'Settings has horizontal overflow at 640px');
  await page.screenshot({ path: path.join(outputDir, 'settings-narrow.png'), fullPage: true });
  await auditAccessibility('settings-narrow');
  await page.setViewportSize({ width: 375, height: 800 });
  check(!await page.evaluate(() => document.documentElement.scrollWidth > innerWidth + 1), 'Settings overflow at 375px');
  await page.emulateMedia({ colorScheme: 'dark' });
  await auditAccessibility('settings-dark');
  await page.emulateMedia({ colorScheme: 'light' });

  await page.setViewportSize({ width: 1280, height: 440 });
  await loadSurface('chrome-profile-menu');
  await page.evaluate(() => {
    window.__shellUpdate({ tabs: [{ id: 1, title: 'Broken favicon fixture', url: 'https://example.com/', favicon_uri: 'data:image/png;base64,broken' }], active_tab_id: 1 });
  });
  await page.waitForFunction(() => document.querySelector('.tab-favicon')?.src.startsWith('data:image/svg+xml,'));
  check(await page.locator('.tab-favicon').evaluate(el => el.complete && el.naturalWidth > 0), 'Broken favicon has no valid replacement');

  await auditAccessibility('chrome-profile');
  await page.locator('.tab.active').focus();
  await page.keyboard.press('Delete');
  check((await ipc(page)).some(item => item.action === 'CloseTab' && item.payload.id === 1), 'Focused tab cannot be closed with the keyboard');

  await page.setViewportSize({ width: 320, height: 760 });
  await loadSurface('sidebar');
  await page.locator('#tabShortcuts').click();
  check(await page.locator('#viewShortcuts.active').count() === 1, 'Shortcut reference tab did not open');
  check(await page.locator('#viewShortcuts').getByText('Ctrl+H').count() === 1, 'History shortcut is missing from the menu help');
  check(await page.locator('.action-item').filter({ hasText: 'Lịch sử' }).getByText('Ctrl+H').count() === 1, 'History shortcut is missing from the browser menu');
  check(await page.locator('.action-item').filter({ hasText: 'Dấu trang' }).getByText('Ctrl+Shift+O').count() === 1, 'Bookmarks shortcut is missing from the browser menu');
  await page.locator('#tabSecurity').click();
  check(await page.locator('#viewSecurity.active').count() === 1, 'Page security tab did not open');
  await page.locator('#tabDownloads').click();
  check(await page.locator('#viewDownloads.active').count() === 1, 'Downloads tab did not open');
  await page.getByRole('button', { name: /Menu trình duyệt/ }).click();
  await page.getByRole('button', { name: 'Lịch sử' }).click();
  messages = await ipc(page);
  check(messages.some(item => item.action === 'OpenLibrary' && item.payload.view === 'history'), 'Menu history action did not reach the host');
  const sidebarOverflow = await page.evaluate(() => document.documentElement.scrollWidth > innerWidth + 1);
  check(!sidebarOverflow, 'Sidebar has horizontal overflow at 320px');
  await page.screenshot({ path: path.join(outputDir, 'sidebar-shortcuts.png'), fullPage: true });
  await auditAccessibility('sidebar-menu');

  await loadSurface('error');
  check(await page.locator('h1').innerText().then(text => text.includes('không tồn tại')), '404 recovery page is missing its explanation');
  check(await page.locator('a[href="evergreen://newtab"]').count() === 1, '404 page has no route back to the Pip home page');
  await page.screenshot({ path: path.join(outputDir, 'error-recovery.png'), fullPage: true });
  await auditAccessibility('error-recovery');

  const seriousAccessibility = accessibility.flatMap(pageResult => pageResult.violations
    .filter(violation => violation.impact === 'serious' || violation.impact === 'critical')
    .map(violation => `${pageResult.page}: ${violation.id} (${violation.nodes.map(node => node.html).join('; ')})`));
  check(seriousAccessibility.length === 0, `Serious or critical accessibility violations: ${seriousAccessibility.join(', ')}`);
  check(errors.length === 0, `Browser UI JavaScript errors: ${errors.join(' | ')}`);
  await browser.close();
  fs.writeFileSync(path.join(outputDir, 'report.json'), JSON.stringify({
    status: 'passed',
    coverage: ['large centered search over mascot background', 'incognito mascot home recommends existing toolbar Google DNS control', 'keyboard operable quick guide and settings explanations', 'responsive and dark new tab/settings', 'valid fallback for broken favicon', 'Google new-tab search and quick link', 'settings IPC for search, Google DNS, downloads and cache cleanup', 'sidebar security/downloads/shortcuts/history', '404 recovery link', 'axe-core accessibility audit'],
    consoleErrors: errors,
    accessibility,
  }, null, 2));
  console.log('PASS: Pip headless UI smoke, interactions, responsive overflow, and JavaScript console checks.');
}

main().catch(error => {
  console.error(error.stack || error.message);
  if (activeBrowser) activeBrowser.close().finally(() => { process.exitCode = 1; });
  else process.exitCode = 1;
});
