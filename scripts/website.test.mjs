import assert from "node:assert/strict";
import { readdir, readFile, stat } from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { runInNewContext } from "node:vm";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const websiteRoots = [
  "index.html",
  "tinh-nang",
  "cau-hoi",
  "quyen-rieng-tu",
  "tin-cay",
  "huong-dan",
];

async function collectHtml(entry) {
  const fullPath = path.join(root, entry);
  const info = await stat(fullPath);
  if (info.isFile()) return [fullPath];
  const children = await readdir(fullPath, { withFileTypes: true });
  const nested = await Promise.all(
    children
      .filter((child) => child.isDirectory() || child.name.endsWith(".html"))
      .map((child) => collectHtml(path.relative(root, path.join(fullPath, child.name)))),
  );
  return nested.flat();
}

const pages = (await Promise.all(websiteRoots.map(collectHtml))).flat();

function resolveLocalReference(sourceFile, reference) {
  if (!reference || reference.startsWith("#") || reference.startsWith("//")) return null;
  const url = new URL(reference, "https://pip.local/");
  if (url.origin !== "https://pip.local") return null;
  const pathname = decodeURIComponent(url.pathname);
  const resolved = path.resolve(root, `.${pathname}`);
  assert.ok(resolved === root || resolved.startsWith(`${root}${path.sep}`), `${sourceFile}: path escapes website root: ${reference}`);
  return resolved;
}

async function existingTarget(referencePath) {
  if (!referencePath) return true;
  const candidates = [referencePath, path.join(referencePath, "index.html")];
  for (const candidate of candidates) {
    try {
      if ((await stat(candidate)).isFile()) return true;
    } catch {
      // Try the directory index fallback.
    }
  }
  return false;
}

