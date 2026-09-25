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
import { groupByFamily, groupByModel, harnessName, modelFamily, modelName, rankRuns, runLabels, seriesColors, subLabel, viewerStatus } from "./run-labels";
import { clockTime, durationLabel, gatewayUrl, hydrateRunAssets } from "./live-shared";
import { CoverBakery, GameTabs, LevelWatch, ReplayLibrary, RunReplayShelf, type LevelSample } from "./replay-views";
import { Overview } from "./overview";


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
  // `?bake=covers` is the page the cover baker drives, not the console.
  const [bake, setBake] = useState(false);
  useEffect(() => { setBake(new URL(window.location.href).searchParams.get("bake") === "covers"); }, []);
  return bake ? <CoverBakery /> : <Console />;
}

function Console() {
  const [runs, setRuns] = useState<RunSummary[]>([]);
  const [selectedGame, setSelectedGame] = useState<GameId>(GAME_IDS[0]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  // Replays live on their own pages: a per-game library and a per-level watch page.
  // With no run, game or replay in the URL the console opens on the overview of every game.
  const [route, setRoute] = useState<{ view: "overview" | "dashboard" | "replays"; level: string | null; play: LevelSample | null; model: string | null }>({ view: "overview", level: null, play: null, model: null });
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
        view: params.get("view") === "replays" || params.get("level") ? "replays" : runId || game ? "dashboard" : "overview",
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
  const overview = !selectedId && route.view === "overview";
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
    go({ game });
  }

  function showOverview() {
    go({});
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
          onClick={showOverview}
          aria-label="返回大盘"
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
        <button type="button" className="overview-link" aria-pressed={overview} onClick={showOverview}>大盘<small>全部游戏</small></button>
        <div className="sidebar-label">游戏</div>
        <nav className="game-switcher" aria-label="选择游戏">
          {GAME_IDS.filter(game => grouped[game].length || selectedGame === game).map(game => <button key={game} aria-pressed={!overview && selectedGame === game} onClick={() => showDashboard(game)}><span>{GAME_META[game].short}</span><small>{grouped[game].length}</small></button>)}
        </nav>
        <div className="mobile-switcher">
          <LiveSelect label="切换游戏" value={overview ? "__overview" : selectedGame} onChange={value => value === "__overview" ? showOverview() : showDashboard(value as GameId)} options={[{ value: "__overview", label: "大盘" }, ...GAME_IDS.filter(game => grouped[game].length || selectedGame === game).map(game => ({ value: game, label: GAME_META[game].short }))]} />
          {!overview && <LiveSelect label="切换运行" value={selectedId ?? (route.view === "replays" ? "__replays" : "__overview")} onChange={value => { const run = runs.find(run => run.id === value); if (run) selectRun(run); else if (value === "__replays") showReplays(); else showDashboard(); }} options={[{ value: "__overview", label: "得分总览" }, { value: "__replays", label: "关卡回放" }, ...visibleRuns.map(run => ({ value: run.id, label: `${labels.get(run.id)} · ${run.score} 分` }))]} />}
        </div>
        {(overview ? [] : [selectedGame]).map((game) => {
          const meta = GAME_META[game];
          const gameRuns = grouped[game];
          return (
            <section
              className={`game-group ${selectedGame === game ? "current" : ""}`}
              key={game}
              style={{ "--accent": meta.accent } as React.CSSProperties}
            >
              <div className="model-list">
                {groupByFamily(gameRuns).flatMap(({ family, runs: familyRuns }) => [
                  <div className="family-heading" key={`family:${family}`}>{family}</div>,
                  // Several thinking depths of one model sit under the model's name.
                  ...groupByModel(familyRuns).flatMap(({ model, runs: modelRuns }) => [
                    ...(modelRuns.length > 1 ? [<div className="model-heading" key={`model:${model}`}>{model}</div>] : []),
                    ...modelRuns.map((run) => (
                  <button
                    key={run.id}
                    className={`model-run ${modelRuns.length > 1 ? "child" : ""} ${selectedId === run.id ? "selected" : ""}`}
                    onClick={() => selectRun(run)}
                    aria-pressed={selectedId === run.id}
                  >
                    <span className={`run-dot ${runStatus(run).className}`} />
                    <span className="model-copy">
                      <strong>{modelRuns.length > 1 ? subLabel(run, labels.get(run.id)) : labels.get(run.id)}</strong>
                      <small>
                        {[modelRuns.length > 1 && subLabel(run, labels.get(run.id)).includes(harnessName(run.agent)) ? "" : harnessName(run.agent), durationLabel(taskDuration(run, snapshotNow)), runStatus(run).tone === "ended" ? "" : runStatus(run).label].filter(Boolean).join(" · ")}
                      </small>
                    </span>
                    <b>{run.score}</b>
                  </button>
                    )),
                  ]),
                ])}
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
            standing={(() => {
              const byScore = visibleRuns.slice().sort((a, b) => b.score - a.score);
              const place = byScore.findIndex(run => run.id === selectedId);
              return place >= 0 ? { place: place + 1, of: byScore.length } : null;
            })()}
            onBack={() => showDashboard(selectedGame)}
            onReplays={() => showReplays(selectedGame, selectedId)}
            onPlay={(key, sample) => openLevel(key, sample)}
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
        ) : overview ? (
          <Overview runs={runs} now={snapshotNow} onRun={run => go({ run: run.id })} onGame={game => showDashboard(game)} />
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
  // Cards are grouped by family, newest model first; the badge keeps the overall place by score.
  const places = new Map(runs.filter(run => !run.live).map((run, index) => [run.id, index + 1]));
  const families = groupByFamily(runs);
  // A model run at several thinking depths gets a summary row with one child row per run.
  const modelRow = (model: string, modelRuns: RunSummary[]) => {
    const best = modelRuns.reduce((top, run) => run.score > top.score ? run : top);
    const share = best.total > 0 ? Math.max(0, Math.min(1, best.score / best.total)) : 0;
    return (
      <tr key={`model:${model}`} className="run-row run-model-row" onClick={() => onSelect(best)} style={{ "--series": colors.get(best.id) } as React.CSSProperties}>
        <td className="rank" />
        <td className="run-model">
          <button type="button" onClick={event => { event.stopPropagation(); onSelect(best); }}>{model}</button>
          <small>{modelRuns.length} 个思考深度 · 最好是 {subLabel(best, labels.get(best.id))}</small>
        </td>
        <td className="run-score">
          <span><b>{best.score}</b><small> / {best.total || "—"}</small><small className="share">{Math.round(share * 100)}%</small></span>
          <span className="overview-bar" aria-hidden="true"><i style={{ width: `${Math.max(2, share * 100)}%` }} /></span>
        </td>
        <td className="optional" />
        <td className="optional" />
      </tr>
    );
  };
  const runRow = (run: RunSummary, name: string, child: boolean) => {
    const status = runStatus(run);
    const place = places.get(run.id) ?? null;
    const share = run.total > 0 ? Math.max(0, Math.min(1, run.score / run.total)) : 0;
    return (
      // The row is the click target; the name is its button for the keyboard.
      <tr key={run.id} className={`run-row ${child ? "run-child" : ""}`} onClick={() => onSelect(run)} style={{ "--series": colors.get(run.id) } as React.CSSProperties}>
        <td className="rank">{place !== null ? <span className="run-rank" aria-label={`第 ${place} 名`}>{place}</span> : <i className="live-pulse" aria-label="直播中" />}</td>
        <td className="run-model">
          <button type="button" onClick={event => { event.stopPropagation(); onSelect(run); }}>{name}</button>
          <small>
            <span className={`status-badge ${status.className}`} title={status.detail}>{status.label}</span>
            {[child && name.includes(harnessName(run.agent)) ? "" : harnessName(run.agent), status.tone === "ended" ? status.detail : ""].filter(Boolean).join(" · ")}
          </small>
        </td>
        <td className="run-score">
          <span><b>{run.score}</b><small> / {run.total || "—"}</small><small className="share">{Math.round(share * 100)}%</small></span>
          <span className="overview-bar" aria-hidden="true"><i style={{ width: `${Math.max(2, share * 100)}%` }} /></span>
        </td>
        <td className="optional run-objective" title={run.objective}>{run.live ? "正在玩" : "停在"} {run.objective}</td>
        <td className="optional run-time">{durationLabel(taskDuration(run, now))}<small>{sinceLastScore(run, now)}</small></td>
      </tr>
    );
  };
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
            {families.flatMap(family => family.runs).map(run => <Toggle.Root className="series-toggle" key={run.id} style={{ "--series": colors.get(run.id) } as React.CSSProperties} pressed={selected.has(run.id)} disabled={!selected.has(run.id) && curves.length >= 8} onPressedChange={pressed => setCompared(() => { const next = new Set(selected); if (pressed) next.add(run.id); else next.delete(run.id); return next; })}><span className="series-toggle-check"><Check size={12} aria-hidden="true" /></span><span>{labels.get(run.id)}</span></Toggle.Root>)}
          </div>
        </section>
      )}

      {runs.length > 0 && (
        <section className="run-table-card">
          <table className="run-table">
            <thead>
              <tr>
                <th scope="col" className="rank">名次</th>
                <th scope="col">模型</th>
                <th scope="col">得分</th>
                <th scope="col" className="optional">进度</th>
                <th scope="col" className="optional">运行</th>
              </tr>
            </thead>
            {families.map(({ family, runs: familyRuns }) => (
              <tbody key={family}>
                <tr className="run-table-family"><th scope="rowgroup" colSpan={5}>{family}<small>{familyRuns.length} 次运行</small></th></tr>
                {groupByModel(familyRuns).flatMap(({ model, runs: modelRuns }) => modelRuns.length === 1
                  ? [runRow(modelRuns[0], labels.get(modelRuns[0].id) ?? model, false)]
                  : [modelRow(model, modelRuns), ...modelRuns.map(run => runRow(run, subLabel(run, labels.get(run.id)), true))])}
              </tbody>
            ))}
          </table>
        </section>
      )}
      {!runs.length && <EmptyState title="还没有运行" body="新的运行开始后会出现在这里。" />}
    </div>
  );
}

/** Score increases, newest first, from the run's score history. */
function recentScores(run: RunSummary, limit: number) {
  const history = run.score_history ?? [];
  const changes: { score: number; delta: number; elapsed: number; at: number }[] = [];
  for (let index = 1; index < history.length; index += 1) {
    const delta = history[index].score - history[index - 1].score;
    if (delta) changes.push({ score: history[index].score, delta, elapsed: history[index].elapsed_ms ?? 0, at: history[index].timestamp_ms });
  }
  return changes.slice(-limit).reverse();
}

function relativeTime(timestamp: number | null | undefined, now: number) {
  if (!timestamp) return "—";
  const seconds = Math.max(0, Math.round((now - timestamp) / 1000));
  if (seconds < 60) return `${seconds} 秒前`;
  if (seconds < 3600) return `${Math.round(seconds / 60)} 分钟前`;
  return clockTime(timestamp);
}

function LiveRun({
  run,
  now,
  standing,
  onBack,
  onReplays,
  onPlay,
}: {
  run: RunDetail | RunSummary | null;
  now: number;
  standing: { place: number; of: number } | null;
  onBack: () => void;
  onReplays: () => void;
  onPlay: (key: string, sample: LevelSample) => void;
}) {
  const [allMessages, setAllMessages] = useState(false);
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
  const messages = runDetail.recent_activity ?? [];
  const scores = recentScores(run, 12);

  return (
    <div className={`detail-page live-page ${run.live ? "is-live" : ""}`} style={{ "--accent": meta.accent } as React.CSSProperties}>
      <header className="detail-heading">
        <button className="back-button" onClick={onBack}>
          ← 返回 {meta.short} 成绩
        </button>
        <div className="detail-title-row">
          <div>
            <h1 className="model-title">
              <small className="model-family">{modelFamily(run.model)}</small>
              {modelName(run.model)}
              {run.effort !== "default" && <span>{run.effort}</span>}
            </h1>
            <div className="live-badges">
              <span className={`status-badge ${status.className}`} title={status.detail}>
                {run.live && <i className="live-pulse" aria-hidden="true" />}
                {status.label}
              </span>
              {standing && <span className="standing">{meta.short} 第 {standing.place} 名 / 共 {standing.of} 次运行</span>}
            </div>
          </div>
          <div className="score-hero">
            <span>{run.live ? "当前得分" : "最终得分"}</span>
            <strong>{run.score}</strong>
            <small>/ {run.total || "—"}</small>
          </div>
        </div>
        <div className="detail-strip">
          {status.tone === "ended" && <span>{status.detail}</span>}
          {run.termination?.kind !== "no_agent" && <span>运行 {durationLabel(elapsed)}</span>}
          {run.termination?.kind !== "no_agent" && <span>{sinceLastScore(run, now)}</span>}
          {harnessName(run.agent) && <span>{harnessName(run.agent)}</span>}
          {run.started_at && <span>{new Intl.DateTimeFormat("zh-CN", { month: "numeric", day: "numeric" }).format(run.started_at)} 开始</span>}
        </div>
      </header>

      <section className="live-stage">
        <div className="environment-card">
          <div className="section-title environment-title">
            <strong title={environmentContext ?? undefined}>{run.live ? "正在玩 " : "停在 "}{run.objective}</strong>
            <small>{run.live ? `最近动作 ${relativeTime(run.last_activity_at, now)}` : `结束于 ${clockTime(run.finished_at ?? run.last_activity_at)}`}</small>
          </div>
          <GameState game={run.game} state={state} previousState={null} />
          <div className="live-footer">
            <span>{run.live ? "画面随 Agent 的每一步实时更新" : "这是运行结束时的画面"}</span>
            <button type="button" className="replay-export-button" onClick={onReplays}>看这次运行的关卡回放</button>
          </div>
        </div>
        <aside className={`agent-feed ${allMessages ? "expanded" : ""}`} aria-label="Agent 实况">
          <div className="section-title">
            <strong>Agent 实况</strong>
            <small>{messages.length ? `最近 ${messages.length} 条` : ""}</small>
          </div>
          <div className="agent-feed-list">
            {messages.length
              ? messages.map((item, index) => (
                <article key={`${item.timestamp_ms}-${index}`} className={index === 0 ? "latest" : ""}>
                  <time>{index === 0 && run.live ? relativeTime(item.timestamp_ms, now) : clockTime(item.timestamp_ms)}</time>
                  <p>{item.text}</p>
                </article>
              ))
              : <EmptyState title="还没有可见消息" body="Agent 在运行中说的话会出现在这里。" />}
          </div>
          {messages.length > 4 && (
            <button type="button" className="agent-feed-more" onClick={() => setAllMessages(value => !value)}>
              {allMessages ? "收起" : `展开全部 ${messages.length} 条`}
            </button>
          )}
        </aside>
      </section>

      <section className="live-lower">
        <section className="chart-card">
          <div className="section-title">
            <strong>分数轨迹</strong>
            <small>横轴是累计有效运行时间</small>
          </div>
          <ScoreChart runs={[run]} now={now} colors={new Map([[run.id, meta.accent]])} />
        </section>
        <section className="score-log">
          <div className="section-title"><strong>最近得分</strong></div>
          {scores.length ? (
            <ol>
              {scores.map(change => (
                <li key={`${change.elapsed}-${change.score}`}>
                  <b className={change.delta > 0 ? "up" : "down"}>{change.delta > 0 ? `+${change.delta}` : change.delta}</b>
                  <span>到 {change.score} 分</span>
                  <small>第 {durationLabel(change.elapsed)}{change.at ? ` · ${clockTime(change.at)}` : ""}</small>
                </li>
              ))}
            </ol>
          ) : <EmptyState title="还没有得分" body="" />}
        </section>
      </section>

      <RunReplayShelf run={runDetail} onPlay={onPlay} onAll={onReplays} />
      <ExperiencePanel experience={runDetail.agent_experience} />
      <LiveDisclosure className="runtime-details" title="运行信息"><dl><dt>运行 ID</dt><dd>{run.id}</dd><dt>Agent</dt><dd>{run.agent}</dd><dt>停止原因</dt><dd>{run.termination?.reason ?? "—"}</dd><dt>最近动作</dt><dd>{clockTime(run.last_activity_at)}</dd></dl></LiveDisclosure>
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
