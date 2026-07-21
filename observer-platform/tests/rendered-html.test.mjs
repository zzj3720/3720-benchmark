import assert from "node:assert/strict";
import { access, readFile } from "node:fs/promises";
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

test("implements a real multi-run scoreboard and drill-down", async () => {
  const page = await readFile(new URL("../app/page.tsx", import.meta.url), "utf8");
  const registry = await readFile(new URL("../app/game-registry.tsx", import.meta.url), "utf8");
  const operator = await readFile(
    new URL("../../games/emergency-operator/observer/index.tsx", import.meta.url),
    "utf8",
  );
  const parabox = await readFile(
    new URL("../../games/parabox-intro/observer/index.tsx", import.meta.url),
    "utf8",
  );
  const paraboxStyles = await readFile(
    new URL("../../games/parabox-intro/observer/styles.css", import.meta.url),
    "utf8",
  );
  const sausage = await readFile(
    new URL("../../games/sausage-roll/observer/index.tsx", import.meta.url),
    "utf8",
  );
  const sausageScene = await readFile(
    new URL("../../games/sausage-roll/observer/scene.ts", import.meta.url),
    "utf8",
  );
  const qaGalleryHtml = await readFile(new URL("../qa-gallery.html", import.meta.url), "utf8");
  const qaGallery = await readFile(new URL("../qa-gallery-entry.tsx", import.meta.url), "utf8");
  const qaGalleryStyles = await readFile(new URL("../qa-gallery.css", import.meta.url), "utf8");
  assert.match(page, /\/api\/live\/subscribe/);
  assert.match(page, /EventSource/);
  assert.doesNotMatch(page, /setTimeout\(refresh|fetch\("\/api\/live/);
  assert.match(page, /ScoreChart/);
  assert.match(page, /SCORE \/ EFFECTIVE AGENT TIME/);
  assert.match(page, /横轴累计有效运行时间 · 纵轴得分/);
  assert.match(page, /模型分数随累计有效运行时间变化图/);
  assert.doesNotMatch(page, /SCORE \/ WALL CLOCK/);
  assert.match(page, /Agent 当前活动/);
  assert.match(page, /状态回放时间轴/);
  assert.match(page, /播放回放/);
  assert.match(page, /返回直播/);
  assert.match(page, /Agent 经验板/);
  assert.match(page, /不展示隐藏推理/);
  assert.match(page, /function selectRun[\s\S]*?setDetail\(null\)[\s\S]*?setSelectedId/);
  assert.match(page, /followingLive[\s\S]*?返回直播/);
  assert.match(page, /SIDECAR TRACE/);
  assert.match(registry, /games\/parabox-intro\/observer/);
  assert.match(registry, /games\/emergency-operator\/observer/);
  assert.match(registry, /games\/sausage-roll\/observer/);
  assert.match(registry, /games\/kitchen-terminal\/observer/);
  assert.match(operator, /Emergency Operator/);
  assert.match(operator, /OperatorState/);
  assert.match(parabox, /CONTAINER PATH/);
  assert.match(parabox, /当前容器路径/);
  assert.match(parabox, /parabox-observer-scene-v1/);
  assert.doesNotMatch(parabox, /OUTER SPACE/);
  assert.match(parabox, /内部空间已渲染/);
  assert.match(parabox, /盒子内部与外层场景未记录/);
  assert.match(parabox, /data-wall-top/);
  assert.match(parabox, /focusContainer \? null : kind === "player"/);
  assert.match(parabox, /MIN_RECURSIVE_SCALE = 1 \/ 512/);
  assert.match(parabox, /scale \/ nestedSpan >= MIN_RECURSIVE_SCALE && depth < 12/);
  assert.match(parabox, /scale \/ Math\.max\(width, height\)/);
  assert.match(parabox, /gridTemplateColumns: `repeat\(\$\{width\}, \$\{100 \/ span\}%\)`/);
  assert.match(parabox, /parentSpan - parentWidth/);
  assert.match(parabox, /parentSpan - parentHeight/);
  assert.doesNotMatch(parabox, /parabox-cycle-reference|ancestry/);
  assert.match(paraboxStyles, /\.parabox-box-face\s*\{[\s\S]*?width: 100%/);
  assert.match(paraboxStyles, /\.parabox-box-face\s*\{[\s\S]*?border: 0/);
  assert.match(paraboxStyles, /\.parabox-box-face > \.parabox-grid/);
  assert.doesNotMatch(parabox, /parabox-box-interior/);
  assert.doesNotMatch(paraboxStyles, /parabox-box-interior/);
  assert.doesNotMatch(paraboxStyles, /\.parabox-box-face::before/);
  assert.match(paraboxStyles, /\.parabox-focus-space\s*\{[\s\S]*?width: 100%;[\s\S]*?height: 100%/);
  assert.doesNotMatch(parabox, /focusSpan|\* 82/);
  assert.doesNotMatch(paraboxStyles, /border: clamp\(5px, 0\.7vw, 9px\)/);
  assert.doesNotMatch(paraboxStyles, /parabox-focus-space[^}]*drop-shadow/);
  assert.match(sausage, /import\("\.\/scene"\)/);
  assert.match(sausage, /每根香肠的四面状态/);
  assert.match(sausageScene, /CapsuleGeometry\(0\.36, 1\.08/);
  assert.match(sausageScene, /terrainTop\(tile\.pos\)/);
  assert.match(sausageScene, /entity\.cells\.length > 1/);
  assert.match(sausageScene, /heldFork\.position\.set\(0, 0\.72, 0\.5\)/);
  assert.match(sausageScene, /function addFork\(parent: THREE\.Group\)/);
  assert.doesNotMatch(sausageScene, /function addFork\([^)]*held/);
  assert.match(sausage, /if \(!previousState\) return new Set<number>\(\)/);
  assert.match(sausageScene, /new THREE\.PerspectiveCamera\(60/);
  assert.match(paraboxStyles, /\.parabox-cell\s*\{[\s\S]*?box-shadow: none/);
  assert.match(qaGalleryHtml, /src="\/qa-gallery-entry\.tsx"/);
  assert.doesNotMatch(qaGalleryHtml, /src="\/qa-gallery\.tsx"/);
  assert.match(qaGallery, /import \{ ParaboxState \}/);
  assert.match(qaGallery, /visible\.map/);
  assert.match(qaGallery, /samples\.slice\(page \* PAGE_SIZE/);
  assert.match(qaGalleryStyles, /grid-template-columns: repeat\(4/);
  assert.match(qaGalleryStyles, /content-visibility: auto/);
  assert.match(parabox, /describeParaboxEvent/);
  assert.match(parabox, /相较上一步有变化/);
  assert.doesNotMatch(page, /JSON\.stringify\(\{ action:/);
  assert.doesNotMatch(page, /DEMO_STATES|DEMO FIXTURE/);
  await assert.rejects(access(new URL("../app/_sites-preview", import.meta.url)));
  await access(new URL("../public/og.png", import.meta.url));
  await access(new URL(".openai/hosting.json", root));
});

test("keeps the public observer gateway read-only", async () => {
  const route = await readFile(
    new URL("../app/observe/[source]/[...path]/route.ts", import.meta.url),
    "utf8",
  );
  assert.match(route, /v1\/observe\/events/);
  assert.match(route, /v1\/observe\/snapshot/);
  assert.doesNotMatch(route, /v1\/(move|undo|restart|submit)/);
  assert.match(route, /observer endpoint is read-only/);

  const liveRoute = await readFile(
    new URL("../app/api/live/[...path]/route.ts", import.meta.url),
    "utf8",
  );
  assert.match(liveRoute, /v1\\\/runs/);
  assert.match(liveRoute, /LIVE_GATEWAY_ORIGIN/);
  assert.match(liveRoute, /live endpoint is read-only/);
  assert.doesNotMatch(liveRoute, /POST|PUT|PATCH|DELETE/);
});