test("public pages have valid canonical URLs and local links/assets", async () => {
  for (const file of pages) {
    const source = await readFile(file, "utf8");
    const relative = path.relative(root, file);
    const canonical = source.match(/<link\s+rel=["']canonical["']\s+href=["']([^"']+)["']/i)?.[1];
    assert.ok(canonical, `${relative}: canonical URL is missing`);

    for (const match of source.matchAll(/\b(?:href|src)=["']([^"']+)["']/gi)) {
      const reference = match[1];
      const target = resolveLocalReference(relative, reference);
      assert.ok(await existingTarget(target), `${relative}: local reference does not exist: ${reference}`);
    }
  }
});

test("sitemap routes resolve to pages with matching canonical URLs", async () => {
  const sitemap = await readFile(path.join(root, "sitemap.xml"), "utf8");
  const urls = [...sitemap.matchAll(/<loc>([^<]+)<\/loc>/g)].map((match) => match[1]);
  assert.ok(urls.includes("https://pip.psharehub.com/tin-cay/"), "trust page is missing from sitemap");
  for (const url of urls) {
    const parsed = new URL(url);
    const target = path.resolve(root, `.${parsed.pathname}`);
    const htmlPath = (await stat(target)).isDirectory() ? path.join(target, "index.html") : target;
    const source = await readFile(htmlPath, "utf8");
    assert.ok(source.includes(`<link rel="canonical" href="${url}">`), `${url}: canonical URL does not match sitemap`);
  }
});

test("homepage download CTA stays visible and trust page clearly states signature status", async () => {
  const homepage = await readFile(path.join(root, "index.html"), "utf8");
  assert.match(homepage, /id="download-link"[^>]+https:\/\/download\.pip\.psharehub\.com\//);
  assert.match(homepage, /href="\/tin-cay\/"/);

  const trustPage = await readFile(path.join(root, "tin-cay/index.html"), "utf8");
  assert.match(trustPage, /chưa có chữ ký Authenticode/);
  assert.match(trustPage, /artifact attestation/i);
  assert.match(trustPage, /SHA256SUMS\.txt/);
});

test("homepage explains WebView2 setup accurately and exposes keyboard focus", async () => {
  const homepage = await readFile(path.join(root, "index.html"), "utf8");
  const styles = await readFile(path.join(root, "styles.css"), "utf8");
  assert.match(homepage, /Nếu máy chưa có Runtime, bộ cài sẽ hướng dẫn/);
  assert.doesNotMatch(homepage, /WebView2 có sẵn trên Windows/);
  assert.match(styles, /:where\(a,button,\[tabindex\]\):focus-visible\s*\{[^}]*outline:\s*3px solid var\(--deep-orange\)/);
});

test("homepage fallback version matches the public version manifest", async () => {
  const homepage = await readFile(path.join(root, "index.html"), "utf8");
  const manifest = JSON.parse(await readFile(path.join(root, "version.json"), "utf8"));
  const fallbackVersion = homepage.match(/id="version">([^<]+)</)?.[1];
  assert.ok(fallbackVersion, "homepage version fallback is missing");
  assert.equal(fallbackVersion, manifest.version);
});

test("fallback release manifest has a valid version, installer, SHA-256 and size", async () => {
  const manifest = JSON.parse(await readFile(path.join(root, "version.json"), "utf8"));
  assert.match(manifest.version ?? "", /^\d+\.\d+\.\d+$/);
  assert.equal(manifest.downloadAvailable, true);
  assert.equal(manifest.installer, "PipSetup.exe");
  assert.match(manifest.url ?? "", /^https:\/\/download\.pip\.psharehub\.com\/$/);
  assert.match(manifest.sha256 ?? "", /^[a-f0-9]{64}$/i);
  assert.ok(Number.isSafeInteger(manifest.size) && manifest.size > 0, "installer size must be a positive safe integer");
  assert.match(manifest.updated ?? "", /^\d{4}-\d{2}-\d{2}$/);
});

test("homepage preview tabs support keyboard navigation", async () => {
  const homepage = await readFile(path.join(root, "index.html"), "utf8");
  const script = await readFile(path.join(root, "app.js"), "utf8");
  assert.match(homepage, /role="tablist"/);
  assert.match(homepage, /role="tabpanel"/);
  assert.match(script, /event\.key === 'ArrowRight'/);
  assert.match(script, /event\.key === 'ArrowLeft'/);
  assert.match(script, /event\.key === 'Home'/);
  assert.match(script, /event\.key === 'End'/);
  assert.match(script, /item\.tabIndex = active \? 0 : -1/);
});

test("all dynamically selected preview images exist", async () => {
  const script = await readFile(path.join(root, "app.js"), "utf8");
  for (const view of ["browser", "private", "menu", "settings"]) {
    const asset = script.match(new RegExp(`\\b${view}:\\s*\\['([^']+)'`))?.[1];
    assert.ok(asset, `${view}: preview asset is missing from app.js`);
    const target = resolveLocalReference("app.js", asset);
    assert.ok(target, `${view}: preview asset must be a local website asset`);
    assert.ok((await stat(target)).isFile(), `${view}: preview asset does not exist: ${asset}`);
  }
});

test("mobile layout keeps navigation and hero copy within the viewport", async () => {
  const homepage = await readFile(path.join(root, "index.html"), "utf8");
  const styles = await readFile(path.join(root, "styles.css"), "utf8");
  assert.match(homepage, /<nav aria-label="Điều hướng chính">/);
  assert.match(styles, /@media\(max-width:600px\)\{[\s\S]*?\.topbar nav\{grid-column:1\/-1;grid-row:2;display:flex;flex-wrap:nowrap;gap:6px 14px;width:100%;min-width:0;overflow-x:auto;overflow-y:hidden;padding:5px 2px 9px;white-space:nowrap;scrollbar-width:thin;scroll-padding-inline-end:28px/);
  assert.match(styles, /\.showcase h1\{display:block;width:100%;max-width:100%;font-size:clamp\(28px,8vw,33px\);[^}]*overflow-wrap:anywhere/);
  assert.match(styles, /\.showcase-head>div:first-child\{align-self:stretch;width:100%;min-width:0\}/);
  assert.match(styles, /\.brand span\{display:none\}/);
});

test("privacy and help pages disclose optional sync and Google DNS fallback behavior", async () => {
  const privacy = await readFile(path.join(root, "quyen-rieng-tu/index.html"), "utf8");
  const faq = await readFile(path.join(root, "cau-hoi/index.html"), "utf8");
  assert.match(privacy, /Pip Sync có sẵn nhưng mặc định tắt/);
  assert.match(privacy, /mã hóa trên thiết bị trước khi gửi tới dịch vụ lưu trữ trên Cloudflare/);
  assert.match(privacy, /lịch sử chỉ nằm trong gói đồng bộ đã mã hóa nếu bạn chủ động bật mục này/);
  assert.match(privacy, /cả truy vấn A và AAAA.*lỗi tạm thời/);
  assert.match(privacy, /mạng Windows có thể thấy hostname/);
  assert.doesNotMatch(privacy, /chưa có Pip Sync|chưa có đồng bộ đám mây/);
  assert.match(faq, /khởi động lại Pip để áp dụng/);
});

test("feature page documents the available UX features and their limits", async () => {
  const features = await readFile(path.join(root, "tinh-nang/index.html"), "utf8");
  assert.match(features, /Nhóm tab và khôi phục phiên/);
  assert.match(features, /Dịch trang trên thiết bị/);
  assert.match(features, /Dùng Microsoft Edge WebView2/);
  assert.doesNotMatch(features, /WebView2 có sẵn trên Windows/);
  assert.match(features, /Đồng bộ Pip tùy chọn/);
  assert.match(features, /bạn có thể bật riêng lịch sử, tab đang mở, một số cài đặt và mật khẩu trong kho Pip/);
  assert.match(features, /Lịch sử, tab, cài đặt và mật khẩu mặc định tắt/);
  assert.match(features, /mật khẩu do WebView2 lưu và thông tin thanh toán không được đồng bộ/);
  assert.match(features, /kho mật khẩu riêng được bảo vệ bằng Windows DPAPI/);
});

test("unreleased feature claims identify their required public release", async () => {
  const features = await readFile(path.join(root, "tinh-nang/index.html"), "utf8");
  assert.match(features, /id="feature-release-status"/);
  assert.equal((features.match(/data-release-version="0\.1\.11"/g) || []).length, 2);
  assert.equal((features.match(/data-release-version="0\.1\.13"/g) || []).length, 2);
  assert.equal((features.match(/data-release-version="0\.1\.14"/g) || []).length, 1);
  assert.equal((features.match(/data-release-version="[^"]+" hidden/g) || []).length, 5);
  assert.match(features, /fetch\('\/version\.json'/);
  assert.doesNotMatch(features, /Pip không có kho mật khẩu riêng/);
  assert.match(features, /kho mật khẩu riêng được bảo vệ bằng Windows DPAPI/);
});

test("feature page only reveals features included in the public version", async () => {
  const source = await readFile(path.join(root, "tinh-nang/index.html"), "utf8");
  const script = source.match(/<script>\s*([\s\S]*?)<\/script>/i)?.[1];
  assert.ok(script, "feature release gate script is missing");

  async function evaluate(version) {
    const status = { hidden: false, textContent: "" };
    const sections = ["0.1.11", "0.1.13", "0.1.14"].map((releaseVersion) => ({
      dataset: { releaseVersion }, hidden: true,
    }));
    const document = {
      getElementById: () => status,
      querySelectorAll: () => sections,
    };
    const fetch = async () => ({ ok: true, json: async () => ({ version }) });
    runInNewContext(script, { document, fetch, Set, Error });
    await new Promise((resolve) => setImmediate(resolve));
    return { status, sections };
  }

  const oldRelease = await evaluate("0.1.9");
  assert.deepEqual(oldRelease.sections.map(({ hidden }) => hidden), [true, true, true]);
  assert.match(oldRelease.status.textContent, /0\.1\.11 hoặc 0\.1\.13 hoặc 0\.1\.14/);
  const currentRelease = await evaluate("0.1.13");
  assert.deepEqual(currentRelease.sections.map(({ hidden }) => hidden), [false, false, true]);
  assert.match(currentRelease.status.textContent, /0\.1\.14/);
  const nextRelease = await evaluate("0.1.14");
  assert.deepEqual(nextRelease.sections.map(({ hidden }) => hidden), [false, false, false]);
  assert.equal(nextRelease.status.hidden, true);
});

test("app preview images retain their intrinsic proportions on mobile", async () => {
  const styles = await readFile(path.join(root, "styles.css"), "utf8");
  const previewRules = [...styles.matchAll(/\.app-preview img\{([^}]+)\}/g)]
    .map((match) => match[1]);
  assert.ok(previewRules.length > 0, "app preview image styles are missing");
  for (const declarations of previewRules) {
    assert.match(declarations, /(?:^|;)height:auto(?:;|$)/);
    assert.doesNotMatch(declarations, /(?:^|;)aspect-ratio\s*:/);
  }
});
