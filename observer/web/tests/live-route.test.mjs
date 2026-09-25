import assert from "node:assert/strict";
import test from "node:test";
import { GET } from "../app/api/live/[...path]/route.ts";

test("local Docker proxy preserves streaming, caching, cursor query and cancellation", async (context) => {
  const original = globalThis.fetch; context.after(() => { globalThis.fetch = original; });
  const abort = new AbortController();
  const request = new Request("http://localhost/api/live/v1/subscribe?protocol=2&run_id=one", {signal: abort.signal});
  let forwarded;
  const stream = new ReadableStream({start(controller) { controller.enqueue(new TextEncoder().encode("data: ready\n\n")); }});
  globalThis.fetch = async (url, init) => { forwarded = {url, init}; return new Response(stream, {headers: {"content-type": "text/event-stream", "cache-control": "no-cache"}}); };
  const result = await GET(request, {params: Promise.resolve({path: ["v1", "subscribe"]})});
  assert.equal(result.headers.get("cache-control"), "no-cache");
  assert.equal(forwarded.url.search, "?protocol=2&run_id=one");
  assert.equal(forwarded.init.signal, request.signal);
  const reader = result.body.getReader();
  assert.equal(new TextDecoder().decode((await reader.read()).value), "data: ready\n\n");
  await reader.cancel();
  abort.abort(); assert.equal(forwarded.init.signal.aborted, true);
});

test("proxy rejects game mutation routes before making an upstream request", async (context) => {
  const original = globalThis.fetch; context.after(() => { globalThis.fetch = original; });
  globalThis.fetch = async () => { throw new Error("must not reach upstream"); };
  const response = await GET(new Request("http://localhost/api/live/v1/move"), {params: Promise.resolve({path: ["v1", "move"]})});
  assert.equal(response.status, 404);
});
