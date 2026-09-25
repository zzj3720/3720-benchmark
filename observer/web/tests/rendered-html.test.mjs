import assert from "node:assert/strict";
import { access } from "node:fs/promises";
import test from "node:test";

const root = new URL("../", import.meta.url);

async function render() {
  const workerUrl = new URL("../dist/server/index.js", import.meta.url);
  workerUrl.searchParams.set("test", `${process.pid}-${Date.now()}`);
  const { default: worker } = await import(workerUrl.href);
  return worker.fetch(
    new Request("https://observer.example/", {
      headers: {
        accept: "text/html",
        host: "observer.example",
        "x-forwarded-host": "observer.example",
        "x-forwarded-proto": "https",
      },
    }),
    {
      ASSETS: {
        fetch: async () => new Response("Not found", { status: 404 }),
      },
    },
    {
      waitUntil() {},
      passThroughOnException() {},
    },
  );
}

test("server-renders the benchmark operations console", async () => {
  const response = await render();
  assert.equal(response.status, 200);
  assert.match(response.headers.get("content-type") ?? "", /^text\/html\b/i);

  const html = await response.text();
  assert.match(html, /<title>3720 Live Operations<\/title>/i);
  assert.match(html, /Benchmark Live/);
  assert.match(html, /Patrick/);
  assert.match(html, />SWARM</);
  assert.match(html, />SAUSAGE</);
  assert.match(html, />OPERATOR</);
  assert.match(html, /https:\/\/observer\.example\/og\.png/);
  assert.doesNotMatch(html, /codex-preview|Your site is taking shape/);
});

test("renders range and comparison controls without search", async () => {
  const html = await (await render()).text();
  assert.match(html, /筛选实验记录/);
  assert.match(html, /最近 8 次/);
  assert.match(html, /aria-label="运行范围"/);
  assert.match(html, /role="combobox"/);
  assert.doesNotMatch(html, /<input[^>]*type="search"/);
  assert.match(html, /选择对比曲线/);
  assert.match(html, /最多 8 条/);
  assert.doesNotMatch(html, /DEMO_STATES|DEMO FIXTURE/);
  await access(new URL("../dist/standalone/server.js", import.meta.url));
  await access(new URL("../public/og.png", import.meta.url));
});

test("has no per-game sidecar proxy", async () => {
  // All live data flows through the Worker's read API (worker/live-api.ts).
  await assert.rejects(access(new URL("../app/observe", import.meta.url)));
  await assert.rejects(access(new URL("../app/api/live", import.meta.url)));
});
