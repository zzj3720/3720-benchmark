// Public read API and authenticated ingest for the Cloudflare-hosted live
// console. Bodies live in R2 exactly as the local publisher projected them;
// run summaries and the subscription stream live in the LiveHub object.

export interface LiveEnv {
  LIVE_BUCKET: R2Bucket;
  LIVE_HUB: DurableObjectNamespace;
  LIVE_INGEST_TOKEN?: string;
}

const SAFE_ID = /^[A-Za-z0-9._-]+$/;
const ASSET_ID = /^[0-9a-f]{64}$/;
const DIGITS = /^\d+$/;
const UPLOAD_CONCURRENCY = 6;

// The project type-checks against both DOM and Workers declarations; these
// are the Workers-only members the DOM types do not know about.
type Background = { waitUntil(promise: Promise<unknown>): void };
const timingSafeEqual = (left: Uint8Array, right: Uint8Array) =>
  (crypto.subtle as unknown as { timingSafeEqual(a: Uint8Array, b: Uint8Array): boolean }).timingSafeEqual(left, right);

export async function handleLive(request: Request, env: LiveEnv, ctx: Background): Promise<Response | null> {
  const url = new URL(request.url);
  if (url.pathname.startsWith("/api/ingest/")) return ingest(request, env, url.pathname.slice("/api/ingest/".length));
  if (!url.pathname.startsWith("/api/live/")) return null;
  if (request.method !== "GET" && request.method !== "HEAD") return error(405, "live endpoint is read-only");
  const path = url.pathname.slice("/api/live/".length);
  if (path === "health") return hub(env, "/health");
  if (path === "v1/runs") return hub(env, "/runs");
  if (path === "v1/subscribe") {
    const target = new URL("https://hub/subscribe");
    const selected = url.searchParams.get("run_id");
    if (selected !== null) {
      if (!SAFE_ID.test(selected)) return error(400, "invalid run id");
      target.searchParams.set("run_id", selected);
    }
    return stub(env).fetch(target.toString(), { signal: request.signal });
  }
  const asset = /^v1\/assets\/([^/]+)$/.exec(path);
  if (asset) {
    if (!ASSET_ID.test(asset[1])) return error(400, "invalid asset id");
    return body(env, request, `pub/assets/${asset[1]}`, "unknown asset");
  }
  // Covers are baked by scripts/bake-covers.mjs; a missing one is a 404 and the page renders it.
  const cover = /^v1\/covers\/([^/]+)\/(\d+)$/.exec(path);
  if (cover) {
    if (!SAFE_ID.test(cover[1])) return error(400, "invalid run id");
    return body(env, request, `pub/covers/${cover[1]}/${cover[2]}.webp`, "no cover yet");
  }
  const levels = /^v1\/games\/([^/]+)\/levels(?:\/([^/]+))?$/.exec(path);
  if (levels) {
    const [, game, key] = levels;
    if (!SAFE_ID.test(game) || (key !== undefined && !/^[0-9a-f]{16}$/.test(key))) return error(400, "invalid level");
    return body(env, request, key ? `pub/games/${game}/levels/${key}.json` : `pub/games/${game}/levels.json`, "unknown level");
  }
  const run = /^v1\/runs\/([^/]+)$/.exec(path);
  if (!run) return error(404, "live endpoint is read-only");
  const id = run[1];
  if (!SAFE_ID.test(id)) return error(400, "invalid run id");
  const attempt = url.searchParams.get("replay_attempt");
  const before = url.searchParams.get("catalog_before");
  if (attempt !== null) {
    // One body per attempt; `preview` is its first frame alone, for level thumbnails.
    if (!DIGITS.test(attempt)) return error(400, "invalid replay attempt");
    const part = url.searchParams.get("preview") === "1" ? ".preview" : "";
    return body(env, request, `pub/runs/${id}/attempts/${attempt}${part}.json`, "unknown replay attempt");
  }
  if (before !== null) {
    if (!DIGITS.test(before)) return error(400, "invalid catalog cursor");
    return body(env, request, `pub/runs/${id}/catalog/${before}.json`, "unknown run");
  }
  return body(env, request, `pub/runs/${id}/detail.json`, "unknown run");
}

function stub(env: LiveEnv) {
  return env.LIVE_HUB.get(env.LIVE_HUB.idFromName("live"));
}

