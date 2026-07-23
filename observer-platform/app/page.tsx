"use client";

import * as ScrollArea from "@radix-ui/react-scroll-area";
import * as Toggle from "@radix-ui/react-toggle";
import {
  ChevronLeft,
  ChevronRight,
  ChevronsLeft,
  ChevronsRight,
  CircleSlash2,
  Map as MapIcon,
  Pause,
  Play,
  Radio,
  Rewind,
  type LucideIcon,
} from "lucide-react";
import { useEffect, useMemo, useState, type ButtonHTMLAttributes } from "react";

import {
  GAME_IDS,
  GAME_META,
  GameState,
  describeGameEvent,
  gameStateContext,
  type GameId,
} from "./game-registry";
import {
  EmptyState,
  Metric,
  asString,
  type Json,
  type ObserverEvent,
} from "./game-observer";

type ScorePoint = { timestamp_ms: number; elapsed_ms?: number; score: number };
type ElapsedScorePoint = { elapsed_ms: number; score: number };

type RunSummary = {
  id: string;
  job: string;
  trial: string;
  task_id: string;
  task: string;
  game: GameId;
  model: string;
  model_id: string;
  agent: string;
  effort: string;
  live: boolean;
  status: string;
  termination: {
    kind: "live" | "resumable" | "agent_stopped" | "completed" | "stopped" | "no_agent";
    resumable: boolean;
    reason?: string | null;
  };
  sidecar_only: boolean;
  score: number;
  total: number;
  objective: string;
  started_at?: number | null;
  finished_at?: number | null;
  last_activity_at?: number | null;
  last_score_at?: number | null;
  last_score_elapsed_ms?: number | null;
  consumed_ms?: number;
  observed_at?: number;
  latest_sequence: number;
  latest_action?: Record<string, Json> | null;
  latest_result?: Record<string, Json> | null;
  score_history?: ScorePoint[];
};

type RunDetail = RunSummary & {
  state: Record<string, Json>;
  asset_refs?: Record<string, ObserverAssetReference>;
  replay_groups: ReplayGroupSummary[];
  agent_experience: {
    updated_at?: number | null;
    source_count: number;
    counts: Record<"plan" | "verified" | "rejected" | "solved", number>;
    plan: string[];
    verified: string[];
    rejected: string[];
    solved: string[];
  };
};

type ReplayFrame = {
  key: string;
  event: ObserverEvent;
  operation_sequence: number;
  operation_timestamp_ms?: number | null;
  operation_action: Record<string, Json>;
  instruction_index: number;
  instruction_count: number;
  operation_size: number;
  has_instruction_trace: boolean;
};

type ReplayOperation = {
  sequence: number;
  timestamp_ms?: number | null;
  action: Record<string, Json>;
  first_frame_key: string;
  last_frame_key: string;
  frame_count: number;
  score_delta: number;
};

type ReplayAttemptSummary = {
  id: number;
  successful: boolean;
  score?: number | null;
};

type ReplayGroupSummary = {
  kind: "level" | "overworld";
  reference: string;
  title?: string | null;
  score?: number | null;
  attempts: ReplayAttemptSummary[];
};

type LoadedAttemptReplay = {
  attempt_id: number;
  kind: "level" | "overworld";
  reference?: string | null;
  title?: string | null;
  score: number;
  successful: boolean;
  asset_refs?: Record<string, ObserverAssetReference>;
  frames: ReplayFrame[];
  operations: ReplayOperation[];
  activity: { timestamp_ms: number; text: string }[];
  skipped_unchanged: number;
  eliminated_history_frames: number;
};

type ObserverAssetReference = {
  id: string;
  media_type: string;
  bytes: number;
};

const SERIES_COLORS = [
  "#a3ff62",
  "#5ed7ff",
  "#ffad57",
  "#c19cff",
  "#ff6f7d",
  "#f6df5f",
  "#62e5ba",
  "#86a8ff",
];

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

