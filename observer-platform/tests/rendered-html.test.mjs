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
  const sausageSceneState = await readFile(
    new URL("../../games/sausage-roll/observer/scene-state.ts", import.meta.url),
    "utf8",
  );
  const qaGalleryHtml = await readFile(new URL("../qa-gallery.html", import.meta.url), "utf8");
  const qaGallery = await readFile(new URL("../qa-gallery-entry.tsx", import.meta.url), "utf8");
  const qaGalleryStyles = await readFile(new URL("../qa-gallery.css", import.meta.url), "utf8");
  assert.match(page, /gatewayUrl\("\/v1\/subscribe"\)/);
  assert.match(page, /hydrateRunAssets/);
  assert.match(page, /\/v1\/assets\//);
  assert.match(page, /EventSource/);
  assert.doesNotMatch(page, /setTimeout\(refresh|fetch\("\/api\/live/);
  assert.match(page, /ScoreChart/);
  assert.match(page, /SCORE \/ EFFECTIVE AGENT TIME/);
  assert.match(page, /横轴累计有效运行时间 · 纵轴得分/);
  assert.match(page, /last_score_elapsed_ms/);
  assert.match(page, /未得分 \{scoreSilence === null \? "全程"/);
  assert.match(page, /运行 \{durationLabel\(elapsed\)\}/);
  assert.match(page, /const RUN_STATUS/);
  assert.match(page, /STOP · 可续跑/);
  assert.match(page, /STOP · AGENT 主动/);
  assert.match(page, /run\.termination\?\.kind/);
  assert.match(page, /function WallClock/);
  assert.doesNotMatch(page, /const \[now, setNow\]/);
  assert.match(page, /className="model-title"[\s\S]*?run\.model[\s\S]*?run\.effort/);
  assert.doesNotMatch(page, /\bFINAL\b|timeAgo\(|活动 \{timeAgo|得分 \{timeAgo/);
  assert.match(page, /模型分数随累计有效运行时间变化图/);
  assert.match(page, /environment-card[\s\S]*?<GameState[\s\S]*?<ReplayTimeline/);
  assert.match(page, /className="replay-panel"/);
  assert.doesNotMatch(page, /className="replay-card"/);
  assert.doesNotMatch(page, /replayFrames|from "\.\/replay"/);
  assert.match(page, /@radix-ui\/react-scroll-area/);
  assert.match(page, /@radix-ui\/react-toggle/);
  assert.match(page, /lucide-react/);
  assert.doesNotMatch(page, /连续操作 #\$\{frame\.operation_sequence\} · 指令/);
  assert.match(page, /跳过失败尝试（以重置为分界）/);
  assert.match(page, /aria-label="选择回放段"/);
  assert.match(page, /MapIcon/);
  assert.match(page, /大地图路段/);
  assert.match(page, /className="score-replay-list"/);
  assert.match(page, /onClick=\{\(\) => onAttemptReplay\(attempt\.id\)\}/);
  assert.match(page, /score-replay-level \$\{group\.kind\}/);
  assert.doesNotMatch(page, /className="score-select"/);
  assert.doesNotMatch(page, /SCORE REPLAYS|按关卡选择尝试/);
  assert.doesNotMatch(page, /operation-card|WAITING FOR EVENT|Sidecar 尚未产生可回放状态/);
  assert.match(page, /frames\.length > 0 && <div className="replay-toolbar">/);
  assert.match(page, /className="replay-inline-scrubber"/);
  assert.doesNotMatch(page, /className="replay-scrubber"|className="replay-note"/);
  assert.doesNotMatch(page, /AUTHORITATIVE ATTEMPT|个有效画面|已跳过|失败尝试以 restart\/reset 为界|正向得分会封口/);
  assert.match(page, /environmentTitle/);
  assert.match(page, /gameStateContext/);
  assert.match(page, /replay_attempt/);
  assert.doesNotMatch(page, /SCORE \/ WALL CLOCK/);
  assert.match(page, /此次尝试的 Agent 活动/);
  assert.match(page, /此次尝试的操作/);
  assert.match(page, /不再展示整段 session/);
  assert.match(page, /不再展示整条事件流/);
  assert.match(page, /状态回放时间轴/);
  assert.match(page, /label=\{playing \? "暂停回放" : "播放回放"\}/);
  assert.match(page, /const \[speed, setSpeed\] = useState\(1\)/);
  assert.match(page, /450 \/ speed/);
  assert.match(page, /<option value=\{0\.5\}>0\.5<\/option>/);
  assert.match(page, /返回直播/);
  assert.match(page, /Agent 经验板/);
  assert.match(page, /不展示隐藏推理/);
  assert.match(page, /function selectRun[\s\S]*?setDetail\(null\)[\s\S]*?setSelectedId/);
  assert.match(page, /followingLive[\s\S]*?返回直播/);
  assert.doesNotMatch(page, /SIDECAR TRACE|最近事件|Agent 当前活动/);
  assert.match(registry, /games\/parabox-intro\/observer/);
  assert.match(registry, /games\/emergency-operator\/observer/);
  assert.match(registry, /games\/sausage-roll\/observer/);
  assert.match(registry, /games\/kitchen-terminal\/observer/);
  assert.match(operator, /Emergency Operator/);
  assert.match(operator, /OperatorState/);
  assert.doesNotMatch(parabox, /CONTAINER PATH|当前容器路径|完整场景/);
  assert.match(parabox, /stateContext/);
  assert.match(parabox, /parabox-observer-scene-v1/);
  assert.doesNotMatch(parabox, /OUTER SPACE/);
  assert.match(parabox, /内部空间已渲染/);
  assert.match(parabox, /盒子内部与外层场景未记录/);
  assert.doesNotMatch(parabox, /parabox-legend|场景图例/);
  assert.doesNotMatch(paraboxStyles, /\.parabox-legend/);
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
  assert.match(sausage, /FOLLOW PLAYER/);
  assert.match(sausage, /FULL MAP/);
  assert.match(sausageScene, /OrbitControls/);
  assert.match(sausageScene, /activityPoints\(this\.state\)/);
  assert.match(sausageScene, /this\.camera\.position\.add\(delta\)/);
  assert.match(sausageScene, /CapsuleGeometry\(0\.36, 1\.08/);
  assert.match(sausageScene, /terrainTop\(tile\.pos\)/);
  assert.match(sausageScene, /entity\.cells\.length > 1/);
  assert.match(sausageScene, /heldFork\.position\.set\(0, 0\.72, 0\.5\)/);
  assert.match(sausageScene, /function addFork\(parent: THREE\.Group\)/);
  assert.doesNotMatch(sausageScene, /function addFork\([^)]*held/);
  assert.doesNotMatch(sausage, /changedEntityIds|previousState/);
  assert.doesNotMatch(sausageScene, /changedIds|emissiveIntensity: changed/);
  assert.match(sausageScene, /new THREE\.PerspectiveCamera\(60/);
  assert.match(sausageScene, /addEntrances\(this\.staticRoot, state\)/);
  assert.match(sausageScene, /staticSignature\(state\)/);
  assert.match(sausageScene, /state\.mode === "overworld"/);
  assert.match(sausageScene, /orientForward\(group, entrance\.direction\)/);
  assert.match(sausageScene, /new THREE\.ConeGeometry\(0\.2, 0\.38, 4\)/);
  assert.match(sausageSceneState, /state\.overworld_map/);
  assert.match(sausage, /ALL ENTRANCES · ARROWS SHOW FACING/);
  assert.match(sausageSceneState, /tile\.variant === -1/);
  assert.match(page, /withSharedGameState/);
  assert.match(paraboxStyles, /\.parabox-cell\s*\{[\s\S]*?box-shadow: none/);
  assert.match(qaGalleryHtml, /src="\/qa-gallery-entry\.tsx"/);
  assert.doesNotMatch(qaGalleryHtml, /src="\/qa-gallery\.tsx"/);
  assert.match(qaGallery, /import \{ ParaboxState \}/);
  assert.match(qaGallery, /visible\.map/);
  assert.match(qaGallery, /samples\.slice\(page \* PAGE_SIZE/);
  assert.match(qaGalleryStyles, /grid-template-columns: repeat\(4/);
  assert.match(qaGalleryStyles, /content-visibility: auto/);
  assert.match(parabox, /describeParaboxEvent/);
  assert.doesNotMatch(parabox, /changedSceneCells|data-legend="changed"|相较上一步有变化/);
  assert.doesNotMatch(paraboxStyles, /parabox-cell\.changed|parabox-change/);
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
