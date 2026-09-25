"use client";
import { canvasExportSource, type CanvasExportSession } from "./webgl/export-source";

import * as ScrollArea from "@radix-ui/react-scroll-area";
import * as Toggle from "@radix-ui/react-toggle";
import {
  Check,
  ChevronLeft,
  ChevronRight,
  ChevronsLeft,
  ChevronsRight,
  CircleSlash2,
  FileImage,
  Film,
  Map as MapIcon,
  Pause,
  Play,
  Radio,
  Rewind,
  type LucideIcon,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore, type ButtonHTMLAttributes } from "react";

import {
  GAME_IDS,
  GAME_META,
  GameState,
  describeGameEvent,
  gameStateContext,
  resolveGameFrameState,
  type GameId,
} from "./game-registry";
import {
  EmptyState,
  Metric,
  REPLAY_CAPTURE_ATTRIBUTE,
  asString,
  type Json,
  type ObserverEvent,
} from "./game-observer";
import { LiveActionMenu, LiveDisclosure, LiveSelect, LiveSlider } from "./live-controls";
import type { ReplayExportFormat } from "./replay-export";

import type { RunSummary, RunDetail, ElapsedScorePoint, ReplayFrame, ReplayGroupSummary, LoadedAttemptReplay } from "./live-contract";
import { LiveResource, RequestGate, applySubscription, effectiveDuration } from "./live-client";
import { harnessName, modelName, rankRuns, runLabels, seriesColors, viewerStatus } from "./run-labels";

