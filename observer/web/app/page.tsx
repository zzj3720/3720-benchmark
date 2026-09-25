"use client";
import * as Toggle from "@radix-ui/react-toggle";
import { Check } from "lucide-react";
import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";

import {
  GAME_IDS,
  GAME_META,
  GameState,
  describeGameEvent,
  gameStateContext,
  resolveGameFrameState,
  type GameId,
} from "./game-registry";
import { EmptyState, Metric, type ObserverEvent } from "./game-observer";
import { LiveDisclosure, LiveSelect } from "./live-controls";

import type { RunSummary, RunDetail, ElapsedScorePoint } from "./live-contract";
import { LiveResource, applySubscription, effectiveDuration } from "./live-client";
import { harnessName, modelName, rankRuns, runLabels, seriesColors, viewerStatus } from "./run-labels";
import { clockTime, durationLabel, gatewayUrl, hydrateRunAssets } from "./live-shared";
import { GameTabs, LevelWatch, ReplayLibrary, type LevelSample } from "./replay-views";


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

type Feed = { connected: boolean; last_seen_ms: number | null };

function feedNow(now: number, feed: Feed | null) {
  return feed && !feed.connected && feed.last_seen_ms !== null ? Math.min(now, feed.last_seen_ms) : now;
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
  // Replays live on their own pages: a per-game library and a per-level watch page.
  const [route, setRoute] = useState<{ view: "dashboard" | "replays"; level: string | null; play: LevelSample | null; model: string | null }>({ view: "dashboard", level: null, play: null, model: null });
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
  const navigateRef = useRef(() => {});
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
      const play = params.get("play")?.split(":");
      contentRef.current?.scrollTo({ top: 0 });
      window.scrollTo({ top: 0 });
      setDetail(current => current?.id === runId ? current : null);
      setSelectedId(runId);
      setRoute({
        view: params.get("view") === "replays" || params.get("level") ? "replays" : "dashboard",
        level: params.get("level"),
        play: play?.length === 2 && /^\d+$/.test(play[1]) ? { run: play[0], attempt: Number(play[1]) } : null,
        model: params.get("model"),
      });
      setSelectedGame(
        runsRef.current.find(run => run.id === runId)?.game ??
        ((GAME_IDS as string[]).includes(game ?? "") ? (game as GameId) : GAME_IDS[0]),
      );
    };
    navigateRef.current = syncFromLocation;
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
  /** Every page is addressable: push the URL, then derive state from it. */
  function go(params: Record<string, string | null | undefined>) {
    const url = new URL(window.location.href);
    url.search = "";
    for (const [key, value] of Object.entries(params)) if (value) url.searchParams.set(key, value);
    window.history.pushState({}, "", url);
    navigateRef.current();
  }

  function selectRun(run: RunSummary) {
    // Selecting the current run must preserve its detail state.
    if (run.id === selectedId) return;
    go({ run: run.id });
  }

  function showDashboard(game = selectedGame) {
    go({ game: game === GAME_IDS[0] ? null : game });
  }

  function showReplays(game = selectedGame, model?: string) {
    go({ game, view: "replays", model });
  }

  function openLevel(key: string, play?: LevelSample) {
    go({ game: selectedGame, level: key, play: play ? `${play.run}:${play.attempt}` : null });
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
          <LiveSelect label="切换运行" value={selectedId ?? (route.view === "replays" ? "__replays" : "__overview")} onChange={value => { const run = runs.find(run => run.id === value); if (run) selectRun(run); else if (value === "__replays") showReplays(); else showDashboard(); }} options={[{ value: "__overview", label: "得分总览" }, { value: "__replays", label: "关卡回放" }, ...visibleRuns.map(run => ({ value: run.id, label: `${labels.get(run.id)} · ${run.score} 分` }))]} />
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
          <LiveRun
            key={selectedId}
            run={detail ?? runs.find((run) => run.id === selectedId) ?? null}
            now={snapshotNow}
            onBack={() => showDashboard(selectedGame)}
            onReplays={() => showReplays(selectedGame, selectedId)}
          />
        ) : route.level ? (
          <LevelWatch
            key={`${selectedGame}:${route.level}`}
            game={selectedGame}
            levelKey={route.level}
            play={route.play}
            runs={visibleRuns}
            labels={labels}
            colors={seriesColors(visibleRuns)}
            onOpenLevel={key => openLevel(key)}
            onPlay={sample => openLevel(route.level!, sample)}
          />
        ) : route.view === "replays" ? (
          <div className="dashboard-page" style={{ "--accent": GAME_META[selectedGame].accent } as React.CSSProperties}>
            <header className="page-heading"><div><h1>{GAME_META[selectedGame].label}</h1></div></header>
            <GameTabs view="replays" onDashboard={() => showDashboard()} onReplays={() => showReplays()} />
            <ReplayLibrary key={`${selectedGame}:${route.model ?? ""}`} game={selectedGame} runs={visibleRuns} labels={labels} initialRun={route.model} onOpenLevel={key => openLevel(key)} />
          </div>
        ) : (
          <GameDashboard key={selectedGame} game={selectedGame} runs={visibleRuns} labels={labels} now={snapshotNow} onSelect={selectRun} onReplays={() => showReplays()} />
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
  onReplays,
}: {
  game: GameId;
  runs: RunSummary[];
  labels: Map<string, string>;
  now: number;
  onSelect: (run: RunSummary) => void;
  onReplays: () => void;
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
      <GameTabs view="dashboard" onDashboard={() => {}} onReplays={onReplays} />

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

function LiveRun({
  run,
  now,
  onBack,
  onReplays,
}: {
  run: RunDetail | RunSummary | null;
  now: number;
  onBack: () => void;
  onReplays: () => void;
}) {
  const runDetail = run && "state" in run ? run as RunDetail : null;
  const game = run?.game;
  const state = useMemo(
    () => game && runDetail ? resolveGameFrameState(game, runDetail.state ?? {}, runDetail.state ?? {}) : {},
    [game, runDetail],
  );
  if (!run || !runDetail) {
    return <EmptyState title="正在加载运行" body="" />;
  }
  const meta = GAME_META[run.game];
  const status = runStatus(run);
  const elapsed = taskDuration(run, now);
  const environmentContext = gameStateContext(run.game, state);
  const operations = runDetail.live_replay?.operations ?? [];

  return (
    <div className="detail-page" style={{ "--accent": meta.accent } as React.CSSProperties}>
      <header className="detail-heading">
        <button className="back-button" onClick={onBack}>
          ← 返回 {meta.short} 成绩
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
          <div className="section-title environment-title">
            <strong title={environmentContext ?? undefined}>{run.objective}</strong>
            <small>{run.live ? "实时画面" : "结束时的画面"}</small>
          </div>
          <GameState game={run.game} state={state} previousState={null} />
          <div className="live-footer">
            <span>{run.live ? `最近动作 ${clockTime(run.last_activity_at)}` : `结束于 ${clockTime(run.finished_at ?? run.last_activity_at)}`}</span>
            <button type="button" className="replay-export-button" onClick={onReplays}>看这次运行的关卡回放</button>
          </div>
        </div>
      </section>

      <LiveDisclosure className="analysis-disclosure" title="得分与 Agent 活动" defaultOpen>
        <div className="disclosure-body">
          <section className="detail-chart chart-card">
            <div className="section-title">
              <strong>分数轨迹</strong>
              <small>累计有效时间 {durationLabel(elapsed)}</small>
            </div>
            <ScoreChart runs={[run]} now={now} colors={new Map([[run.id, meta.accent]])} />
          </section>
          <section className="activity-grid">
            <div className="activity-card">
              <div className="section-title"><strong>Agent 最近说了什么</strong></div>
              <div className="activity-list">
                {runDetail.recent_activity?.length
                  ? runDetail.recent_activity.map((item, index) => (
                    <article key={`${item.timestamp_ms}-${index}`}>
                      <time>{clockTime(item.timestamp_ms)}</time>
                      <p>{item.text}</p>
                    </article>
                  ))
                  : <EmptyState title="暂无可见消息" body="Agent 的可见消息会出现在这里。" />}
              </div>
            </div>
            <div className="event-card">
              <div className="section-title"><strong>最近的操作</strong></div>
              <div className="event-list">
                {operations.length
                  ? operations.slice().reverse().map(operation => {
                    const event: ObserverEvent = { sequence: operation.sequence, timestamp_ms: operation.timestamp_ms, action: operation.action, score_delta: operation.score_delta };
                    return (
                      <div className="event-row" key={operation.sequence}>
                        <time>{clockTime(operation.timestamp_ms)}</time>
                        <strong>{describeGameEvent(run.game, event, null).title}</strong>
                        <span>{operation.score_delta ? `${operation.score_delta > 0 ? "+" : ""}${operation.score_delta} 分` : `${operation.frame_count} 帧`}</span>
                      </div>
                    );
                  })
                  : <EmptyState title="暂无操作" body="" />}
              </div>
            </div>
          </section>
          <ExperiencePanel experience={runDetail.agent_experience} />
          <LiveDisclosure className="runtime-details" title="运行信息"><dl><dt>运行 ID</dt><dd>{run.id}</dd><dt>Agent</dt><dd>{run.agent}</dd><dt>停止原因</dt><dd>{run.termination?.reason ?? "—"}</dd><dt>最近动作</dt><dd>{clockTime(run.last_activity_at)}</dd></dl></LiveDisclosure>
        </div>
      </LiveDisclosure>
    </div>
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
