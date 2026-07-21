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
      new Request("https://live.benchmark.3720.org/observe/sausage/v1/observe/snapshot"),
    );
    assert.equal(await response.text(), "ok");
    assert.equal(response.headers.get("content-length"), "2");
    assert.equal(response.headers.has("content-encoding"), false);
    assert.equal(
      forwarded.url,
      "https://benchmark-live-origin.3720.org/observe/sausage/v1/observe/snapshot",
    );
    assert.equal(forwarded.init.method, "GET");
    assert.equal(forwarded.init.headers.get("x-forwarded-host"), "live.benchmark.3720.org");
    assert.equal(forwarded.init.headers.has("accept-encoding"), false);
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
