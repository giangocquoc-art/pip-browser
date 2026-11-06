import assert from "node:assert/strict";
import test from "node:test";
import worker from "./download-worker.mjs";

const installer = {
  name: "Pip-Setup-0.1.1-Windows-x64.exe",
  browser_download_url: "https://github.com/giangocquoc-art/pip-browser/releases/download/v0.1.1/Pip-Setup-0.1.1-Windows-x64.exe",
};
const checksums = {
  name: "SHA256SUMS.txt",
  browser_download_url: "https://github.com/giangocquoc-art/pip-browser/releases/download/v0.1.1/SHA256SUMS.txt",
};

async function withMockedFetch(mock, action) {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = mock;
  try {
    return await action();
  } finally {
    globalThis.fetch = originalFetch;
  }
}

function releaseResponse(assets = [installer, checksums]) {
  return new Response(JSON.stringify({ tag_name: "v0.1.1", assets }), {
    headers: { "Content-Type": "application/json" },
  });
}

test("streams the latest installer without redirecting the visitor to GitHub", async () => {
  const seen = [];
  const response = await withMockedFetch(async (input, init) => {
    seen.push({ input: String(input), init });
    if (String(input).includes("api.github.com")) return releaseResponse();
    return new Response("installer-bytes", { headers: { "Content-Length": "15", "Accept-Ranges": "bytes" } });
  }, () => worker.fetch(new Request("https://download.pip.psharehub.com/")));

  assert.equal(response.status, 200);
  assert.equal(response.headers.get("Content-Disposition"), 'attachment; filename="PipSetup.exe"');
  assert.equal(response.headers.get("Content-Type"), "application/octet-stream");
  assert.equal(response.headers.get("Cache-Control"), "private, no-store");
  assert.equal(await response.text(), "installer-bytes");
  assert.equal(seen.length, 2);
  assert.match(seen[1].input, /github\.com\/giangocquoc-art\/pip-browser\/releases\/download/);
});

test("serves the matching SHA-256 manifest as a direct download", async () => {
  const response = await withMockedFetch(async (input) => {
    if (String(input).includes("api.github.com")) return releaseResponse();
    return new Response(`${"a".repeat(64)}  Pip-Setup-0.1.1-Windows-x64.exe`);
  }, () => worker.fetch(new Request("https://download.pip.psharehub.com/checksums")));

  assert.equal(response.status, 200);
  assert.equal(response.headers.get("Content-Disposition"), 'attachment; filename="SHA256SUMS.txt"');
  assert.match(await response.text(), new RegExp(`${"a".repeat(64)}  PipSetup\\.exe`));
});

test("rejects unsupported paths and methods", async () => {
  const seen = [];
  const responses = await withMockedFetch(async (input) => {
    seen.push(String(input));
    throw new Error("Invalid paths and methods must not contact GitHub");
  }, async () => Promise.all([
    worker.fetch(new Request("https://download.pip.psharehub.com/unknown")),
    worker.fetch(new Request("https://download.pip.psharehub.com/", { method: "POST" })),
  ]));
  const [pathResponse, methodResponse] = responses;
  assert.equal(pathResponse.status, 404);
  assert.equal(methodResponse.status, 405);
  assert.equal(seen.length, 0);
});

test("rejects an asset URL outside the Pip GitHub repository", async () => {
  const response = await withMockedFetch(async (input) => {
    if (String(input).includes("api.github.com")) {
      return releaseResponse([{ name: installer.name, browser_download_url: "https://attacker.example/setup.exe" }]);
    }
    throw new Error("The invalid URL must not be fetched");
  }, () => worker.fetch(new Request("https://download.pip.psharehub.com/")));
  assert.equal(response.status, 502);
});
