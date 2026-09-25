import assert from "node:assert/strict";
import { gunzipSync, gzipSync } from "node:zlib";
import test from "node:test";
import { handleLive } from "../worker/live-api.ts";

// Workers-only globals the handler relies on.
crypto.subtle.timingSafeEqual ??= (left, right) =>
  left.byteLength === right.byteLength && left.every((byte, index) => byte === right[index]);

function environment() {
  const objects = new Map();
  const hubRequests = [];
  const bucket = {
    objects,
    async put(key, body, options) { objects.set(key, { bytes: new Uint8Array(body), options }); },
    async get(key) {
      const stored = objects.get(key);
      if (!stored) return null;
      return {
        body: new Blob([stored.bytes]).stream(),
        arrayBuffer: async () => stored.bytes.buffer.slice(stored.bytes.byteOffset, stored.bytes.byteOffset + stored.bytes.byteLength),
        httpEtag: '"etag"',
        httpMetadata: stored.options.httpMetadata,
      };
    },
  };
  const hub = {
    idFromName: name => name,
    get: () => ({
      fetch: async (url, init = {}) => {
        const body = init.body ? await new Response(init.body).text() : null;
        hubRequests.push({ url: String(url), method: init.method ?? "GET", body });
        return Response.json({ ok: true, path: new URL(url).pathname });
      },
    }),
  };
  return { env: { LIVE_BUCKET: bucket, LIVE_HUB: hub, LIVE_INGEST_TOKEN: "secret" }, objects, hubRequests };
}

const context = { waitUntil() {} };

function frame(entries) {
  const manifest = Buffer.from(JSON.stringify(entries.map(({ body, ...entry }) => ({ ...entry, bytes: body.length }))));
  const header = Buffer.alloc(4);
  header.writeUInt32BE(manifest.length);
  return Buffer.concat([header, manifest, ...entries.map(entry => entry.body)]);
}

test("ingest stores framed bodies and the read API serves them with their encoding", async () => {
  const { env, objects } = environment();
  const detail = gzipSync(JSON.stringify({ schema: "benchmark-live-run-v1", run: { id: "run-1" } }));
  const replay = gzipSync(JSON.stringify({ attempt_id: 4 }));
  const response = await handleLive(new Request("https://live/api/ingest/objects", {
    method: "POST",
    headers: { authorization: "Bearer secret" },
    body: frame([
      { key: "pub/runs/run-1/detail.json", content_type: "application/json", encoding: "gzip", cache_control: "no-store", body: detail },
      { key: "pub/runs/run-1/attempts/4.preview.json", content_type: "application/json", encoding: "gzip", cache_control: "public, max-age=31536000, immutable", body: replay },
    ]),
  }), env, context);
  assert.equal(response.status, 200, await response.clone().text());
  assert.equal(objects.size, 2);

  const plain = await handleLive(new Request("https://live/api/live/v1/runs/run-1"), env, context);
  assert.equal(plain.headers.get("content-encoding"), null);
  assert.equal((await plain.json()).run.id, "run-1");

  const read = await handleLive(new Request("https://live/api/live/v1/runs/run-1", { headers: { "accept-encoding": "gzip, br" } }), env, context);
  assert.equal(read.headers.get("content-encoding"), "gzip");
  assert.equal(read.headers.get("cache-control"), "no-store");
  assert.equal(JSON.parse(gunzipSync(Buffer.from(await read.arrayBuffer()))).run.id, "run-1");

  const page = await handleLive(new Request("https://live/api/live/v1/runs/run-1?replay_attempt=4&preview=1", { headers: { "accept-encoding": "gzip" } }), env, context);
  assert.equal(page.status, 200);
  assert.match(page.headers.get("cache-control"), /immutable/);
  const missing = await handleLive(new Request("https://live/api/live/v1/runs/run-1?replay_attempt=4"), env, context);
  assert.equal(missing.status, 404);
});

test("ingest rejects missing or wrong tokens and unsafe keys", async () => {
  const { env, objects } = environment();
  const body = frame([{ key: "pub/../x", content_type: "a", encoding: null, cache_control: "no-store", body: Buffer.from("x") }]);
  for (const authorization of [undefined, "Bearer wrong", "secret"]) {
    const headers = authorization ? { authorization } : {};
    const response = await handleLive(new Request("https://live/api/ingest/objects", { method: "POST", headers, body }), env, context);
    assert.equal(response.status, 401);
  }
  const unsafe = await handleLive(new Request("https://live/api/ingest/objects", { method: "POST", headers: { authorization: "Bearer secret" }, body }), env, context);
  assert.equal(unsafe.status, 400);
  assert.equal(objects.size, 0);
});

test("gzipped run batches reach the hub decoded; the read side is GET-only", async () => {
  const { env, hubRequests } = environment();
  const message = { order: ["a"], runs: [], removed: [] };
  const response = await handleLive(new Request("https://live/api/ingest/runs", {
    method: "POST",
    headers: { authorization: "Bearer secret", "x-body-encoding": "gzip" },
    body: gzipSync(JSON.stringify(message)),
  }), env, context);
  assert.equal(response.status, 200);
  assert.deepEqual(JSON.parse(hubRequests[0].body), message);
  assert.equal(new URL(hubRequests[0].url).pathname, "/runs");

  const post = await handleLive(new Request("https://live/api/live/v1/runs", { method: "POST", body: "{}" }), env, context);
  assert.equal(post.status, 405);
  const other = await handleLive(new Request("https://live/api/live/v1/move"), env, context);
  assert.equal(other.status, 404);
  assert.equal(await handleLive(new Request("https://live/"), env, context), null);
  const invalid = await handleLive(new Request("https://live/api/live/v1/subscribe?run_id=../x"), env, context);
  assert.equal(invalid.status, 400);
});

test("level indexes are served per game and keys are validated", async () => {
  const { env } = environment();
  await env.LIVE_BUCKET.put("pub/games/parabox/levels.json", new TextEncoder().encode('{"levels":[]}'), { httpMetadata: { contentType: "application/json", cacheControl: "no-store" } });
  await env.LIVE_BUCKET.put("pub/games/parabox/levels/0123456789abcdef.json", new TextEncoder().encode('{"runs":[]}'), { httpMetadata: { contentType: "application/json", cacheControl: "no-store" } });
  assert.deepEqual(await (await handleLive(new Request("https://live/api/live/v1/games/parabox/levels"), env, context)).json(), { levels: [] });
  assert.deepEqual(await (await handleLive(new Request("https://live/api/live/v1/games/parabox/levels/0123456789abcdef"), env, context)).json(), { runs: [] });
  assert.equal((await handleLive(new Request("https://live/api/live/v1/games/parabox/levels/../x"), env, context)).status, 404);
  assert.equal((await handleLive(new Request("https://live/api/live/v1/games/parabox/levels/nothex"), env, context)).status, 400);
});