function taskDuration(run: RunSummary, now: number) {
  if (typeof run.consumed_ms === "number") {
    const liveTail = run.live ? Math.max(0, now - (run.observed_at ?? now)) : 0;
    return run.consumed_ms + liveTail;
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

const RUN_STATUS = {
  live: { label: "LIVE", className: "live" },
  resumable: { label: "STOP · 可续跑", className: "resumable" },
  agent_stopped: { label: "STOP · AGENT 主动", className: "agent-stopped" },
  completed: { label: "DONE", className: "completed" },
  stopped: { label: "STOP", className: "finished" },
  no_agent: { label: "NO AGENT", className: "waiting" },
} as const;

function runStatus(run: RunSummary) {
  return RUN_STATUS[run.termination?.kind ?? (run.live ? "live" : "stopped")];
}

function WallClock() {
  const [timestamp, setTimestamp] = useState(() => Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setTimestamp(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, []);
  return (
    <time>
      {new Intl.DateTimeFormat("zh-CN", {
        hour: "2-digit",
        minute: "2-digit",
        second: "2-digit",
        hour12: false,
      }).format(timestamp)}
    </time>
  );
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

function withSharedGameState(
  game: GameId,
  state: Record<string, Json>,
  latest: Record<string, Json>,
) {
  if (game !== "sausage" || state.overworld_map || !latest.overworld_map) return state;
  return { ...state, overworld_map: latest.overworld_map };
}

function gatewayUrl(path: string) {
  const local = ["localhost", "127.0.0.1"].includes(window.location.hostname);
  const origin = import.meta.env.VITE_LIVE_GATEWAY_ORIGIN ?? "http://127.0.0.1:3740";
  return new URL(local ? `${origin}${path}` : `/api/live${path}`, window.location.origin);
}

async function hydrateRunAssets(detail: RunDetail) {
  const entries = Object.entries(detail.asset_refs ?? {});
  if (!entries.length) return detail;
  const assets = await Promise.all(
    entries.map(async ([name, reference]) => {
      const response = await fetch(gatewayUrl(`/v1/assets/${reference.id}`), {
        cache: "force-cache",
      });
      if (!response.ok) throw new Error(`asset ${reference.id}: HTTP ${response.status}`);
      return [name, await response.json()] as const;
    }),
  );
  return { ...detail, state: { ...detail.state, ...Object.fromEntries(assets) } };
}

export default function Home() {
  const [runs, setRuns] = useState<RunSummary[]>([]);
  const [selectedGame, setSelectedGame] = useState<GameId>("parabox");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [detail, setDetail] = useState<RunDetail | null>(null);
  const [connection, setConnection] = useState<"connecting" | "live" | "offline">("connecting");
  const [paused, setPaused] = useState(false);
  const [snapshotNow, setSnapshotNow] = useState(() => Date.now());

  useEffect(() => {
    const fromUrl = new URL(window.location.href).searchParams.get("run");
    if (fromUrl) queueMicrotask(() => setSelectedId(fromUrl));
  }, []);

  useEffect(() => {
    if (paused) return;
    const url = gatewayUrl("/v1/subscribe");
    if (selectedId) url.searchParams.set("run_id", selectedId);
    const subscription = new EventSource(url);
    let cancelled = false;
    let loadedRevision = -1;
    let loadingRevision = -1;
    async function loadDetail(observedAt: number, revision: number) {
      if (!selectedId || revision === loadedRevision || revision === loadingRevision) return;
      loadingRevision = revision;
      try {
        const response = await fetch(gatewayUrl(`/v1/runs/${encodeURIComponent(selectedId)}`), {
          cache: "no-store",
        });
        if (!response.ok) throw new Error(`HTTP ${response.status}`);
        const payload = await response.json() as { run: RunDetail };
        const hydrated = await hydrateRunAssets(payload.run);
        if (!cancelled) {
          loadedRevision = revision;
          setDetail({ ...hydrated, observed_at: observedAt });
        }
      } catch {
        if (!cancelled) setConnection("offline");
      } finally {
        if (loadingRevision === revision) loadingRevision = -1;
      }
    }
    subscription.onopen = () => setConnection("live");
    subscription.onerror = () => setConnection("offline");
    subscription.onmessage = (event) => {
      try {
        const payload = JSON.parse(event.data) as {
          generated_at?: number;
          revision?: number;
          runs?: RunSummary[];
        };
        const observedAt = payload.generated_at ?? Date.now();
        setSnapshotNow(observedAt);
        const next = Array.isArray(payload.runs)
          ? payload.runs.map((run) => ({ ...run, observed_at: observedAt }))
          : [];
        setRuns(next);
        setConnection("live");
        if (selectedId) {
          const selected = next.find((run) => run.id === selectedId);
          if (selected) setSelectedGame(selected.game);
          void loadDetail(observedAt, payload.revision ?? selected?.latest_sequence ?? 0);
        }
      } catch {
        setConnection("offline");
      }
    };
    return () => {
      cancelled = true;
      subscription.close();
    };
  }, [selectedId, paused]);

  const grouped = useMemo(
    () =>
      Object.fromEntries(
        GAME_IDS.map((game) => [
          game,
          runs.filter((run) => run.game === game),
        ]),
      ) as Record<GameId, RunSummary[]>,
    [runs],
  );
  const visibleRuns = grouped[selectedGame];
  const liveCount = runs.filter((run) => run.live).length;
  function selectRun(run: RunSummary) {
    setDetail(null);
    setSelectedId(run.id);
    setSelectedGame(run.game);
    const url = new URL(window.location.href);
    url.searchParams.set("run", run.id);
    window.history.replaceState({}, "", url);
  }

  function showDashboard(game = selectedGame) {
    setSelectedGame(game);
    setSelectedId(null);
    setDetail(null);
    const url = new URL(window.location.href);
    url.searchParams.delete("run");
    window.history.replaceState({}, "", url);
  }

  return (
    <main className="live-shell">
      <header className="topbar">
        <button
          className="brand"
          onClick={() => showDashboard("parabox")}
          aria-label="返回直播大盘"
        >
          <span>3720</span>
          <strong>Benchmark Live</strong>
        </button>
        <div className={`ingest-status ${connection}`}>
          <i />
          {paused
            ? "画面已暂停"
            : connection === "live"
              ? `${liveCount} RUNS LIVE`
              : connection.toUpperCase()}
        </div>
        <div className="top-actions">
          <WallClock />
          <button onClick={() => setPaused((value) => !value)}>{paused ? "继续" : "暂停"}</button>
        </div>
      </header>

      <aside className="run-sidebar">
        <div className="sidebar-label">GAMES / MODELS</div>
        {GAME_IDS.map((game) => {
          const meta = GAME_META[game];
          const gameRuns = grouped[game];
          return (
            <section
              className="game-group"
              key={game}
              style={{ "--accent": meta.accent } as React.CSSProperties}
            >
              <button
                className={`game-heading ${selectedGame === game && !selectedId ? "active" : ""}`}
                onClick={() => showDashboard(game)}
                aria-pressed={selectedGame === game && !selectedId}
              >
                <span>{meta.short}</span>
                <small>{gameRuns.length}</small>
              </button>
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
                      <strong>{run.model}</strong>
                      <small>{run.objective}</small>
                    </span>
                    <b>{run.score}</b>
                  </button>
                ))}
                {!gameRuns.length && <div className="no-runs">暂无真实运行</div>}
              </div>
            </section>
          );
        })}
      </aside>

      <section className="content">
        {selectedId ? (
          <RunDetails
            key={selectedId}
            run={detail ?? runs.find((run) => run.id === selectedId) ?? null}
            now={snapshotNow}
            onBack={() => showDashboard(selectedGame)}
          />
        ) : (
          <GameDashboard game={selectedGame} runs={visibleRuns} now={snapshotNow} onSelect={selectRun} />
        )}
      </section>
    </main>
  );
}

function GameDashboard({
  game,
  runs,
  now,
  onSelect,
}: {
  game: GameId;
  runs: RunSummary[];
  now: number;
  onSelect: (run: RunSummary) => void;
}) {
  const meta = GAME_META[game];
  const leader = runs.slice().sort((a, b) => b.score - a.score)[0];
  return (
    <div className="dashboard-page" style={{ "--accent": meta.accent } as React.CSSProperties}>
      <header className="page-heading">
        <div>
          <p className="eyebrow">LIVE SCOREBOARD / {meta.short}</p>
          <h1>{meta.label}</h1>
          <p className="page-subtitle">
            每条曲线对应一个模型的连续私有 sidecar 状态链；点击模型进入当前测试。
          </p>
        </div>
        <div className="heading-metrics">
          <Metric label="MODELS" value={String(runs.length)} />
          <Metric label="ACTIVE" value={String(runs.filter((run) => run.live).length)} />
          <Metric label="LEADER" value={leader ? `${leader.score}` : "—"} />
        </div>
      </header>

      <section className="chart-card">
        <div className="section-title">
          <div>
            <span>SCORE / EFFECTIVE AGENT TIME</span>
            <strong>模型得分轨迹</strong>
          </div>
          <small>横轴累计有效运行时间 · 纵轴得分</small>
        </div>
        <ScoreChart runs={runs} now={now} onSelect={onSelect} />
      </section>

      <section className="run-grid">
        {runs.map((run, index) => {
          const elapsed = taskDuration(run, now);
          const scoreSilence = noScoreDuration(run, now);
          const status = runStatus(run);
          return (
            <button
              className="run-card"
              key={run.id}
              onClick={() => onSelect(run)}
              style={
                { "--series": SERIES_COLORS[index % SERIES_COLORS.length] } as React.CSSProperties
              }
            >
              <div className="run-card-top">
                <span className={`status-badge ${status.className}`}>
                  {status.label}
                </span>
              </div>
              <h2 className="model-title">
                {run.model}
                <span>{run.effort.toUpperCase()}</span>
              </h2>
              <div className="score-block">
                <strong>{run.score}</strong>
                <span>/ {run.total || "—"}</span>
              </div>
              <p>{run.objective}</p>
              <div className="run-card-meta">
                <span>运行 {durationLabel(elapsed)}</span>
                <span>
                  未得分 {scoreSilence === null ? "全程" : durationLabel(scoreSilence)}
                </span>
              </div>
            </button>
          );
        })}
        {!runs.length && (
          <EmptyState
            title="尚无真实运行"
            body="直播网关在线后，新的 Harbor 游戏 run 会自动出现在这里。"
          />
        )}
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
  const frames = useMemo(() => attemptReplay?.frames ?? [], [attemptReplay]);
  const [cursorKey, setCursorKey] = useState<string | null>(null);
  const [playing, setPlaying] = useState(false);
  const [speed, setSpeed] = useState(1);

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
    : frames.findIndex((frame) => frame.key === cursorKey);
  const activeIndex = cursorIndex < 0 ? frames.length - 1 : cursorIndex;
  const activeFrame = frames[activeIndex] ?? null;
  const activeEvent = activeFrame?.event ?? null;
  const previousEvent = activeIndex > 0 ? frames[activeIndex - 1].event : null;
  const isLatest = activeIndex >= frames.length - 1;
  const followingLive = attemptReplay === null;
  const failedAttempts = (runDetail?.replay_groups ?? []).reduce(
    (total, group) => total + (group.kind === "level" ? group.attempts.filter((attempt) => !attempt.successful).length : 0),
    0,
  );

  useEffect(() => {
    if (!playing || !frames.length || isLatest) {
      if (playing && isLatest) queueMicrotask(() => setPlaying(false));
      return;
    }
    const timer = window.setTimeout(() => {
      setCursorKey(frames[activeIndex + 1]?.key ?? null);
    }, 450 / speed);
    return () => window.clearTimeout(timer);
  }, [activeIndex, frames, isLatest, playing, speed]);

  if (!run || !("state" in run)) {
    return <EmptyState title="正在接入测试" body="等待权威 sidecar 状态。" />;
  }
  const detail = run as RunDetail;
  const state = withSharedGameState(
    run.game,
    activeEvent?.state ?? detail.state ?? {},
    detail.state ?? {},
  );
  const previousState = previousEvent?.state
    ? withSharedGameState(run.game, previousEvent.state, detail.state ?? {})
    : null;
  const meta = GAME_META[run.game];
  const selectedAttempt = (detail.replay_groups ?? [])
    .flatMap((group) => group.attempts.map((attempt, index) => ({ group, attempt, index })))
    .find(({ attempt }) => attempt.id === attemptReplay?.attempt_id);
  const environmentTitle = selectedAttempt
    ? selectedAttempt.group.kind === "overworld"
      ? `${selectedAttempt.group.title ?? "Land's End"} / 大地图`
      : `${selectedAttempt.group.reference} / ${selectedAttempt.group.title ?? "未命名关卡"}`
    : run.objective;
  const environmentContext = gameStateContext(run.game, state);
  const elapsed = taskDuration(run, now);
  const scoreSilence = noScoreDuration(run, now);
  const status = runStatus(run);

  function moveCursor(index: number) {
    const bounded = Math.max(0, Math.min(frames.length - 1, index));
    setCursorKey(frames[bounded]?.key ?? null);
    setPlaying(false);
  }

  function togglePlayback() {
    if (playing) {
      setPlaying(false);
      return;
    }
    if (!frames.length) return;
    if (isLatest) setCursorKey(frames[0].key);
    setPlaying(true);
  }

  function followLive() {
    setAttemptReplay(null);
    setPendingAttemptReplay(null);
    setCursorKey(null);
    setPlaying(false);
  }

  async function loadAttemptReplay(attemptId: number) {
    if (!runDetail) return;
    if (attemptReplay?.attempt_id === attemptId) {
      setCursorKey(frames[0]?.key ?? null);
      setPlaying(frames.length > 1);
      return;
    }
    setCursorKey(null);
    setPlaying(false);
    setAttemptReplayLoading(true);
    setPendingAttemptReplay(attemptId);
    setAttemptReplayError("");
    try {
      const path = `/v1/runs/${encodeURIComponent(runDetail.id)}`;
      const url = gatewayUrl(path);
      url.searchParams.set("replay_attempt", String(attemptId));
      const response = await fetch(url, { cache: "no-store" });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      const payload = await response.json() as LoadedAttemptReplay & { schema: string };
      setCursorKey(payload.frames[0]?.key ?? null);
      setPlaying(payload.frames.length > 1);
      setAttemptReplay(payload);
    } catch {
      setAttemptReplayError("尝试回放加载失败");
    } finally {
      setAttemptReplayLoading(false);
      setPendingAttemptReplay(null);
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
            <p className="eyebrow">{run.task}</p>
            <h1 className="model-title">
              {run.model}
              <span>{run.effort.toUpperCase()}</span>
            </h1>
            <p className="page-subtitle">{run.objective}</p>
          </div>
          <div className="score-hero">
            <span>SCORE</span>
            <strong>{run.score}</strong>
            <small>/ {run.total || "—"}</small>
          </div>
        </div>
        <div className="detail-strip">
          <span className={`status-badge ${status.className}`}>
            {status.label}
          </span>
          {run.termination?.reason && <span>{run.termination.reason}</span>}
          <span>累计运行 {durationLabel(elapsed)}</span>
          <span>未得分 {scoreSilence === null ? "全程" : durationLabel(scoreSilence)}</span>
          <span>SEQ {run.latest_sequence}</span>
          <span>{run.agent}</span>
        </div>
      </header>

      <section className="detail-grid">
        <div className="environment-card">
          <div className="section-title environment-title">
            <strong>{environmentTitle}</strong>
            {environmentContext && <small>{environmentContext}</small>}
          </div>
          <GameState game={run.game} state={state} previousState={previousState} />
          <ReplayTimeline
            frames={frames}
            activeIndex={activeIndex}
            isLatest={isLatest}
            followingLive={followingLive}
            playing={playing}
            speed={speed}
            failedAttempts={failedAttempts}
            skipFailedAttempts={skipFailedAttempts}
            replayGroups={runDetail?.replay_groups ?? []}
            selectedAttemptReplay={pendingAttemptReplay ?? attemptReplay?.attempt_id ?? null}
            attemptReplayLoading={attemptReplayLoading}
            pendingAttemptReplay={pendingAttemptReplay}
            attemptReplayError={attemptReplayError}
            onMove={moveCursor}
            onTogglePlayback={togglePlayback}
            onSpeed={setSpeed}
            onFollowLive={followLive}
            onSkipFailedAttempts={setSkipFailedAttempts}
            onAttemptReplay={loadAttemptReplay}
          />
        </div>
      </section>

      <ExperiencePanel experience={detail.agent_experience} />

      <section className="detail-chart chart-card">
        <div className="section-title">
          <div>
            <span>RUN HISTORY</span>
            <strong>分数轨迹</strong>
          </div>
          <small>累计有效时间 {durationLabel(taskDuration(run, now))}</small>
        </div>
        <ScoreChart runs={[run]} now={now} />
      </section>

      <section className="activity-grid">
        <div className="activity-card">
          <div className="section-title">
            <div>
              <span>ATTEMPT SESSION</span>
              <strong>此次尝试的 Agent 活动</strong>
            </div>
            <small>{selectedAttempt ? `${selectedAttempt.group.kind === "overworld" ? "大地图" : selectedAttempt.group.reference} · ${selectedAttempt.group.kind === "overworld" ? "路段" : "尝试"} ${selectedAttempt.index + 1}` : "随回放段按需载入"}</small>
          </div>
          <div className="activity-list">
            {attemptReplay?.activity.length ? (
              attemptReplay.activity.map((item, index) => (
                  <article key={`${item.timestamp_ms}-${index}`}>
                    <time>{clockTime(item.timestamp_ms)}</time>
                    <p>{item.text}</p>
                  </article>
                ))
            ) : (
              <EmptyState
                title={attemptReplay ? "此次尝试没有可见消息" : "选择一次尝试"}
                body={attemptReplay ? "权威 session 在该尝试时间窗内没有保存可展示消息。" : "Agent 活动会和所选尝试一起按需载入，不再展示整段 session。"}
              />
            )}
          </div>
        </div>
        <div className="event-card">
          <div className="section-title">
            <div>
              <span>ATTEMPT OPERATIONS</span>
              <strong>此次尝试的操作</strong>
            </div>
            <small>{attemptReplay ? `${attemptReplay.operations.length} 组` : "随尝试按需载入"}</small>
          </div>
          <div className="event-list">
            {attemptReplay?.operations.length ? (
              attemptReplay.operations.map((operation, operationIndex) => {
                const frameIndex = frames.findIndex(
                  (frame) => frame.key === operation.first_frame_key,
                );
                const frame = frames.find((candidate) => candidate.key === operation.last_frame_key);
                const previousOperation = operationIndex > 0
                  ? attemptReplay.operations[operationIndex - 1]
                  : null;
                const previousFrame = previousOperation
                  ? frames.find((candidate) => candidate.key === previousOperation.last_frame_key)
                  : null;
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
                  <span>{operation.score_delta ? `+${operation.score_delta} 分` : `${operation.frame_count} 帧`}</span>
                  <small>#{operation.sequence}</small>
                </button>
              )})
            ) : (
              <EmptyState
                title={attemptReplay ? "此次尝试没有有效操作" : "选择一次尝试"}
                body="Sidecar 操作会和所选尝试一起按需载入，不再展示整条事件流。"
              />
            )}
          </div>
        </div>
      </section>
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
  onMove,
  onTogglePlayback,
  onSpeed,
  onFollowLive,
  onSkipFailedAttempts,
  onAttemptReplay,
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
  onMove: (index: number) => void;
  onTogglePlayback: () => void;
  onSpeed: (speed: number) => void;
  onFollowLive: () => void;
  onSkipFailedAttempts: (enabled: boolean) => void;
  onAttemptReplay: (attemptId: number) => void;
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
  const selectedReplay = replayAttempts.find(
    ({ attempt }) => attempt.id === selectedAttemptReplay,
  );
  return (
    <section className="replay-panel" aria-label="状态回放时间轴">
      {frames.length > 0 && <div className="replay-toolbar">
        <div className="replay-controls" role="group" aria-label="回放操作">
          <ReplayIconButton label="跳到最早状态" icon={Rewind} onClick={() => onMove(0)} disabled={!frames.length || activeIndex <= 0} />
          <ReplayIconButton label="上一组操作" icon={ChevronsLeft} onClick={() => onMove(previousOperation)} disabled={previousOperation < 0} />
          <ReplayIconButton label="上一条有效指令" icon={ChevronLeft} onClick={() => onMove(activeIndex - 1)} disabled={!frames.length || activeIndex <= 0} />
          <ReplayIconButton className="play-button" label={playing ? "暂停回放" : "播放回放"} icon={playing ? Pause : Play} onClick={onTogglePlayback} disabled={frames.length < 2} />
          <ReplayIconButton label="下一条有效指令" icon={ChevronRight} onClick={() => onMove(activeIndex + 1)} disabled={!frames.length || isLatest} />
          <ReplayIconButton label="下一组操作" icon={ChevronsRight} onClick={() => onMove(nextOperation)} disabled={nextOperation < 0} />
          <ReplayIconButton label="返回直播" icon={Radio} onClick={onFollowLive} disabled={!frames.length || followingLive} />
          <Toggle.Root
            className="replay-icon replay-toggle"
            aria-label="跳过失败尝试（以重置为分界）"
            title="跳过失败尝试（以重置为分界）"
            pressed={skipFailedAttempts}
            onPressedChange={onSkipFailedAttempts}
            disabled={!failedAttempts}
          >
            <CircleSlash2 aria-hidden="true" size={17} strokeWidth={2} />
          </Toggle.Root>
          <label className="compact-select" title="回放速度">
            <span aria-hidden="true">×</span>
            <select aria-label="回放速度" value={speed} onChange={(event) => onSpeed(Number(event.target.value))}>
              <option value={0.5}>0.5</option>
              <option value={1}>1</option>
              <option value={2}>2</option>
              <option value={4}>4</option>
            </select>
          </label>
          <label className="replay-inline-scrubber">
            <span className="sr-only">选择有效回放画面</span>
            <input
              type="range"
              min="0"
              max={Math.max(0, frames.length - 1)}
              value={Math.max(0, activeIndex)}
              onChange={(event) => onMove(Number(event.target.value))}
              aria-label="选择有效回放画面"
            />
            <b>{active ? `${instructionLabel(active)} · ${actionLabel(active.event.action)}` : "—"}</b>
          </label>
        </div>
        <div className="replay-status" aria-live="polite">
          <i className={followingLive && selectedAttemptReplay === null ? "live" : "replay"} />
          {pendingAttemptReplay !== null
            ? "正在载入尝试回放…"
            : selectedReplay
              ? selectedReplay.group.kind === "overworld"
                ? `正在回放大地图 · 路段 ${selectedReplay.index + 1}`
                : `正在回放 ${selectedReplay.group.reference} · 尝试 ${selectedReplay.index + 1}`
              : followingLive
                ? "位于最新状态"
                : isLatest
                  ? "回看最新事件 · 未跟随"
                  : `回看 ${clockTime(active?.event.timestamp_ms)}`}
        </div>
      </div>}
      <div className="score-replay-picker">
        <ScrollArea.Root className="score-replay-scroll" type="auto">
          <ScrollArea.Viewport className="score-replay-viewport">
            <div className="score-replay-list" role="listbox" aria-label="选择回放段">
              {replayGroups.map((group) => (
                <section className={`score-replay-level ${group.kind}`} key={group.reference}>
                  <header>
                    <strong>{group.kind === "overworld" ? <MapIcon aria-label="大地图" size={14} /> : group.reference}</strong>
                    <span>{group.title ?? (group.kind === "overworld" ? "Land's End" : "未命名关卡")}</span>
                    <small>{group.kind === "overworld" ? "大地图" : `${group.score} 分`}</small>
                  </header>
                  <div>
                    {group.attempts
                      .map((attempt, index) => ({ attempt, index }))
                      .filter(({ attempt }) => group.kind === "overworld" || !skipFailedAttempts || attempt.successful)
                      .map(({ attempt, index }) => {
                      const selected = attempt.id === selectedAttemptReplay;
                      const loading = attempt.id === pendingAttemptReplay;
                      return (
                        <button
                          type="button"
                          role="option"
                          aria-selected={selected}
                          className={`${selected ? "selected" : ""} ${group.kind === "overworld" ? "map" : attempt.successful ? "scored" : "failed"}`}
                          key={attempt.id}
                          title={group.kind === "overworld" ? `${group.title ?? "Land's End"} · 大地图路段 ${index + 1}` : `${group.reference} / ${group.title ?? "未命名关卡"} · 尝试 ${index + 1}${attempt.successful ? ` · 得分 ${attempt.score}` : " · 未得分"}`}
                          onClick={() => onAttemptReplay(attempt.id)}
                          disabled={attemptReplayLoading && !loading}
                        >
                          <strong>{loading ? "…" : group.kind === "overworld" ? <MapIcon aria-hidden="true" size={15} /> : attempt.successful ? `+${attempt.score}` : "○"}</strong>
                          <small>{group.kind === "overworld" ? "路段" : "尝试"} {index + 1}</small>
                        </button>
                      );
                      })}
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
      </div>
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
  return (
    <section className="experience-card">
      <div className="section-title">
        <div>
          <span>EXPLICIT AGENT NOTES</span>
          <strong>Agent 经验板</strong>
        </div>
        <small>
          {experience?.updated_at
            ? `${experience.source_count} 份可见笔记 · 更新于 ${clockTime(experience.updated_at)}`
            : "仅投影 Agent 明确写入的可见 Markdown 条目"}
        </small>
      </div>
      <div className="experience-grid">
        {EXPERIENCE_SECTIONS.map(([key, eyebrow, title]) => {
          const items = experience?.[key] ?? [];
          return (
            <section key={key} data-experience={key}>
              <header>
                <span>{eyebrow}</span>
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
      <p className="experience-disclosure">不展示隐藏推理；分类只来自 Agent 自己保存的标题、项目符号和明确的 solved / rejected / verified 标记。</p>
    </section>
  );
}

function ScoreChart({
  runs,
  now,
  onSelect,
}: {
  runs: RunSummary[];
  now: number;
  onSelect?: (run: RunSummary) => void;
}) {
  const width = 960;
  const height = 284;
  const pad = { left: 52, right: 22, top: 24, bottom: 42 };
  const timelines = runs.map((run) => ({ run, points: scoreTimeline(run, now) }));
  const points = timelines.flatMap((timeline) => timeline.points);
  const maxDuration = Math.max(1, ...points.map((point) => point.elapsed_ms));
  const observedMax = Math.max(
    1,
    ...runs.map((run) => run.score),
    ...points.map((point) => point.score),
  );
  const yMax = observedMax <= 10 ? 10 : Math.ceil(observedMax / 10) * 10;
  const x = (value: number) => pad.left + (value / maxDuration) * (width - pad.left - pad.right);
  const y = (value: number) => pad.top + (1 - value / yMax) * (height - pad.top - pad.bottom);
  const ticks = Array.from({ length: 5 }, (_, index) => ({
    value: (yMax * index) / 4,
    y: y((yMax * index) / 4),
  }));

  return (
    <div className="score-chart-wrap">
      <svg
        className="score-chart"
        viewBox={`0 0 ${width} ${height}`}
        role="img"
        aria-label="模型分数随累计有效运行时间变化图"
      >
        {ticks.map((tick) => (
          <g key={tick.value}>
            <line
              x1={pad.left}
              x2={width - pad.right}
              y1={tick.y}
              y2={tick.y}
              className="grid-line"
            />
            <text x={pad.left - 10} y={tick.y + 4} textAnchor="end">
              {Math.round(tick.value)}
            </text>
          </g>
        ))}
        {[0, 0.25, 0.5, 0.75, 1].map((ratio) => {
          const duration = maxDuration * ratio;
          return (
            <text
              key={ratio}
              x={x(duration)}
              y={height - 12}
              textAnchor={ratio === 0 ? "start" : ratio === 1 ? "end" : "middle"}
            >
              {durationLabel(duration, maxDuration)}
            </text>
          );
        })}
        {timelines.map(({ run, points: history }, index) => {
          const color = SERIES_COLORS[index % SERIES_COLORS.length];
          const path = history
            .map((point, pointIndex) => {
              if (!pointIndex) return `M ${x(point.elapsed_ms)} ${y(point.score)}`;
              const previous = history[pointIndex - 1];
              return `L ${x(point.elapsed_ms)} ${y(previous.score)} L ${x(point.elapsed_ms)} ${y(point.score)}`;
            })
            .join(" ");
          const last = history[history.length - 1];
          return (
            <g
              key={run.id}
              className={onSelect ? "clickable-series" : ""}
              onClick={() => onSelect?.(run)}
            >
              <path d={path} fill="none" stroke={color} strokeWidth="3" />
              <circle cx={x(last.elapsed_ms)} cy={y(last.score)} r="4.5" fill={color} />
              <title>
                {run.model}: {run.score} / {durationLabel(last.elapsed_ms)}
              </title>
            </g>
          );
        })}
      </svg>
      <div className="chart-legend">
        {runs.map((run, index) => (
          <button key={run.id} onClick={() => onSelect?.(run)} disabled={!onSelect}>
            <i style={{ background: SERIES_COLORS[index % SERIES_COLORS.length] }} />
            <span>{run.model}</span>
            <strong>{run.score}</strong>
          </button>
        ))}
      </div>
    </div>
  );
}