function hub(env: LiveEnv, path: string, init?: RequestInit) {
  return stub(env).fetch(`https://hub${path}`, init);
}

/**
 * Serve a stored body. Bodies are stored gzip-encoded and passed through
 * as-is. They are not put in the Cache API: it stores manual-encoded bytes as
 * the decoded body and re-compresses them on a hit.
 */
async function body(env: LiveEnv, request: Request, key: string, missing: string) {
  const object = await env.LIVE_BUCKET.get(key);
  if (!object) return error(404, missing);
  const headers = new Headers({
    "content-type": object.httpMetadata?.contentType ?? "application/json; charset=utf-8",
    "cache-control": object.httpMetadata?.cacheControl ?? "no-store",
    "access-control-allow-origin": "*",
    etag: object.httpEtag,
  });
  const encoding = object.httpMetadata?.contentEncoding;
  if (encoding === "gzip" && !/\bgzip\b/.test(request.headers.get("accept-encoding") ?? "")) {
    // Cloudflare would drop Content-Encoding for this client but pass the
    // manual-encoded bytes through, so decode them here instead.
    return new Response(object.body.pipeThrough(new DecompressionStream("gzip")), { headers });
  }
  if (encoding) headers.set("content-encoding", encoding);
  return new Response(object.body, { headers, encodeBody: "manual" });
}

async function ingest(request: Request, env: LiveEnv, route: string) {
  if (request.method !== "POST") return error(405, "ingest requires POST");
  if (!(await authorized(request, env))) return error(401, "unauthorized");
  if (route === "objects") {
    try {
      return await putObjects(request, env);
    } catch (failure) {
      return error(400, failure instanceof Error ? failure.message : "invalid object frame");
    }
  }
  if (route === "runs" || route === "heartbeat") {
    const source = request.headers.get("x-body-encoding") === "gzip"
      ? request.body?.pipeThrough(new DecompressionStream("gzip"))
      : request.body;
    return hub(env, `/${route}`, { method: "POST", body: source, headers: { "content-type": "application/json" } });
  }
  return error(404, "unknown ingest route");
}

async function authorized(request: Request, env: LiveEnv) {
  const expected = env.LIVE_INGEST_TOKEN;
  const header = request.headers.get("authorization") ?? "";
  if (!expected || !header.startsWith("Bearer ")) return false;
  const encoder = new TextEncoder();
  const [left, right] = [encoder.encode(header.slice(7)), encoder.encode(expected)];
  const digest = async (bytes: Uint8Array<ArrayBuffer>) => new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
  // Compare fixed-length digests so the comparison does not leak the length.
  return timingSafeEqual(await digest(left), await digest(right));
}

type Manifest = { key: string; bytes: number; content_type: string; encoding: string | null; cache_control: string };

/** Frame: u32 big-endian manifest length, manifest JSON, then each body in order. */
async function putObjects(request: Request, env: LiveEnv) {
  const frame = new Uint8Array(await request.arrayBuffer());
  if (frame.byteLength < 4) return error(400, "truncated frame");
  const length = new DataView(frame.buffer, frame.byteOffset, 4).getUint32(0);
  const manifest = JSON.parse(new TextDecoder().decode(frame.subarray(4, 4 + length))) as Manifest[];
  let offset = 4 + length;
  const uploads = manifest.map(entry => {
    if (!/^(pub|raw)\/[A-Za-z0-9._\/-]+$/.test(entry.key) || entry.key.includes("..")) throw new Error(`invalid key ${entry.key}`);
    const part = frame.subarray(offset, offset + entry.bytes);
    offset += entry.bytes;
    return { entry, part };
  });
  if (offset !== frame.byteLength) return error(400, "frame length mismatch");
  for (let index = 0; index < uploads.length; index += UPLOAD_CONCURRENCY) {
    await Promise.all(uploads.slice(index, index + UPLOAD_CONCURRENCY).map(({ entry, part }) =>
      env.LIVE_BUCKET.put(entry.key, part, {
        httpMetadata: {
          contentType: entry.content_type,
          contentEncoding: entry.encoding ?? undefined,
          cacheControl: entry.cache_control,
        },
      })));
  }
  return Response.json({ ok: true, stored: uploads.length });
}

function error(status: number, message: string) {
  return Response.json({ error: message }, { status, headers: { "cache-control": "no-store" } });
}
