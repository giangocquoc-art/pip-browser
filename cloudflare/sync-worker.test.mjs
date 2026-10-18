import assert from "node:assert/strict";
import test from "node:test";
import worker, { PipSyncVault } from "./sync-worker.mjs";

const id = "a".repeat(32);
const token = "e".repeat(64);
const envelope = JSON.stringify({ version: 1, salt: "b".repeat(32), iv: "c".repeat(24), ciphertext: "d".repeat(64) });

class MemoryStorage {
  values = new Map();
  tail = Promise.resolve();
  async get(key) {
    if (Array.isArray(key)) return new Map(key.flatMap((entryKey) => this.values.has(entryKey) ? [[entryKey, this.values.get(entryKey)]] : []));
    return this.values.get(key);
  }
  async put(key, value) {
    if (typeof key === "object") {
      for (const [entryKey, entryValue] of Object.entries(key)) this.values.set(entryKey, entryValue);
      return;
    }
    this.values.set(key, value);
  }
  async delete(key) { return this.values.delete(key); }
  async transaction(callback) {
    const previous = this.tail;
    let release;
    this.tail = new Promise((resolve) => { release = resolve; });
    await previous;
    try {
      return await callback(this);
    } finally {
      release();
    }
  }
}

function testEnv() {
  const objects = new Map();
  return {
    VAULTS: {
      idFromName(name) { return name; },
      get(name) {
        if (!objects.has(name)) objects.set(name, new PipSyncVault({ storage: new MemoryStorage() }));
        return objects.get(name);
      },
    },
  };
}

function auth(headers = {}) {
  return { Authorization: `Bearer ${token}`, ...headers };
}

test("stores an opaque encrypted envelope and returns a revision", async () => {
  const env = testEnv();
  const created = await worker.fetch(new Request(`https://sync.psharehub.com/v1/vault/${id}`, {
    method: "PUT", headers: auth({ "If-None-Match": "*", "Content-Type": "application/json" }), body: envelope,
  }), env);
  assert.equal(created.status, 204);
  const etag = created.headers.get("ETag");
  assert.match(etag, /^"[a-f0-9]{64}"$/);
  const fetched = await worker.fetch(new Request(`https://sync.psharehub.com/v1/vault/${id}`, { headers: auth() }), env);
  assert.equal(fetched.status, 200);
  assert.equal(fetched.headers.get("ETag"), etag);
  assert.equal(await fetched.text(), envelope);
  assert.equal(fetched.headers.get("Cache-Control"), "no-store");
});

test("rejects stale writes instead of silently overwriting a newer device", async () => {
  const env = testEnv();
  const url = `https://sync.psharehub.com/v1/vault/${id}`;
  await worker.fetch(new Request(url, { method: "PUT", headers: auth({ "If-None-Match": "*" }), body: envelope }), env);
  const stale = await worker.fetch(new Request(url, { method: "PUT", headers: auth({ "If-Match": '"stale"' }), body: envelope }), env);
  assert.equal(stale.status, 412);
});

test("serializes concurrent conditional writes so only one revision wins", async () => {
  const env = testEnv();
  const url = `https://sync.psharehub.com/v1/vault/${id}`;
  const created = await worker.fetch(new Request(url, {
    method: "PUT", headers: auth({ "If-None-Match": "*" }), body: envelope,
  }), env);
  const etag = created.headers.get("ETag");
  const nextEnvelope = JSON.stringify({ version: 1, salt: "f".repeat(32), iv: "1".repeat(24), ciphertext: "2".repeat(64) });
  const writes = await Promise.all([
    worker.fetch(new Request(url, { method: "PUT", headers: auth({ "If-Match": etag }), body: nextEnvelope }), env),
    worker.fetch(new Request(url, { method: "PUT", headers: auth({ "If-Match": etag }), body: envelope }), env),
  ]);
  assert.deepEqual(writes.map((result) => result.status).sort(), [204, 412]);
});

test("rejects weak vault IDs, malformed envelopes, and unsupported methods", async () => {
  const env = testEnv();
  assert.equal((await worker.fetch(new Request("https://sync.psharehub.com/v1/vault/abc"), env)).status, 404);
  assert.equal((await worker.fetch(new Request(`https://sync.psharehub.com/v1/vault/${id}`, { method: "PUT", headers: auth(), body: "{}" }), env)).status, 400);
  assert.equal((await worker.fetch(new Request(`https://sync.psharehub.com/v1/vault/${id}`, { method: "POST" }), env)).status, 405);
});

test("requires the private sync token and stores only its hash", async () => {
  const env = testEnv();
  const url = `https://sync.psharehub.com/v1/vault/${id}`;
  const created = await worker.fetch(new Request(url, { method: "PUT", headers: auth({ "If-None-Match": "*" }), body: envelope }), env);
  assert.equal(created.status, 204);
  const wrongToken = "f".repeat(64);
  assert.equal((await worker.fetch(new Request(url, { headers: { Authorization: `Bearer ${wrongToken}` } }), env)).status, 401);
  assert.equal((await worker.fetch(new Request(url, { method: "PUT", headers: { ...auth({ "If-Match": created.headers.get("ETag") }), Authorization: `Bearer ${wrongToken}` }, body: envelope }), env)).status, 401);
  const stored = await env.VAULTS.get(id).storage.get("tokenHash");
  assert.equal(stored, await crypto.subtle.digest("SHA-256", new TextEncoder().encode(token)).then((hash) => [...new Uint8Array(hash)].map((byte) => byte.toString(16).padStart(2, "0")).join("")));
  assert.notEqual(stored, token);
});

test("limits request bodies before storing large values", async () => {
  const env = testEnv();
  const oversized = JSON.stringify({ version: 1, salt: "b".repeat(32), iv: "c".repeat(24), ciphertext: "d".repeat(1024 * 1024 * 2) });
  const response = await worker.fetch(new Request(`https://sync.psharehub.com/v1/vault/${id}`, {
    method: "PUT", headers: auth({ "If-None-Match": "*" }), body: oversized,
  }), env);
  assert.equal(response.status, 413);
});

test("permanently deletes a vault, revokes old devices, and requires a new recovery code", async () => {
  const env = testEnv();
  const url = `https://sync.psharehub.com/v1/vault/${id}`;
  const created = await worker.fetch(new Request(url, { method: "PUT", headers: auth({ "If-None-Match": "*" }), body: envelope }), env);
  const etag = created.headers.get("ETag");
  assert.equal((await worker.fetch(new Request(url, { method: "DELETE", headers: auth({ "If-Match": '"stale"' }) }), env)).status, 412);
  assert.equal((await worker.fetch(new Request(url, { method: "DELETE", headers: auth({ "If-Match": etag }) }), env)).status, 204);
  assert.equal((await worker.fetch(new Request(url, { headers: auth() }), env)).status, 410);
  assert.equal((await worker.fetch(new Request(url, { method: "PUT", headers: auth({ "If-None-Match": "*" }), body: envelope }), env)).status, 410);
  assert.equal((await worker.fetch(new Request(url, { method: "DELETE", headers: auth() }), env)).status, 204);
});