function clockTime(timestamp?: number | null) {
  if (!timestamp) return "—";
  return new Intl.DateTimeFormat("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  }).format(timestamp);
}

function fileSlug(value: string) {
  return value
    .normalize("NFKD")
    .replace(/[^\w.-]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .toLowerCase();
}


function taskDuration(run: RunSummary, now: number) {
  if (typeof run.consumed_ms === "number") {
    return effectiveDuration(run, now);
  }
  const history = run.score_history ?? [];
  const elapsed = history.map((point) => point.elapsed_ms ?? 0);
  if (elapsed.some((value) => value > 0)) return Math.max(...elapsed);
  const timestamps = history.map((point) => point.timestamp_ms);
  const startedAt = Math.min(run.started_at ?? Number.POSITIVE_INFINITY, ...timestamps);
  if (!Number.isFinite(startedAt)) return 0;
  const lastHistoryAt = timestamps[timestamps.length - 1] ?? startedAt;
  const endedAt = run.live ? now : (run.finished_at ?? lastHistoryAt);
  return Math.max(0, lastHistoryAt - startedAt, endedAt - startedAt);
}

function durationLabel(durationMs: number, axisMaxMs = durationMs) {
  if (axisMaxMs < 60 * 60 * 1000) return `${Math.round(durationMs / 60_000)}m`;
  return `${(durationMs / (60 * 60 * 1000)).toFixed(1)}h`;
}

function noScoreDuration(run: RunSummary, now: number) {
  if (typeof run.last_score_elapsed_ms !== "number") return null;
  return Math.max(0, taskDuration(run, now) - run.last_score_elapsed_ms);
}

function scoreTimeline(run: RunSummary, now: number): ElapsedScorePoint[] {
  const history = run.score_history ?? [];
  const source = history.length
    ? history
    : [{ timestamp_ms: run.started_at ?? now, score: run.score }];
  const startedAt = Math.min(run.started_at ?? Number.POSITIVE_INFINITY, source[0].timestamp_ms);
  const points = source.map((point) => ({
    elapsed_ms: point.elapsed_ms ?? Math.max(0, point.timestamp_ms - startedAt),
    score: point.score,
  }));
  const durationMs = taskDuration(run, now);
  const last = points[points.length - 1];
  return durationMs > last.elapsed_ms
    ? [...points, { elapsed_ms: durationMs, score: last.score }]
    : points;
}

const STATUS_CLASS = { live: "live", done: "completed", ended: "finished", waiting: "waiting" } as const;

function runStatus(run: RunSummary) {
  const status = viewerStatus(run);
  return { ...status, className: STATUS_CLASS[status.tone] };
}

function sinceLastScore(run: RunSummary, now: number) {
  if (typeof run.last_score_elapsed_ms !== "number") return "没有得分";
  if (!run.live) return `最后一分在第 ${durationLabel(run.last_score_elapsed_ms)}`;
  const silence = noScoreDuration(run, now) ?? 0;
  return silence < 60_000 ? "刚刚得分" : `${durationLabel(silence)}前得分`;
}

function actionLabel(action?: Record<string, Json> | null) {
  const command = asString(action?.command, "STATE").toUpperCase();
  const direction = asString(action?.direction, "");
  return direction ? `${command} ${direction.toUpperCase()}` : command;
}

function instructionLabel(frame: ReplayFrame) {
  if (frame.has_instruction_trace && frame.operation_size > 1) {
    return `${frame.instruction_index} / ${frame.instruction_count}`;
  }
  if (!frame.has_instruction_trace && frame.operation_size > 1) return "整组";
  return "单个";
}

type Feed = { connected: boolean; last_seen_ms: number | null };

/** Once the local publisher is gone, time stops where its last heartbeat was. */
function feedNow(now: number, feed: Feed | null) {
  return feed && !feed.connected && feed.last_seen_ms !== null ? Math.min(now, feed.last_seen_ms) : now;
}

function gatewayUrl(path: string) {
  const origin = import.meta.env.VITE_LIVE_GATEWAY_ORIGIN;
  return new URL(origin ? `${origin}${path}` : `/api/live${path}`, window.location.origin);
}

// Assets are content-addressed and immutable: parse each id once per session
// instead of re-fetching and re-parsing megabytes of JSON on every live tick.
const assetCache = new Map<string, { promise: Promise<Json>; bytes: number }>();
const ASSET_CACHE_BYTES = 16 * 1024 * 1024;

function fetchAsset(id: string, decodedBytes = 0) {
  const cached = assetCache.get(id);
  if (cached) { assetCache.delete(id); assetCache.set(id, cached); return cached.promise; }
  const promise = fetch(gatewayUrl(`/v1/assets/${id}`), {cache: "force-cache", signal: AbortSignal.timeout(15_000)}).then(async response => {
    if (!response.ok) throw new Error(`asset ${id}: HTTP ${response.status}`);
    return await response.json() as Json;
  });
  const bytes = Math.max(decodedBytes, 64 * 1024) * 4;
  if (bytes <= ASSET_CACHE_BYTES) {
    while ([...assetCache.values()].reduce((total, entry) => total + entry.bytes, 0) + bytes > ASSET_CACHE_BYTES) {
      const oldest = assetCache.keys().next().value;
      if (oldest === undefined) break;
      assetCache.delete(oldest);
    }
    assetCache.set(id, {promise, bytes});
    promise.catch(() => { if (assetCache.get(id)?.promise === promise) assetCache.delete(id); });
  }
  return promise;
}

async function hydrateRunAssets(detail: RunDetail) {
  const entries = Object.entries(detail.asset_refs ?? {});
  if (!entries.length) return detail;
  const assets = await Promise.all(
    entries.map(async ([name, reference]) => [name, await fetchAsset(reference.id, reference.bytes)] as const),
  );
  return { ...detail, state: { ...detail.state, ...Object.fromEntries(assets) } };
}

// Keep the previous state object when every value is reference-identical, so
// the memoized GameState (and the three.js scene) is not rebuilt on ticks
// where only score or experience changed.
function reuseUnchangedState(previous: RunDetail | null, next: RunDetail): RunDetail {
  if (!previous || previous.id !== next.id) return next;
  if (next.state_revision && next.state_revision === previous.state_revision) return { ...next, state: previous.state };
  const before = previous.state ?? {};
  const after = next.state ?? {};
  const keys = Object.keys(after);
  if (
    keys.length === Object.keys(before).length &&
    keys.every((key) => before[key] === after[key])
  ) {
    return { ...next, state: before };
  }
  return next;
}

export default function Home() {
  const [runs, setRuns] = useState<RunSummary[]>([]);
  const [selectedGame, setSelectedGame] = useState<GameId>(GAME_IDS[0]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [detail, setDetail] = useState<RunDetail | null>(null);
  const [connection, setConnection] = useState<"connecting" | "live" | "offline">("connecting");
  const [paused, setPaused] = useState(false);
  const [snapshotNow, setSnapshotNow] = useState(() => Date.now());
  const [detailStatus, setDetailStatus] = useState<"loading" | "ready" | "error">("loading");
  const [detailError, setDetailError] = useState("");
  const [feedError, setFeedError] = useState("");
  const [feedRetry, setFeedRetry] = useState(0);
  const [lastReceivedAt, setLastReceivedAt] = useState<number | null>(null);
  const [clockOffset, setClockOffset] = useState(0);
  const [feed, setFeed] = useState<Feed | null>(null);
  const runsRef = useRef<RunSummary[]>([]);
  const detailResource = useRef<LiveResource<RunDetail> | null>(null);
  const contentRef = useRef<HTMLElement>(null);

  useEffect(() => {
    if (paused || connection !== "live") return;
    const timer = window.setInterval(() => setSnapshotNow(feedNow(Date.now() + clockOffset, feed)), 1000);
    return () => window.clearInterval(timer);
  }, [paused, connection, clockOffset, feed]);

  useEffect(() => {
    const syncFromLocation = () => {
      const params = new URL(window.location.href).searchParams;
      const runId = params.get("run");
      const game = params.get("game");
      contentRef.current?.scrollTo({ top: 0 });
      window.scrollTo({ top: 0 });
      setDetail(current => current?.id === runId ? current : null);
      setSelectedId(runId);
      setSelectedGame(
        runsRef.current.find(run => run.id === runId)?.game ??
        ((GAME_IDS as string[]).includes(game ?? "") ? (game as GameId) : GAME_IDS[0]),
      );
    };
    queueMicrotask(syncFromLocation);
    window.addEventListener("popstate", syncFromLocation);
    return () => window.removeEventListener("popstate", syncFromLocation);
  }, []);

  useEffect(() => {
    if (paused) return;
    const url = gatewayUrl("/v1/subscribe");
    url.searchParams.set("protocol", "2");
    if (selectedId) url.searchParams.set("run_id", selectedId);
    const subscription = new EventSource(url);
    let appliedRevision = -1;
    let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
    const resource = new LiveResource<RunDetail>({
      load: async (signal) => {
        const response = await fetch(gatewayUrl(`/v1/runs/${encodeURIComponent(selectedId ?? "")}`), { cache: "no-store", signal });
        if (!response.ok) throw new Error(response.status === 404 ? "找不到这条运行记录" : `详情请求失败（${response.status}）`);
        const payload = await response.json() as { run: RunDetail };
        if (!payload.run || payload.run.id !== selectedId || !payload.run.state) throw new Error("运行详情格式无效");
        return hydrateRunAssets(payload.run);
      },
      commit: (hydrated) => setDetail(previous => reuseUnchangedState(previous, hydrated)),
      status: (status, error) => {
        setDetailStatus(status);
        if (status === "error") setDetailError(error instanceof Error ? error.message : "详情加载失败");
        if (status === "ready") setDetailError("");
      },
    });
    detailResource.current = resource;
    if (selectedId) resource.request("initial");
    subscription.onopen = () => { setConnection("live"); setFeedError(""); };
    subscription.onerror = () => setConnection("offline");
    subscription.onmessage = (event) => {
      try {
        const payload = JSON.parse(event.data);
        if (typeof payload.revision === "number" && payload.revision <= appliedRevision && payload.reset !== true) return;
        const update = applySubscription<RunSummary>(runsRef.current, payload);
        appliedRevision = typeof payload.revision === "number" ? payload.revision : appliedRevision;
        const observedAt = update.generatedAt;
        const nextFeed = payload.feed && typeof payload.feed.connected === "boolean" ? payload.feed as Feed : null;
        runsRef.current = update.runs;
        setRuns(update.runs);
        setFeed(nextFeed);
        setSnapshotNow(feedNow(observedAt, nextFeed));
        setClockOffset(observedAt - Date.now());
        setLastReceivedAt(Date.now());
        setConnection("live");
        setFeedError("");
        if (reconnectTimer !== null) { clearTimeout(reconnectTimer); reconnectTimer = null; }
        if (selectedId) {
          const selected = update.runs.find(run => run.id === selectedId);
          if (selected) setSelectedGame(selected.game);
          resource.request(selected?.detail_revision ?? `${selected?.latest_sequence ?? 0}:${selected?.live}:${selected?.termination?.kind}`);
        }
      } catch (error) {
        setFeedError(error instanceof Error ? error.message : "直播数据暂时不可用");
        setConnection("offline");
        // A server-side projection error can recover without another game event.
        // Reconnect once through the component's normal effect cleanup.
        if (reconnectTimer === null) reconnectTimer = setTimeout(() => { subscription.close(); setFeedRetry(value => value + 1); }, 3000);
      }
    };
    return () => {
      resource.dispose();
      detailResource.current = null;
      subscription.close();
      if (reconnectTimer !== null) clearTimeout(reconnectTimer);
    };
  }, [selectedId, paused, feedRetry]);

  const grouped = useMemo(
    () =>
      Object.fromEntries(
        GAME_IDS.map((game) => [
          game,
          rankRuns(runs.filter((run) => run.game === game)),
        ]),
      ) as Record<GameId, RunSummary[]>,
    [runs],
  );
  // Labels only need to tell runs of the same game apart.
  const labels = useMemo(
    () => new Map(GAME_IDS.flatMap((game) => [...runLabels(grouped[game])])),
    [grouped],
  );
  const visibleRuns = grouped[selectedGame];
  const liveCount = runs.filter((run) => run.live).length;
  const sourceOffline = feed !== null && !feed.connected && feed.last_seen_ms !== null;
  function selectRun(run: RunSummary) {
    // Selecting the current run must preserve its detail and replay state.
    // Its unchanged ID would not restart the detail resource after clearing it.
    if (run.id === selectedId) return;
    contentRef.current?.scrollTo({ top: 0 });
    window.scrollTo({ top: 0 });
    setDetail(null);
    setSelectedId(run.id);
    setSelectedGame(run.game);
    const url = new URL(window.location.href);
    url.searchParams.set("run", run.id);
    url.searchParams.delete("game");
    window.history.pushState({}, "", url);
  }

  function showDashboard(game = selectedGame) {
    contentRef.current?.scrollTo({ top: 0 });
    window.scrollTo({ top: 0 });
    setSelectedGame(game);
    setSelectedId(null);
    setDetail(null);
    const url = new URL(window.location.href);
    url.searchParams.delete("run");
    if (game === GAME_IDS[0]) url.searchParams.delete("game");
    else url.searchParams.set("game", game);
    window.history.pushState({}, "", url);
  }

  return (
    <main className="live-shell">
      <header className="topbar">
        <button
          className="brand"
          onClick={() => showDashboard(selectedGame)}
          aria-label="返回直播大盘"
        >
          <span>3720</span>
          <strong>Benchmark Live</strong>
        </button>
        <div className={`ingest-status ${sourceOffline ? "offline" : connection}`} title={lastReceivedAt ? `最近数据更新 ${clockTime(lastReceivedAt)}` : "正在连接"}>
          <i />
          {paused
            ? "画面已暂停"
            : sourceOffline
              ? `直播源离线 · 停在 ${clockTime(feed?.last_seen_ms)}`
              : connection === "live"
                ? liveCount ? `${liveCount} 个运行进行中` : "已连接 · 暂无运行"
                : connection === "connecting" ? "正在连接" : "连接中断"}
        </div>
        <div className="top-actions">
          {(paused || liveCount > 0) && <button onClick={() => setPaused((value) => !value)}>{paused ? "恢复更新" : "暂停更新"}</button>}
        </div>
      </header>

      <aside className="run-sidebar">
        <div className="sidebar-label">游戏</div>
        <nav className="game-switcher" aria-label="选择游戏">
          {GAME_IDS.filter(game => grouped[game].length || selectedGame === game).map(game => <button key={game} aria-pressed={selectedGame === game} onClick={() => showDashboard(game)}><span>{GAME_META[game].short}</span><small>{grouped[game].length}</small></button>)}
        </nav>
        <div className="mobile-switcher">
          <LiveSelect label="切换游戏" value={selectedGame} onChange={value => showDashboard(value as GameId)} options={GAME_IDS.filter(game => grouped[game].length || selectedGame === game).map(game => ({ value: game, label: GAME_META[game].short }))} />
          <LiveSelect label="切换运行" value={selectedId ?? "__overview"} onChange={value => { const run = runs.find(run => run.id === value); if (run) selectRun(run); else showDashboard(); }} options={[{ value: "__overview", label: "得分总览" }, ...visibleRuns.map(run => ({ value: run.id, label: `${labels.get(run.id)} · ${run.score} 分` }))]} />
        </div>
        {[selectedGame].map((game) => {
          const meta = GAME_META[game];
          const gameRuns = grouped[game];
          return (
            <section
              className={`game-group ${selectedGame === game ? "current" : ""}`}
              key={game}
              style={{ "--accent": meta.accent } as React.CSSProperties}
            >
              <div className="model-list">
                {gameRuns.map((run) => (
                  <button
                    key={run.id}
                    className={`model-run ${selectedId === run.id ? "selected" : ""}`}
                    onClick={() => selectRun(run)}
                    aria-pressed={selectedId === run.id}
                  >
                    <span className={`run-dot ${runStatus(run).className}`} />
                    <span className="model-copy">
                      <strong>{labels.get(run.id)}</strong>
                      <small>
                        {[harnessName(run.agent), durationLabel(taskDuration(run, snapshotNow)), runStatus(run).tone === "ended" ? "" : runStatus(run).label].filter(Boolean).join(" · ")}
                      </small>
                    </span>
                    <b>{run.score}</b>
                  </button>
                ))}
                {!gameRuns.length && <div className="no-runs">暂无运行记录</div>}
              </div>
            </section>
          );
        })}
      </aside>

      <section className="content" ref={contentRef}>
        {sourceOffline && <div className="data-notice" role="status">评测机已与直播断开，画面停在 {clockTime(feed?.last_seen_ms)} 的最后状态，恢复后会自动续上</div>}
        {feedError && <div className="data-notice" role="alert">{feedError} · 保留上次画面 <button onClick={() => setFeedRetry(value => value + 1)}>重新连接</button></div>}
        {selectedId && detailStatus === "error" && <div className="data-notice" role="alert">{detailError} · 画面可能已过期 <button onClick={() => detailResource.current?.retry()}>重试详情</button></div>}
        {selectedId ? (
          <RunDetails
            key={selectedId}
            run={detail ?? runs.find((run) => run.id === selectedId) ?? null}
            now={snapshotNow}
            onBack={() => showDashboard(selectedGame)}
          />
        ) : (
          <GameDashboard key={selectedGame} game={selectedGame} runs={visibleRuns} labels={labels} now={snapshotNow} onSelect={selectRun} />
        )}
      </section>
    </main>
  );
}

function GameDashboard({
  game,
  runs,
  labels,
  now,
  onSelect,
}: {
  game: GameId;
  runs: RunSummary[];
  labels: Map<string, string>;
  now: number;
  onSelect: (run: RunSummary) => void;
}) {
  const [compared, setCompared] = useState<Set<string> | null>(null);
  const meta = GAME_META[game];
  const colors = useMemo(() => seriesColors(runs), [runs]);
  const selected = compared ?? new Set(runs.slice(0, 6).map(run => run.id));
  const curves = runs.filter(run => selected.has(run.id)).slice(0, 8);
  const [scale, setScale] = useState<ChartScale | null>(null);
  const chartScale = scale ?? preferredScale(curves, now);
  const best = runs.slice().sort((a, b) => b.score - a.score)[0];
  const liveRuns = runs.filter((run) => run.live).length;
  let rank = 0;
  return (
    <div className="dashboard-page" style={{ "--accent": meta.accent } as React.CSSProperties}>
      <header className="page-heading">
        <div>
          <h1>{meta.label}</h1>
          <p className="page-subtitle">
            {best
              ? `${runs.length} 次运行，最好成绩是 ${labels.get(best.id)} 的 ${best.score} / ${best.total || "—"}。点开一次运行可以看现场画面和每次尝试的回放。`
              : "新的运行开始后会出现在这里。"}
          </p>
        </div>
        <div className="heading-metrics">
          <Metric label="运行" value={String(runs.length)} />
          <Metric label="直播中" value={String(liveRuns)} />
          <Metric label="最高分" value={best ? `${best.score}` : "—"} />
        </div>
      </header>

      {runs.length > 0 && (
        <section className="chart-card">
          <div className="section-title">
            <strong>得分随运行时间的变化</strong>
            <div className="scale-switch" role="group" aria-label="横轴刻度">
              {(["linear", "log"] as const).map(option => (
                <button key={option} aria-pressed={chartScale === option} onClick={() => setScale(option)}>
                  {option === "linear" ? "线性时间" : "对数时间"}
                </button>
              ))}
            </div>
          </div>
          <ScoreChart runs={curves} now={now} colors={colors} labels={labels} scale={chartScale} onSelect={onSelect} />
          <div className="compare-picker" role="group" aria-label="选择对比曲线">
            <span>对比曲线（最多 8 条）</span>
            {runs.map(run => <Toggle.Root className="series-toggle" key={run.id} style={{ "--series": colors.get(run.id) } as React.CSSProperties} pressed={selected.has(run.id)} disabled={!selected.has(run.id) && curves.length >= 8} onPressedChange={pressed => setCompared(() => { const next = new Set(selected); if (pressed) next.add(run.id); else next.delete(run.id); return next; })}><span className="series-toggle-check"><Check size={12} aria-hidden="true" /></span><span>{labels.get(run.id)}</span></Toggle.Root>)}
          </div>
        </section>
      )}

      <section className="run-grid">
        {runs.map((run) => {
          const status = runStatus(run);
          const place = run.live ? null : ++rank;
          return (
            <button
              className="run-card"
              key={run.id}
              onClick={() => onSelect(run)}
              style={{ "--series": colors.get(run.id) } as React.CSSProperties}
            >
              <div className="run-card-top">
                {place !== null && <span className="run-rank" aria-label={`第 ${place} 名`}>{place}</span>}
                <span className={`status-badge ${status.className}`} title={status.detail}>{status.label}</span>
                {status.tone === "ended" && <small>{status.detail}</small>}
              </div>
              <h2 className="model-title">{labels.get(run.id)}</h2>
              <div className="score-block">
                <strong>{run.score}</strong>
                <span>/ {run.total || "—"}</span>
              </div>
              <p>{run.live ? "正在玩" : "停在"} {run.objective}</p>
              <div className="run-card-meta">
                <span>运行 {durationLabel(taskDuration(run, now))}</span>
                <span>{sinceLastScore(run, now)}</span>
              </div>
            </button>
          );
        })}
        {!runs.length && <EmptyState title="还没有运行" body="新的运行开始后会出现在这里。" />}
      </section>
    </div>
  );
}

function RunDetails({
  run,
  now,
  onBack,
}: {
  run: RunDetail | RunSummary | null;
  now: number;
  onBack: () => void;
}) {
  const runDetail = run as RunDetail | null;
  const [attemptReplay, setAttemptReplay] = useState<LoadedAttemptReplay | null>(null);
  const [attemptReplayLoading, setAttemptReplayLoading] = useState(false);
  const [pendingAttemptReplay, setPendingAttemptReplay] = useState<number | null>(null);
  const [attemptReplayError, setAttemptReplayError] = useState("");
  const [skipFailedAttempts, setSkipFailedAttempts] = useState(false);
  const [catalogPage, setCatalogPage] = useState<{ groups: ReplayGroupSummary[]; more: boolean; before: number | null } | null>(null);
  const [catalogLoading, setCatalogLoading] = useState(false);
  const [catalogError, setCatalogError] = useState("");
  const catalogGate = useRef(new RequestGate());
  const [replayPages, setReplayPages] = useState<(number | null)[]>([null]);
  const replayGroups = catalogPage?.groups ?? runDetail?.replay_groups ?? [];
  const [exporting, setExporting] = useState<{
    format: ReplayExportFormat;
    completed: number;
    total: number;
  } | null>(null);
  const [exportNotice, setExportNotice] = useState("");
  const exportStageRef = useRef<HTMLDivElement>(null);
  const exportAbortRef = useRef<AbortController | null>(null);
  const [heldLiveReplay, setHeldLiveReplay] = useState<RunDetail["live_replay"]>(null);
  const replayGate = useRef(new RequestGate());
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    const replay = replayGate.current;
    const catalog = catalogGate.current;
    return () => { mounted.current = false; replay.cancel(); catalog.cancel(); exportAbortRef.current?.abort(); };
  }, []);
  const liveReplay = heldLiveReplay ?? runDetail?.live_replay ?? null;
  const frames = useMemo(
    () => attemptReplay?.frames ?? liveReplay?.frames ?? [],
    [attemptReplay, liveReplay],
  );
  const [cursorKey, setCursorKey] = useState<string | null>(null);
  const [playing, setPlaying] = useState(false);
  const [speed, setSpeed] = useState(1);
  const frameIndexByKey = useMemo(
    () => new Map(frames.map((frame, index) => [frame.key, index])),
    [frames],
  );

  // Live mode shows the latest frame (cursorKey === null). The gateway sends a
  // fresh live_replay object on every detail reload, so keying any cursor reset
  // off its identity would yank viewers back to frame 0 on every live tick.
  // Attempt replays opt into autoplay explicitly in loadAttemptReplay.
  useEffect(() => {
    if (
      cursorKey !== null &&
      frames.length > 0 &&
      !frames.some((frame) => frame.key === cursorKey)
    ) {
      queueMicrotask(() => {
        setCursorKey(null);
        setPlaying(false);
      });
    }
  }, [cursorKey, frames]);

  const cursorIndex = cursorKey === null
    ? frames.length - 1
    : (frameIndexByKey.get(cursorKey) ?? -1);
  const activeIndex = cursorIndex < 0 ? frames.length - 1 : cursorIndex;
  const activeFrame = frames[activeIndex] ?? null;
  const activeEvent = activeFrame?.event ?? null;
  const previousEvent = activeIndex > 0 ? frames[activeIndex - 1].event : null;
  const isLatest = activeIndex >= frames.length - 1;
  const followingLive = attemptReplay === null && heldLiveReplay === null && cursorKey === null && pendingAttemptReplay === null;
  const failedAttempts = replayGroups.reduce(
    (total, group) => total + (group.kind === "level" ? group.attempts.filter((attempt) => !attempt.successful && attempt.status !== "running").length : 0),
    0,
  );

  const runId = runDetail?.id;
  const loadedAttemptId = attemptReplay?.attempt_id;
  const hasNextFragment = attemptReplay?.next_after_sequence != null;
  const loadAttemptReplay = useCallback(async (attemptId: number, pages: (number | null)[] = [null]) => {
    if (!runId || exporting) return;
    if (loadedAttemptId === attemptId && pages.length === 1 && replayPages.length === 1) {
      setCursorKey(frames[0]?.key ?? null);
      setPlaying(frames.length > 1 || hasNextFragment);
      return;
    }
    setCursorKey(null);
    setPlaying(false);
    setAttemptReplayLoading(true);
    setPendingAttemptReplay(attemptId);
    setAttemptReplayError("");
    const request = replayGate.current.begin();
    try {
      const path = `/v1/runs/${encodeURIComponent(runId)}`;
      const url = gatewayUrl(path);
      url.searchParams.set("replay_attempt", String(attemptId));
      const after = pages[pages.length - 1];
      if (after !== null) url.searchParams.set("after_sequence", String(after));
      const response = await fetch(url, { cache: "no-store", signal: AbortSignal.any([request.signal, AbortSignal.timeout(15_000)]) });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      const payload = await response.json() as LoadedAttemptReplay & { schema: string };
      if (!request.current()) return;
      if (payload.attempt_id !== attemptId || !Array.isArray(payload.frames)) throw new Error("回放格式无效");
      setHeldLiveReplay(null);
      setCursorKey(payload.frames[0]?.key ?? null);
      setPlaying(payload.frames.length > 1 || payload.next_after_sequence != null);
      setAttemptReplay(payload);
      setReplayPages(pages);
    } catch {
      if (request.current()) setAttemptReplayError("尝试回放加载失败，请重新选择尝试");
    } finally {
      if (request.current()) { setAttemptReplayLoading(false); setPendingAttemptReplay(null); }
    }
  }, [runId, exporting, loadedAttemptId, hasNextFragment, frames, replayPages.length]);

  useEffect(() => {
    if (!playing || !frames.length || isLatest) {
      if (playing && isLatest) queueMicrotask(() => {
        if (attemptReplay?.next_after_sequence != null) void loadAttemptReplay(attemptReplay.attempt_id, [...replayPages, attemptReplay.next_after_sequence]);
        else setPlaying(false);
      });
      return;
    }
    const timer = window.setTimeout(() => {
      setCursorKey(frames[activeIndex + 1]?.key ?? null);
    }, 450 / speed);
    return () => window.clearTimeout(timer);
  }, [activeIndex, frames, isLatest, playing, speed, attemptReplay?.next_after_sequence, attemptReplay?.attempt_id, replayPages, loadAttemptReplay]);

  const frameGame = run?.game;
  const frameState = (followingLive ? runDetail?.state : activeEvent?.state) ?? runDetail?.state;
  const state = useMemo(() => frameGame ? resolveGameFrameState(frameGame, frameState ?? {}, runDetail?.state ?? {}) : {}, [frameGame, frameState, runDetail?.state]);
  const previousFrameState = followingLive ? null : previousEvent?.state;
  const previousState = useMemo(() => frameGame && previousFrameState ? resolveGameFrameState(frameGame, previousFrameState, runDetail?.state ?? {}) : null, [frameGame, previousFrameState, runDetail?.state]);
  if (!run || !("state" in run)) {
    return <EmptyState title="正在加载运行" body="" />;
  }
  const detail = run as RunDetail;
  const meta = GAME_META[run.game];
  const selectedAttempt = replayGroups
    .flatMap((group) => group.attempts.map((attempt, index) => ({ group, attempt, index })))
    .find(({ attempt }) => attempt.id === attemptReplay?.attempt_id);
  const environmentTitle = attemptReplay
    ? attemptReplay.kind === "overworld"
      ? `${attemptReplay.title ?? "Land's End"} / 大地图`
      : attemptReplay.kind === "level"
        ? [...new Set([attemptReplay.reference, attemptReplay.title].filter(Boolean))].join(" / ") || "未命名关卡"
        : attemptReplay.title ?? "历史任务"
    : run.objective;
  const environmentContext = gameStateContext(run.game, state);
  const elapsed = taskDuration(run, now);
  const status = runStatus(run);

  function moveCursor(index: number) {
    if (exporting) return;
    if (!attemptReplay && !heldLiveReplay) setHeldLiveReplay(runDetail?.live_replay ?? null);
    const bounded = Math.max(0, Math.min(frames.length - 1, index));
    setCursorKey(frames[bounded]?.key ?? null);
    setPlaying(false);
  }

  function togglePlayback() {
    if (exporting) return;
    if (!attemptReplay && !heldLiveReplay) setHeldLiveReplay(runDetail?.live_replay ?? null);
    if (playing) {
      setPlaying(false);
      return;
    }
    if (!frames.length) return;
    if (isLatest) setCursorKey(frames[0].key);
    setPlaying(true);
  }

  function followLive() {
    if (exporting) return;
    replayGate.current.cancel();
    setAttemptReplayLoading(false);
    setAttemptReplayError("");
    setHeldLiveReplay(null);
    setReplayPages([null]);
    setAttemptReplay(null);
    setPendingAttemptReplay(null);
    setCursorKey(null);
    setPlaying(false);
  }


  async function loadOlderCatalog() {
    const before = catalogPage?.before ?? runDetail?.replay_catalog_before;
    if (!runDetail || before == null || catalogLoading || exporting) return;
    const request = catalogGate.current.begin();
    setCatalogLoading(true); setCatalogError("");
    try {
      const url = gatewayUrl(`/v1/runs/${encodeURIComponent(runDetail.id)}`);
      url.searchParams.set("catalog_before", String(before));
      const response = await fetch(url, {signal: AbortSignal.any([request.signal, AbortSignal.timeout(15_000)])});
      if (!response.ok) throw new Error("较早记录加载失败");
      const page = await response.json() as { groups: ReplayGroupSummary[]; more: boolean; before: number | null };
      if (!Array.isArray(page.groups)) throw new Error("回放目录格式无效");
      if (request.current()) setCatalogPage(page);
    } catch (error) {
      if (request.current()) setCatalogError(error instanceof Error ? error.message : "回放目录加载失败");
    } finally { if (request.current()) setCatalogLoading(false); }
  }

  async function exportSegment(format: ReplayExportFormat) {
    const element = exportStageRef.current?.querySelector<HTMLElement>(
      `[${REPLAY_CAPTURE_ATTRIBUTE}]`,
    );
    const source = attemptReplay?.frames;
    if (!element || !source?.length || exporting || attemptReplayLoading || !attemptReplay || !run) return;
    replayGate.current.cancel();
    const segment = selectedAttempt
      ? `${selectedAttempt.group.reference}-${selectedAttempt.group.kind === "overworld" ? "route" : "attempt"}-${selectedAttempt.index + 1}`
      : `attempt-${attemptReplay.attempt_id}`;
    const abort = new AbortController();
    exportAbortRef.current = abort;
    setPlaying(false);
    setExportNotice("");
    setExporting({ format, completed: 0, total: source.length });
    let session: CanvasExportSession | undefined;
    try {
      const { exportReplaySegment } = await import("./replay-export");
      const renderer = canvasExportSource(element);
      if (!renderer) throw new Error("画布尚未准备好，请稍后重试");
      const exportFrames = source.map((frame, index) => ({
        state: resolveGameFrameState(run.game, frame.event.state ?? {}, runDetail?.state ?? {}),
        previous: index > 0 ? resolveGameFrameState(run.game, source[index - 1].event.state ?? {}, runDetail?.state ?? {}) : null,
      }));
      session = await renderer.createSession(exportFrames, 900, 900, 80_000_000);
      const activeSession = session;
      await exportReplaySegment({
        fileName: fileSlug(`${run.game}-${run.model}-${segment}`),
        format,
        frameCount: source.length,
        frameDelayMs: 450 / speed,
        signal: abort.signal,
        captureFrame: (index) => activeSession.capture(exportFrames[index]),
        onProgress: (completed) => {
          setExporting({ format, completed, total: source.length });
        },
      });
      if (mounted.current) setExportNotice(format === "gif" ? "GIF 已下载" : "视频已下载");
    } catch (reason) {
      if (mounted.current) setExportNotice(
        reason instanceof DOMException && reason.name === "AbortError"
          ? "导出已取消"
          : reason instanceof Error
            ? reason.message
            : "回放导出失败",
      );
    } finally {
      exportAbortRef.current = null;
      session?.dispose();
      if (mounted.current) setExporting(null);
    }
  }

  return (
    <div className="detail-page" style={{ "--accent": meta.accent } as React.CSSProperties}>
      <header className="detail-heading">
        <button className="back-button" onClick={onBack}>
          ← 返回 {meta.short} 大盘
        </button>
        <div className="detail-title-row">
          <div>
            <h1 className="model-title">
              {modelName(run.model)}
              {run.effort !== "default" && <span>{run.effort}</span>}
            </h1>
          </div>
          <div className="score-hero">
            <span>当前得分</span>
            <strong>{run.score}</strong>
            <small>/ {run.total || "—"}</small>
          </div>
        </div>
        <div className="detail-strip">
          <span className={`status-badge ${status.className}`} title={status.detail}>
            {status.label}
          </span>
          {status.tone === "ended" && <span>{status.detail}</span>}
          {run.termination?.kind !== "no_agent" && <span>运行 {durationLabel(elapsed)}</span>}
          {run.termination?.kind !== "no_agent" && <span>{sinceLastScore(run, now)}</span>}
          {harnessName(run.agent) && <span>{harnessName(run.agent)}</span>}
          {run.started_at && <span>{new Intl.DateTimeFormat("zh-CN", { month: "numeric", day: "numeric" }).format(run.started_at)} 开始</span>}
        </div>
      </header>

      <section className="detail-grid">
        <div className="environment-card">
          <div className="replay-export-stage" ref={exportStageRef}>
            <div className="section-title environment-title">
              <strong title={environmentContext ?? undefined}>{environmentTitle}</strong>
            </div>
            <GameState game={run.game} state={state} previousState={previousState} />
          </div>
          <ReplayTimeline
            frames={frames}
            activeIndex={activeIndex}
            isLatest={isLatest}
            followingLive={followingLive}
            playing={playing}
            speed={speed}
            failedAttempts={failedAttempts}
            skipFailedAttempts={skipFailedAttempts}
            replayGroups={replayGroups}
            selectedAttemptReplay={pendingAttemptReplay ?? attemptReplay?.attempt_id ?? null}
            attemptReplayLoading={attemptReplayLoading}
            pendingAttemptReplay={pendingAttemptReplay}
            attemptReplayError={attemptReplayError}
            canExport={Boolean(attemptReplay?.frames.length) && !attemptReplayLoading}
            exporting={exporting}
            exportNotice={exportNotice}
            onMove={moveCursor}
            onTogglePlayback={togglePlayback}
            onSpeed={(value) => { if (!exporting) setSpeed(value); }}
            onFollowLive={followLive}
            onSkipFailedAttempts={setSkipFailedAttempts}
            onAttemptReplay={loadAttemptReplay}
            onExport={exportSegment}
            onCancelExport={() => exportAbortRef.current?.abort()}
          />
          <div className="history-pages" role="group" aria-label="历史分页">
            {(catalogPage?.more ?? detail.replay_catalog_more) && <button disabled={catalogLoading || exporting !== null} onClick={() => void loadOlderCatalog()}>{catalogLoading ? "加载中…" : "较早的尝试"}</button>}
            {catalogPage && <button disabled={exporting !== null} onClick={() => { catalogGate.current.cancel(); setCatalogLoading(false); setCatalogPage(null); }}>最新尝试</button>}
            {attemptReplay && replayPages.length > 1 && <button disabled={attemptReplayLoading || exporting !== null} onClick={() => void loadAttemptReplay(attemptReplay.attempt_id, replayPages.slice(0, -1))}>上一片段</button>}
            {attemptReplay?.next_after_sequence != null && <button disabled={attemptReplayLoading || exporting !== null} onClick={() => void loadAttemptReplay(attemptReplay.attempt_id, [...replayPages, attemptReplay.next_after_sequence!])}>下一片段</button>}
            {attemptReplay && (replayPages.length > 1 || attemptReplay.next_after_sequence != null) && <span>片段 {replayPages.length} · 导出当前片段</span>}
            {catalogError && <span role="alert">{catalogError}</span>}
          </div>
        </div>
      </section>

      <LiveDisclosure className="analysis-disclosure" title="得分与 Agent 活动" defaultOpen>
        <div className="disclosure-body">

      <section className="detail-chart chart-card">
        <div className="section-title">
          <div>

            <strong>分数轨迹</strong>
          </div>
          <small>累计有效时间 {durationLabel(taskDuration(run, now))}</small>
        </div>
        <ScoreChart runs={[run]} now={now} colors={new Map([[run.id, meta.accent]])} />
      </section>

      <section className="activity-grid">
        <div className="activity-card">
          <div className="section-title">
            <div>
              <strong>{attemptReplay ? "此次尝试的 Agent 活动" : "Agent 最近活动"}</strong>
            </div>
            <small>{selectedAttempt ? `${selectedAttempt.group.kind === "overworld" ? "大地图" : selectedAttempt.group.reference} · ${selectedAttempt.group.kind === "overworld" ? "路段" : "尝试"} ${selectedAttempt.index + 1}` : "随回放段按需载入"}</small>
          </div>
          <div className="activity-list">
            {(attemptReplay?.activity ?? detail.recent_activity)?.length ? (
              (attemptReplay?.activity ?? detail.recent_activity ?? []).map((item, index) => (
                  <article key={`${item.timestamp_ms}-${index}`}>
                    <time>{clockTime(item.timestamp_ms)}</time>
                    <p>{item.text}</p>
                  </article>
                ))
            ) : (
              <EmptyState
                title={attemptReplay ? "此次尝试没有可见消息" : "暂无可见活动"}
                body={attemptReplay ? "这段时间没有保存可展示的消息。" : "这里显示 Agent 的可见消息；历史活动随所选尝试载入。"}
              />
            )}
          </div>
        </div>
        <div className="event-card">
          <div className="section-title">
            <div>
              <strong>{attemptReplay ? "此次尝试的操作" : "最近操作"}</strong>
            </div>
            <small>{attemptReplay ? `${attemptReplay.operations.length} 组` : "随尝试按需载入"}</small>
          </div>
          <div className="event-list">
            {(attemptReplay?.operations ?? liveReplay?.operations)?.length ? (
              (attemptReplay?.operations ?? liveReplay?.operations ?? []).map((operation, operationIndex) => {
                const frameIndex = frameIndexByKey.get(operation.first_frame_key) ?? -1;
                const lastFrameIndex = frameIndexByKey.get(operation.last_frame_key);
                const frame = lastFrameIndex === undefined ? undefined : frames[lastFrameIndex];
                const previousOperation = operationIndex > 0
                  ? (attemptReplay?.operations ?? liveReplay?.operations ?? [])[operationIndex - 1]
                  : null;
                const previousFrameIndex = previousOperation
                  ? frameIndexByKey.get(previousOperation.last_frame_key)
                  : undefined;
                const previousFrame = previousFrameIndex === undefined
                  ? null
                  : frames[previousFrameIndex];
                const event: ObserverEvent = {
                  ...(frame?.event ?? { sequence: operation.sequence }),
                  sequence: operation.sequence,
                  timestamp_ms: operation.timestamp_ms,
                  action: operation.action,
                  score_delta: operation.score_delta,
                };
                const description = describeGameEvent(
                  run.game,
                  event,
                  previousFrame?.event ?? null,
                );
                return (
                <button
                  className={`event-row ${operation.sequence === activeFrame?.operation_sequence ? "active" : ""}`}
                  key={operation.sequence}
                  onClick={() => frameIndex >= 0 && moveCursor(frameIndex)}
                  disabled={frameIndex < 0}
                  aria-current={operation.sequence === activeFrame?.operation_sequence ? "step" : undefined}
                >
                  <time>{clockTime(operation.timestamp_ms)}</time>
                  <strong>{description.title}</strong>
                  <span>{operation.score_delta ? `${operation.score_delta > 0 ? "+" : ""}${operation.score_delta} 分` : `${operation.frame_count} 帧`}</span>
                  <small>#{operation.sequence}</small>
                </button>
              )})
            ) : (
              <EmptyState
                title={attemptReplay ? "此次尝试没有有效操作" : "选择一次尝试"}
                body="选择历史尝试后，可查看操作并跳转到对应画面。"
              />
            )}
          </div>
        </div>
      </section>
      <ExperiencePanel experience={detail.agent_experience} />
      <LiveDisclosure className="runtime-details" title="运行信息"><dl><dt>运行 ID</dt><dd>{run.id}</dd><dt>Agent</dt><dd>{run.agent}</dd><dt>停止原因</dt><dd>{run.termination?.reason ?? "—"}</dd><dt>最近动作</dt><dd>{clockTime(run.last_activity_at)}</dd></dl></LiveDisclosure>
        </div>
      </LiveDisclosure>
    </div>
  );
}

function ReplayTimeline({
  frames,
  activeIndex,
  isLatest,
  followingLive,
  playing,
  speed,
  failedAttempts,
  skipFailedAttempts,
  replayGroups,
  selectedAttemptReplay,
  attemptReplayLoading,
  pendingAttemptReplay,
  attemptReplayError,
  canExport,
  exporting,
  exportNotice,
  onMove,
  onTogglePlayback,
  onSpeed,
  onFollowLive,
  onSkipFailedAttempts,
  onAttemptReplay,
  onExport,
  onCancelExport,
}: {
  frames: ReplayFrame[];
  activeIndex: number;
  isLatest: boolean;
  followingLive: boolean;
  playing: boolean;
  speed: number;
  failedAttempts: number;
  skipFailedAttempts: boolean;
  replayGroups: ReplayGroupSummary[];
  selectedAttemptReplay: number | null;
  attemptReplayLoading: boolean;
  pendingAttemptReplay: number | null;
  attemptReplayError: string;
  canExport: boolean;
  exporting: { format: ReplayExportFormat; completed: number; total: number } | null;
  exportNotice: string;
  onMove: (index: number) => void;
  onTogglePlayback: () => void;
  onSpeed: (speed: number) => void;
  onFollowLive: () => void;
  onSkipFailedAttempts: (enabled: boolean) => void;
  onAttemptReplay: (attemptId: number) => void;
  onExport: (format: ReplayExportFormat) => void;
  onCancelExport: () => void;
}) {
  const active = frames[activeIndex];
  const previousOperation = frames.findLastIndex(
    (frame, index) => index < activeIndex && frame.operation_sequence !== active?.operation_sequence,
  );
  const nextOperation = frames.findIndex(
    (frame, index) => index > activeIndex && frame.operation_sequence !== active?.operation_sequence,
  );
  const replayAttempts = replayGroups.flatMap((group) =>
    group.attempts.map((attempt, index) => ({ group, attempt, index })),
  );
  const [groupReference, setGroupReference] = useState<string | null>(null);
  const selectedReplay = replayAttempts.find(
    ({ attempt }) => attempt.id === selectedAttemptReplay,
  );
  const newestAttempt = replayAttempts.reduce((latest, item) => item.attempt.id > (latest?.attempt.id ?? -1) ? item : latest, replayAttempts[0]);
  const activeGroup = replayGroups.find(group => group.reference === groupReference) ?? selectedReplay?.group ?? newestAttempt?.group;
  return (
    <section className="replay-panel" aria-label="状态回放时间轴">
      <div className="replay-toolbar">
        {frames.length > 0 && <div className="replay-controls" role="group" aria-label="回放操作" inert={exporting !== null}>
          <ReplayIconButton label="上一条有效指令" icon={ChevronLeft} onClick={() => onMove(activeIndex - 1)} disabled={!frames.length || activeIndex <= 0} />
          <ReplayIconButton className="play-button" label={playing ? "暂停回放" : "播放回放"} icon={playing ? Pause : Play} onClick={onTogglePlayback} disabled={frames.length < 2} />
          <ReplayIconButton label="下一条有效指令" icon={ChevronRight} onClick={() => onMove(activeIndex + 1)} disabled={!frames.length || isLatest} />

          <LiveSelect className="replay-speed" label="回放速度" value={String(speed)} onChange={value => onSpeed(Number(value))} disabled={attemptReplayLoading} options={[0.5, 1, 2, 4].map(value => ({ value: String(value), label: `${value}×` }))} />
          <LiveActionMenu items={[
            { label: "跳到最早状态", icon: Rewind, onSelect: () => onMove(0), disabled: activeIndex <= 0 },
            { label: "上一组操作", icon: ChevronsLeft, onSelect: () => onMove(previousOperation), disabled: previousOperation < 0 },
            { label: "下一组操作", icon: ChevronsRight, onSelect: () => onMove(nextOperation), disabled: nextOperation < 0 },
            { label: "导出 GIF", icon: FileImage, onSelect: () => onExport("gif"), disabled: !canExport || exporting !== null, separator: true },
            { label: "导出视频", icon: Film, onSelect: () => onExport("video"), disabled: !canExport || exporting !== null },
          ]} />
          <div className="replay-inline-scrubber">
            <LiveSlider max={Math.max(0, frames.length - 1)} value={Math.max(0, activeIndex)} onChange={onMove} disabled={!frames.length} />
            <b>{active ? `${instructionLabel(active)} · ${actionLabel(active.event.action)}` : "—"}</b>
          </div>
        </div>}
        <div className="replay-status" aria-live="polite">
          <i className={followingLive && selectedAttemptReplay === null ? "live" : "replay"} />
          {exporting
            ? <>
                正在导出 {exporting.format === "gif" ? "GIF" : "视频"} · {exporting.completed} / {exporting.total}
                <button type="button" className="replay-export-cancel" onClick={onCancelExport}>
                  取消
                </button>
              </>
            : exportNotice
              ? exportNotice
              : pendingAttemptReplay !== null
            ? "正在载入尝试回放…"
            : selectedReplay
              ? selectedReplay.group.kind === "overworld"
                ? `正在回放大地图 · 路段 ${selectedReplay.index + 1}`
                : `正在回放 ${selectedReplay.group.reference} · 尝试 ${selectedReplay.index + 1}`
              : followingLive
                ? "最新状态"
                : isLatest
                  ? "已到当前片段末尾"
                  : `回看 ${clockTime(active?.event.timestamp_ms)}`}
          {!followingLive && <button type="button" className="follow-live-button" onClick={onFollowLive} disabled={exporting !== null}><Radio aria-hidden="true" size={16} />返回最新</button>}
        </div>
      </div>
      {replayGroups.length > 0 && <div className="score-replay-picker">
        <div className="history-selector">
          <span className="control-label">历史尝试</span><LiveSelect className="history-select" label="选择历史关卡" value={activeGroup?.reference ?? ""} onChange={setGroupReference} disabled={!replayGroups.length || exporting !== null} options={replayGroups.map(group => ({ value: group.reference, label: `${group.title && group.title !== group.reference ? `${group.reference} · ${group.title}` : group.reference} · ${group.attempts.length} 次尝试` }))} />
          <Toggle.Root
            className="replay-icon replay-toggle"
            aria-label="跳过失败尝试（以重置为分界）"
            title="跳过失败尝试（以重置为分界）"
            pressed={skipFailedAttempts}
            onPressedChange={onSkipFailedAttempts}
            disabled={!failedAttempts}
          >
            <CircleSlash2 aria-hidden="true" size={17} strokeWidth={2} /><span>隐藏失败</span>
          </Toggle.Root>
        </div>
        <ScrollArea.Root className="score-replay-scroll" type="auto">
          <ScrollArea.Viewport className="score-replay-viewport">
            <div className="score-replay-list" role="group" aria-label="选择回放段">
              {(activeGroup ? [activeGroup] : []).map((group) => (
                <section className={`score-replay-level ${group.kind}`} key={group.reference}>
                  <header>
                    <strong>{group.kind === "overworld" ? <MapIcon aria-label="大地图" size={14} /> : group.reference}</strong>
                    <span>{group.title ?? (group.kind === "overworld" ? "Land's End" : "未命名关卡")}</span>
                    <small>{group.kind === "overworld" ? "大地图" : group.kind === "shift" ? "班次" : group.kind === "world" ? "世界" : `${group.score} 分`}</small>
                  </header>
                  <div>
                    {group.attempts
                      .map((attempt, index) => ({ attempt, index }))
                      .filter(({ attempt }) => group.kind === "overworld" || !skipFailedAttempts || attempt.successful || attempt.status === "running")
                      .reverse()
                      .map(({ attempt, index }) => {
                      const selected = attempt.id === selectedAttemptReplay;
                      const loading = attempt.id === pendingAttemptReplay;
                      return (
                        <button
                          type="button"
                          aria-pressed={selected}
                          className={`${selected ? "selected" : ""} ${group.kind === "overworld" ? "map" : attempt.successful ? "scored" : "failed"}`}
                          key={attempt.id}
                          aria-label={`尝试 ${index + 1}，${attempt.successful ? `得分 ${attempt.score}` : "未得分"}`}
                          title={group.kind === "overworld" ? `${group.title ?? "Land's End"} · 大地图路段 ${index + 1}` : `${group.reference} / ${group.title ?? "未命名关卡"} · 尝试 ${index + 1}${attempt.successful ? ` · 得分 ${attempt.score}` : " · 未得分"}`}
                          onClick={() => onAttemptReplay(attempt.id)}
                          disabled={exporting !== null || (attemptReplayLoading && !loading)}
                        >
                          <strong>{loading ? "…" : index + 1}</strong>
                          {attempt.successful && <small>+{attempt.score}</small>}
                        </button>
                      );
                      })}
                    {skipFailedAttempts && group.kind !== "overworld" && !group.attempts.some(attempt => attempt.successful || attempt.status === "running") && <p className="history-filter-empty">此关卡没有得分尝试，关闭“隐藏失败”可查看全部。</p>}
                  </div>
                </section>
              ))}
              {!replayGroups.length && <span className="score-replay-empty">暂无可回放操作</span>}
            </div>
          </ScrollArea.Viewport>
          <ScrollArea.Scrollbar className="score-replay-scrollbar" orientation="horizontal">
            <ScrollArea.Thumb className="score-replay-thumb" />
          </ScrollArea.Scrollbar>
        </ScrollArea.Root>
      </div>}
      {attemptReplayError && <p className="replay-error">{attemptReplayError}</p>}
    </section>
  );
}

function ReplayIconButton({
  label,
  icon,
  className = "",
  ...props
}: {
  label: string;
  icon: LucideIcon;
  className?: string;
} & ButtonHTMLAttributes<HTMLButtonElement>) {
  const Icon = icon;
  return (
    <button className={`replay-icon ${className}`} aria-label={label} title={label} {...props}>
      <Icon aria-hidden="true" size={17} strokeWidth={2} />
    </button>
  );
}

const EXPERIENCE_SECTIONS = [
  ["plan", "CURRENT PLAN", "当前计划与开放问题"],
  ["verified", "VERIFIED MECHANICS", "已验证规律与可复用经验"],
  ["rejected", "REJECTED ROUTES", "已否定路线"],
  ["solved", "SOLVED", "已解关卡"],
] as const;

function ExperiencePanel({ experience }: { experience?: RunDetail["agent_experience"] }) {
  if (!EXPERIENCE_SECTIONS.some(([key]) => experience?.[key]?.length)) return null;
  return (
    <section className="experience-card">
      <div className="section-title">
        <div>
          <strong>Agent 经验板</strong>
        </div>
        <small>
          {experience?.updated_at
            ? `${experience.source_count} 份可见笔记 · 更新于 ${clockTime(experience.updated_at)}`
            : "仅投影 Agent 明确写入的可见 Markdown 条目"}
        </small>
      </div>
      <div className="experience-grid">
        {EXPERIENCE_SECTIONS.map(([key, , title]) => {
          const items = experience?.[key] ?? [];
          if (!items.length) return null;
          return (
            <section key={key} data-experience={key}>
              <header>
                <strong>{title}</strong>
                <b>{experience?.counts[key] ?? 0}</b>
              </header>
              {items.length ? (
                <ol>
                  {items.slice().reverse().map((item, index) => <li key={`${key}-${index}`}>{item}</li>)}
                </ol>
              ) : (
                <p>暂无明确记录</p>
              )}
            </section>
          );
        })}
      </div>
      <p className="experience-disclosure">来自 Agent 保存的笔记。</p>
    </section>
  );
}

type ChartScale = "linear" | "log";

const MINUTE = 60_000;

/** Log time when the compared runs' lengths differ by more than 5×. */
function preferredScale(runs: RunSummary[], now: number): ChartScale {
  const lengths = runs.map(run => taskDuration(run, now)).filter(length => length > 0);
  return lengths.length > 1 && Math.max(...lengths) > 5 * Math.min(...lengths) ? "log" : "linear";
}

function logTickLabel(value: number) {
  if (value === 0) return "0";
  return value < 60 * MINUTE ? `${Math.round(value / MINUTE)}m` : `${Math.round(value / (60 * MINUTE))}h`;
}

function subscribeNarrow(callback: () => void) {
  const query = window.matchMedia("(max-width: 700px)");
  query.addEventListener("change", callback);
  return () => query.removeEventListener("change", callback);
}

function useNarrowScreen() {
  return useSyncExternalStore(subscribeNarrow, () => window.matchMedia("(max-width: 700px)").matches, () => false);
}

function ScoreChart({
  runs,
  now,
  colors,
  labels,
  scale = "linear",
  onSelect,
}: {
  runs: RunSummary[];
  now: number;
  colors: Map<string, string>;
  labels?: Map<string, string>;
  scale?: ChartScale;
  onSelect?: (run: RunSummary) => void;
}) {
  const narrow = useNarrowScreen();
  const width = narrow ? 560 : 960;
  const height = narrow ? 360 : 300;
  const pad = { left: narrow ? 56 : 48, right: labels ? (narrow ? 150 : 210) : 24, top: 20, bottom: narrow ? 48 : 40 };
  const timelines = runs.map((run) => ({ run, points: scoreTimeline(run, now) }));
  const points = timelines.flatMap((timeline) => timeline.points);
  const maxDuration = Math.max(MINUTE, ...points.map((point) => point.elapsed_ms));
  const observedMax = Math.max(1, ...runs.map((run) => run.score), ...points.map((point) => point.score));
  // Scores can go negative (e.g. kitchen penalties); a zero-floored axis
  // would draw those trajectories outside the chart.
  const observedMin = Math.min(0, ...runs.map((run) => run.score), ...points.map((point) => point.score));
  const yMax = observedMax <= 10 ? 10 : Math.ceil(observedMax / 10) * 10;
  const yMin = observedMin === 0 ? 0 : Math.floor(observedMin / 10) * 10;
  const plotWidth = width - pad.left - pad.right;
  const position = scale === "log"
    ? (value: number) => Math.log1p(value / MINUTE) / Math.log1p(maxDuration / MINUTE)
    : (value: number) => value / maxDuration;
  const x = (value: number) => pad.left + position(value) * plotWidth;
  const y = (value: number) => pad.top + (1 - (value - yMin) / (yMax - yMin)) * (height - pad.top - pad.bottom);
  const yTicks = Array.from({ length: 5 }, (_, index) => yMin + ((yMax - yMin) * index) / 4);
  const xTicks = (scale === "log"
    ? [0, 10 * MINUTE, 60 * MINUTE, 3 * 60 * MINUTE, 10 * 60 * MINUTE, 24 * 60 * MINUTE, 72 * 60 * MINUTE, 240 * 60 * MINUTE]
    : [0, 0.25, 0.5, 0.75, 1].map((ratio) => maxDuration * ratio))
    .filter((value) => value <= maxDuration)
    .reduce<number[]>((kept, value) => kept.length && x(value) - x(kept[kept.length - 1]) < (narrow ? 70 : 56) ? kept : [...kept, value], []);
  // End labels sit in a column right of the plot, spread so they never overlap.
  const gap = narrow ? 24 : 17;
  const ends = timelines
    .map(({ run, points: history }) => ({ run, last: history[history.length - 1] }))
    .map((end) => ({ ...end, labelY: y(end.last.score) }))
    .sort((a, b) => a.labelY - b.labelY);
  for (let index = 1; index < ends.length; index += 1) {
    ends[index].labelY = Math.max(ends[index].labelY, ends[index - 1].labelY + gap);
  }
  const overflow = ends.length ? ends[ends.length - 1].labelY - (height - pad.bottom) : 0;
  if (overflow > 0) for (const end of ends) end.labelY = Math.max(pad.top, end.labelY - overflow);
  const labelX = width - pad.right + 14;
  const labelChars = narrow ? 11 : 20;

  return (
    <div className="score-chart-wrap">
      <svg
        className={`score-chart ${narrow ? "narrow" : ""}`}
        viewBox={`0 0 ${width} ${height}`}
        role="img"
        aria-label="模型得分随累计有效运行时间变化图"
      >
        {yTicks.map((value) => (
          <g key={value}>
            <line x1={pad.left} x2={width - pad.right} y1={y(value)} y2={y(value)} className="grid-line" />
            <text x={pad.left - 10} y={y(value) + 4} textAnchor="end">{Math.round(value)}</text>
          </g>
        ))}
        {xTicks.map((value, index) => (
          <text
            key={value}
            x={x(value)}
            y={height - (narrow ? 16 : 12)}
            textAnchor={index === 0 ? "start" : x(value) > width - pad.right - 30 ? "end" : "middle"}
          >
            {scale === "log" ? logTickLabel(value) : durationLabel(value, maxDuration)}
          </text>
        ))}
        {timelines.map(({ run, points: history }) => {
          const color = colors.get(run.id) ?? "var(--accent)";
          const path = history
            .map((point, pointIndex) => {
              if (!pointIndex) return `M ${x(point.elapsed_ms)} ${y(point.score)}`;
              const previous = history[pointIndex - 1];
              return `L ${x(point.elapsed_ms)} ${y(previous.score)} L ${x(point.elapsed_ms)} ${y(point.score)}`;
            })
            .join(" ");
          const last = history[history.length - 1];
          return (
            <g key={run.id} className={onSelect ? "clickable-series" : ""} onClick={() => onSelect?.(run)}>
              <path d={path} fill="none" stroke={color} strokeWidth={narrow ? 4 : 3} />
              <circle cx={x(last.elapsed_ms)} cy={y(last.score)} r={narrow ? 6 : 4.5} fill={color} />
              <title>{labels?.get(run.id) ?? modelName(run.model)}：{run.score} 分 · 运行 {durationLabel(last.elapsed_ms)}</title>
            </g>
          );
        })}
        {labels && ends.map(({ run, last, labelY }) => {
          const color = colors.get(run.id) ?? "var(--accent)";
          const name = labels.get(run.id) ?? modelName(run.model);
          const short = name.length > labelChars ? `${name.slice(0, labelChars - 1)}…` : name;
          return (
            <g key={run.id} className={onSelect ? "clickable-series series-label" : "series-label"} onClick={() => onSelect?.(run)}>
              <path d={`M ${x(last.elapsed_ms) + 6} ${y(last.score)} L ${labelX - 4} ${labelY - 4}`} stroke={color} className="label-leader" />
              <text x={labelX} y={labelY} fill={color}>
                <tspan className="label-score">{run.score}</tspan> {short}
              </text>
            </g>
          );
        })}
      </svg>
    </div>
  );
}
