const VAULT_PATH = /^\/v1\/vault\/([a-f0-9]{32})$/;
const MAX_PAYLOAD_BYTES = 1024 * 1024;
const NO_STORE_HEADERS = {
  "Cache-Control": "no-store",
  "X-Content-Type-Options": "nosniff",
  "Referrer-Policy": "no-referrer",
};

function response(body, status, extraHeaders = {}) {
  return new Response(body, {
    status,
    headers: { ...NO_STORE_HEADERS, ...extraHeaders },
  });
}

function validPayload(value) {
  return value && value.version === 1
    && typeof value.salt === "string" && /^[a-f0-9]{32}$/.test(value.salt)
    && typeof value.iv === "string" && /^[a-f0-9]{24}$/.test(value.iv)
    && typeof value.ciphertext === "string"
    && /^[a-f0-9]+$/.test(value.ciphertext)
    && value.ciphertext.length >= 32
    && value.ciphertext.length <= MAX_PAYLOAD_BYTES * 2;
}

function hex(value) {
  return [...new Uint8Array(value)].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

function constantTimeEqual(left, right) {
  if (left.length !== right.length) return false;
  let mismatch = 0;
  for (let index = 0; index < left.length; index += 1) mismatch |= left.charCodeAt(index) ^ right.charCodeAt(index);
  return mismatch === 0;
}

async function readLimitedText(request) {
  const declared = Number(request.headers.get("Content-Length"));
  if (Number.isFinite(declared) && declared > MAX_PAYLOAD_BYTES) return null;
  if (!request.body) return "";
  const reader = request.body.getReader();
  const chunks = [];
  let length = 0;
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    length += value.byteLength;
    if (length > MAX_PAYLOAD_BYTES) {
      await reader.cancel();
      return null;
    }
    chunks.push(value);
  }
  const bytes = new Uint8Array(length);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
}

export default {
  async fetch(request, env) {
    if (!["GET", "PUT", "DELETE"].includes(request.method)) {
      return response("Method not allowed", 405, { Allow: "GET, PUT, DELETE" });
    }

    const match = new URL(request.url).pathname.match(VAULT_PATH);
    if (!match) return response("Not found", 404);

    const id = env.VAULTS.idFromName(match[1]);
    return env.VAULTS.get(id).fetch(new Request("https://vault.internal/", request));
  },
};

// Every vault is addressed by a cryptographically random, 256-bit bearer ID.
// The object stores only an opaque AES-GCM envelope. It never receives a key.
export class PipSyncVault {
  constructor(state) {
    this.storage = state.storage;
  }

  async fetch(request) {
    const stored = await this.storage.get("envelope");
    const authorization = request.headers.get("Authorization") || "";
    const token = authorization.startsWith("Bearer ") ? authorization.slice(7) : "";
    if (!/^[a-f0-9]{64}$/.test(token)) return response("Unauthorized", 401);
    const tokenHash = hex(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(token)));
    const savedTokenHash = await this.storage.get("tokenHash");
    if (savedTokenHash && !constantTimeEqual(tokenHash, savedTokenHash)) return response("Unauthorized", 401);
    const revoked = await this.storage.get("revoked");

    if (request.method === "GET") {
      if (revoked) return response("Vault deleted", 410);
      if (!stored) return response("Not found", 404);
      return response(stored.body, 200, {
        "Content-Type": "application/json; charset=utf-8",
        ETag: stored.etag,
      });
    }

    if (request.method === "DELETE") {
      if (revoked) return response(null, 204);
      if (stored) {
        const condition = request.headers.get("If-Match");
        if (condition !== stored.etag && condition !== "*") return response("Conflict", 412);
      }
      await this.storage.delete("envelope");
      if (!savedTokenHash) await this.storage.put("tokenHash", tokenHash);
      // Retain only a one-bit tombstone and the access-token hash. This prevents another
      // device with the old recovery code from silently recreating the deleted vault.
      await this.storage.put("revoked", true);
      return response(null, 204);
    }
    if (revoked) return response("Vault deleted", 410);

    let body;
    try {
      body = await readLimitedText(request);
    } catch {
      return response("Invalid UTF-8", 400);
    }
    if (body === null) return response("Payload too large", 413);
    let envelope;
    try {
      envelope = JSON.parse(body);
    } catch {
      return response("Invalid JSON", 400);
    }
    if (!validPayload(envelope)) return response("Invalid encrypted envelope", 400);

    const condition = request.headers.get("If-Match");
    const create = request.headers.get("If-None-Match") === "*";
    if (stored ? condition !== stored.etag : !create) return response("Conflict", 412);

    const bytes = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(body));
    const etag = `"${hex(bytes)}"`;
    await this.storage.put("envelope", { body, etag });
    if (!savedTokenHash) await this.storage.put("tokenHash", tokenHash);
    return response(null, 204, { ETag: etag });
  }
}
