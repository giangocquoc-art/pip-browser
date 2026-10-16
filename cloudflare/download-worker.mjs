const RELEASE_API = "https://api.github.com/repos/giangocquoc-art/pip-browser/releases/latest";
const RELEASE_CACHE_SECONDS = 120;
const INSTALLER_NAME = /^Pip-Setup-[\w.+-]+-Windows-x64\.exe$/;

function textResponse(body, status) {
  return new Response(body, {
    status,
    headers: {
      "Cache-Control": "no-store",
      "Content-Type": "text/plain; charset=utf-8",
      "X-Content-Type-Options": "nosniff",
    },
  });
}

async function getLatestRelease() {
  const response = await fetch(RELEASE_API, {
    headers: {
      Accept: "application/vnd.github+json",
      "User-Agent": "Pip-Browser-Download",
      "X-GitHub-Api-Version": "2022-11-28",
    },
    cf: { cacheEverything: true, cacheTtl: RELEASE_CACHE_SECONDS },
  });
  if (!response.ok) throw new Error(`Release lookup failed (${response.status})`);
  return response.json();
}

function getAsset(release, pathname) {
  if (pathname === "/checksums") {
    return release.assets?.find((asset) => asset.name === "SHA256SUMS.txt");
  }
  if (pathname === "/" || pathname === "/installer") {
    return release.assets?.find((asset) => INSTALLER_NAME.test(asset.name));
  }
  return null;
}

function copyDownloadHeaders(upstream, asset) {
  const headers = new Headers({
    "Cache-Control": "private, no-store",
    "Content-Disposition": `attachment; filename="${asset.name}"`,
    "Content-Type": asset.name.endsWith(".txt") ? "text/plain; charset=utf-8" : "application/octet-stream",
    "X-Content-Type-Options": "nosniff",
  });
  for (const name of ["accept-ranges", "content-length", "content-range", "etag", "last-modified"]) {
    const value = upstream.headers.get(name);
    if (value) headers.set(name, value);
  }
  return headers;
}

export default {
  async fetch(request) {
    if (request.method !== "GET" && request.method !== "HEAD") {
      return textResponse("Chỉ hỗ trợ tải tệp bằng GET hoặc HEAD.", 405);
    }

    const { pathname } = new URL(request.url);
    try {
      const release = await getLatestRelease();
      const asset = getAsset(release, pathname);
      if (!asset) return textResponse("Không tìm thấy tệp yêu cầu trong bản Pip mới nhất.", 404);

      const assetUrl = new URL(asset.browser_download_url);
      if (
        assetUrl.protocol !== "https:" ||
        assetUrl.hostname !== "github.com" ||
        !assetUrl.pathname.startsWith("/giangocquoc-art/pip-browser/releases/download/")
      ) {
        return textResponse("Địa chỉ tệp phát hành không hợp lệ.", 502);
      }

      const headers = new Headers();
      for (const name of ["if-range", "range"]) {
        const value = request.headers.get(name);
        if (value) headers.set(name, value);
      }
      const upstream = await fetch(assetUrl, { method: request.method, headers, redirect: "follow" });
      if (!upstream.ok) return textResponse("Chưa thể lấy bộ cài từ bản phát hành hiện tại.", 502);

      return new Response(request.method === "HEAD" ? null : upstream.body, {
        status: upstream.status,
        headers: copyDownloadHeaders(upstream, asset),
      });
    } catch {
      return textResponse("Tạm thời chưa lấy được bộ cài Pip mới nhất. Vui lòng thử lại sau.", 503);
    }
  },
};
