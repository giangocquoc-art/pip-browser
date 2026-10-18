import assert from "node:assert/strict";
import { readdir, readFile, stat } from "node:fs/promises";
import path from "node:path";
import test from "node:test";
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

test("homepage fallback version matches the public version manifest", async () => {
  const homepage = await readFile(path.join(root, "index.html"), "utf8");
  const manifest = JSON.parse(await readFile(path.join(root, "version.json"), "utf8"));
  const fallbackVersion = homepage.match(/id="version">([^<]+)</)?.[1];
  assert.ok(fallbackVersion, "homepage version fallback is missing");
  assert.equal(fallbackVersion, manifest.version);
});

test("feature page documents the available UX features and their limits", async () => {
  const features = await readFile(path.join(root, "tinh-nang/index.html"), "utf8");
  assert.match(features, /Nhóm tab và khôi phục phiên/);
  assert.match(features, /Dịch trang trên thiết bị/);
  assert.match(features, /Đồng bộ dấu trang tùy chọn/);
  assert.match(features, /Sync không bao gồm lịch sử, mật khẩu, cookie, dữ liệu website hoặc phiên tab/);
  assert.match(features, /Pip không có kho mật khẩu riêng/);
});
