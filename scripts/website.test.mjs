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

test("mobile layout keeps navigation and hero copy within the viewport", async () => {
  const homepage = await readFile(path.join(root, "index.html"), "utf8");
  const styles = await readFile(path.join(root, "styles.css"), "utf8");
  assert.match(homepage, /<nav aria-label="Điều hướng chính">/);
  assert.match(styles, /@media\(max-width:600px\)\{[\s\S]*?\.topbar nav\{grid-column:1\/-1;grid-row:2;display:flex;flex-wrap:wrap/);
  assert.match(styles, /\.showcase h1\{display:block;width:100%;max-width:100%;font-size:clamp\(28px,8vw,33px\);[^}]*overflow-wrap:anywhere/);
  assert.match(styles, /\.showcase-head>div:first-child\{align-self:stretch;width:100%;min-width:0\}/);
  assert.match(styles, /\.brand span\{display:none\}/);
});

test("privacy and help pages describe only released sync and DNS behavior", async () => {
  const privacy = await readFile(path.join(root, "quyen-rieng-tu/index.html"), "utf8");
  const faq = await readFile(path.join(root, "cau-hoi/index.html"), "utf8");
  assert.match(privacy, /Phiên bản Pip tải công khai hiện tại chưa có Pip Sync/);
  assert.match(privacy, /Bản công khai hiện tại chưa có đồng bộ đám mây/);
  assert.doesNotMatch(privacy, /Nếu bạn chủ động bật Pip Sync/);
  assert.match(faq, /khởi động lại Pip để áp dụng/);
});

test("feature page documents the available UX features and their limits", async () => {
  const features = await readFile(path.join(root, "tinh-nang/index.html"), "utf8");
  assert.match(features, /Nhóm tab và khôi phục phiên/);
  assert.match(features, /Dịch trang trên thiết bị/);
  assert.match(features, /Dùng Microsoft Edge WebView2/);
  assert.doesNotMatch(features, /WebView2 có sẵn trên Windows/);
  assert.match(features, /Đồng bộ Pip tùy chọn/);
  assert.match(features, /dấu trang, lịch sử, tab thường và một số cài đặt/);
  assert.match(features, /mật khẩu do WebView2 lưu không được đồng bộ/);
  assert.match(features, /kho mật khẩu riêng được bảo vệ bằng Windows DPAPI/);
});

test("unreleased feature claims identify their required public release", async () => {
  const features = await readFile(path.join(root, "tinh-nang/index.html"), "utf8");
  assert.match(features, /id="feature-release-status"/);
  assert.equal((features.match(/data-release-version="0\.1\.11"/g) || []).length, 2);
  assert.equal((features.match(/data-release-version="0\.1\.12"/g) || []).length, 2);
  assert.equal((features.match(/data-release-version="[^"]+" hidden/g) || []).length, 4);
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
    const sections = ["0.1.11", "0.1.12"].map((releaseVersion) => ({
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
  assert.deepEqual(oldRelease.sections.map(({ hidden }) => hidden), [true, true]);
  assert.match(oldRelease.status.textContent, /0\.1\.11 hoặc 0\.1\.12/);
  const currentRelease = await evaluate("0.1.13");
  assert.deepEqual(currentRelease.sections.map(({ hidden }) => hidden), [false, false]);
  assert.equal(currentRelease.status.hidden, true);
});
