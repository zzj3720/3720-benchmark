import assert from "node:assert/strict";
import test from "node:test";

import worker from "../edge-proxy/worker.js";

test("edge proxy forwards only read requests to the tunnel origin", async () => {
  const originalFetch = globalThis.fetch;
  let forwarded;
  globalThis.fetch = async (input, init) => {
    forwarded = { url: input.toString(), init };
    return new Response("ok", { headers: { "content-encoding": "gzip" } });
  };

  try {
    const response = await worker.fetch(
      new Request("https://live.benchmark.3720.org/observe/sausage/v1/observe/snapshot", {
        headers: { "accept-encoding": "gzip" },
      }),
    );
    assert.equal(await response.text(), "ok");
    assert.equal(response.headers.has("content-length"), false);
    assert.equal(response.headers.get("content-encoding"), "gzip");
    assert.equal(
      forwarded.url,
      "https://benchmark-live-origin.3720.org/observe/sausage/v1/observe/snapshot",
    );
    assert.equal(forwarded.init.method, "GET");
    assert.equal(forwarded.init.headers.get("x-forwarded-host"), "live.benchmark.3720.org");
    assert.equal(forwarded.init.headers.get("accept-encoding"), "gzip");
  } finally {
    globalThis.fetch = originalFetch;
  }

  const rejected = await worker.fetch(
    new Request("https://live.benchmark.3720.org/observe/sausage/v1/move", {
      method: "POST",
    }),
  );
  assert.equal(rejected.status, 405);
});

test("edge proxy sends live JSON endpoints directly to the gateway", async () => {
  const originalFetch = globalThis.fetch;
  let forwarded;
  globalThis.fetch = async (input) => {
    forwarded = input.toString();
    return Response.json({ run: { id: "abc" } });
  };

  try {
    const response = await worker.fetch(
      new Request("https://live.benchmark.3720.org/api/live/v1/runs/abc?replay_attempt=3"),
    );
    assert.equal(
      forwarded,
      "https://benchmark-live-origin.3720.org/v1/runs/abc?replay_attempt=3",
    );
    assert.equal(response.headers.get("cache-control"), "no-store");
    assert.equal((await response.json()).run.id, "abc");
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("edge proxy streams live subscriptions without buffering", async () => {
  const originalFetch = globalThis.fetch;
  let forwarded;
  globalThis.fetch = async (input) => {
    forwarded = input.toString();
    const response = new Response("data: {\"runs\":[]}\n\n", {
      headers: { "content-type": "text/event-stream" },
    });
    response.arrayBuffer = () => {
      throw new Error("subscription response was buffered");
    };
    return response;
  };

  try {
    const response = await worker.fetch(
      new Request("https://live.benchmark.3720.org/api/live/subscribe?run_id=abc"),
    );
    assert.equal(forwarded, "https://benchmark-live-origin.3720.org/v1/subscribe?run_id=abc");
    assert.match(response.headers.get("content-type") ?? "", /^text\/event-stream/);
    assert.equal(await response.text(), 'data: {"runs":[]}\n\n');
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("edge proxy caches HTML at the edge without making browsers retain a stale release", async () => {
  const originalFetch = globalThis.fetch;
  const originalCaches = globalThis.caches;
  let fetched = 0;
  let stored;
  const pending = [];
  globalThis.fetch = async () => {
    fetched += 1;
    return new Response("<html>release</html>", {
      headers: { "content-type": "text/html", "cache-control": "no-store" },
    });
  };
  globalThis.caches = {
    default: {
      match: async () => stored?.response.clone(),
      put: async (key, response) => {
        stored = { key: key.url, response: response.clone() };
      },
    },
  };
  const request = new Request("https://live.benchmark.3720.org/", {
    headers: { accept: "text/html" },
  });
  const context = { waitUntil(promise) { pending.push(promise); } };

  try {
    const first = await worker.fetch(request, {}, context);
    await Promise.all(pending);
    const second = await worker.fetch(request, {}, context);
    assert.equal(await first.text(), "<html>release</html>");
    assert.equal(await second.text(), "<html>release</html>");
    assert.equal(fetched, 1);
    assert.match(stored.key, /__edge=2026-07-22-v5/);
    assert.equal(stored.response.headers.get("cache-control"), "public, max-age=15, stale-while-revalidate=300");
    assert.equal(second.headers.get("cache-control"), "public, max-age=0, must-revalidate");
  } finally {
    globalThis.fetch = originalFetch;
    if (originalCaches === undefined) delete globalThis.caches;
    else globalThis.caches = originalCaches;
  }
});
