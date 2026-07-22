"use client";

import { useEffect, useMemo, useState } from "react";

import {
  GAME_IDS,
  GAME_META,
  GameState,
  describeGameEvent,
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
  sidecar_only: boolean;
  score: number;
  total: number;
  objective: string;
  started_at?: number | null;
  finished_at?: number | null;
  last_activity_at?: number | null;
  last_score_at?: number | null;
  consumed_ms?: number;
  observed_at?: number;
  latest_sequence: number;
  latest_action?: Record<string, Json> | null;
  latest_result?: Record<string, Json> | null;
  score_history: ScorePoint[];
};

type RunDetail = RunSummary & {
  state: Record<string, Json>;
  events: ObserverEvent[];
  agent_activity: { timestamp?: string | null; text: string }[];
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

function timeAgo(timestamp?: number | null, now = Date.now()) {
  if (!timestamp) return "无记录";
  const seconds = Math.max(0, Math.round((now - timestamp) / 1000));
  if (seconds < 60) return `${seconds}s 前`;
  if (seconds < 3600) return `${Math.round(seconds / 60)}m 前`;
  return `${(seconds / 3600).toFixed(1)}h 前`;
}

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
  const elapsed = run.score_history.map((point) => point.elapsed_ms ?? 0);
  if (elapsed.some((value) => value > 0)) return Math.max(...elapsed);
  const timestamps = run.score_history.map((point) => point.timestamp_ms);
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

function scoreTimeline(run: RunSummary, now: number): ElapsedScorePoint[] {
  const source = run.score_history.length
    ? run.score_history
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

function actionLabel(action?: Record<string, Json> | null) {
  return asString(action?.command, "STATE").toUpperCase();
}

function withSharedGameState(
  game: GameId,
  state: Record<string, Json>,
  latest: Record<string, Json>,
) {
  if (game !== "sausage" || state.overworld_map || !latest.overworld_map) return state;
  return { ...state, overworld_map: latest.overworld_map };
}

export default function Home() {
  const [runs, setRuns] = useState<RunSummary[]>([]);
  const [selectedGame, setSelectedGame] = useState<GameId>("parabox");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [detail, setDetail] = useState<RunDetail | null>(null);
  const [connection, setConnection] = useState<"connecting" | "live" | "offline">("connecting");
  const [paused, setPaused] = useState(false);
  const [now, setNow] = useState(0);

  useEffect(() => {
    queueMicrotask(() => setNow(Date.now()));
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    const fromUrl = new URL(window.location.href).searchParams.get("run");
    if (fromUrl) queueMicrotask(() => setSelectedId(fromUrl));
  }, []);

  useEffect(() => {
    if (paused) return;
    const local = ["localhost", "127.0.0.1"].includes(window.location.hostname);
    const gatewayOrigin = import.meta.env.VITE_LIVE_GATEWAY_ORIGIN ?? "http://127.0.0.1:3740";
    const url = new URL(
      local ? `${gatewayOrigin}/v1/subscribe` : "/api/live/subscribe",
      window.location.origin,
    );
    if (selectedId) url.searchParams.set("run_id", selectedId);
    const subscription = new EventSource(url);
    subscription.onopen = () => setConnection("live");
    subscription.onerror = () => setConnection("offline");
    subscription.onmessage = (event) => {
      try {
        const payload = JSON.parse(event.data) as {
          generated_at?: number;
          runs?: RunSummary[];
          run?: RunDetail | null;
        };
        const observedAt = payload.generated_at ?? Date.now();
        const next = Array.isArray(payload.runs)
          ? payload.runs.map((run) => ({ ...run, observed_at: observedAt }))
          : [];
        setRuns(next);
        setDetail(
          selectedId && payload.run
            ? { ...payload.run, observed_at: observedAt }
            : null,
        );
        setConnection("live");
        if (selectedId) {
          const selected = next.find((run) => run.id === selectedId);
          if (selected) setSelectedGame(selected.game);
        }
      } catch {
        setConnection("offline");
      }
    };
    return () => subscription.close();
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
          <time>
            {now
              ? new Intl.DateTimeFormat("zh-CN", {
                  hour: "2-digit",
                  minute: "2-digit",
                  second: "2-digit",
                  hour12: false,
                }).format(now)
              : "--:--:--"}
          </time>
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
                    <span
                      className={`run-dot ${run.live ? "live" : run.sidecar_only ? "waiting" : "finished"}`}
                    />
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
            run={detail ?? runs.find((run) => run.id === selectedId) ?? null}
            now={now}
            onBack={() => showDashboard(selectedGame)}
          />
        ) : (
          <GameDashboard game={selectedGame} runs={visibleRuns} now={now} onSelect={selectRun} />
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
        {runs.map((run, index) => (
          <button
            className="run-card"
            key={run.id}
            onClick={() => onSelect(run)}
            style={
              { "--series": SERIES_COLORS[index % SERIES_COLORS.length] } as React.CSSProperties
            }
          >
            <div className="run-card-top">
              <span
                className={`status-badge ${run.live ? "live" : run.sidecar_only ? "waiting" : "finished"}`}
              >
                {run.live ? "LIVE" : run.sidecar_only ? "NO AGENT" : "FINAL"}
              </span>
              <span>{run.effort.toUpperCase()}</span>
            </div>
            <h2>{run.model}</h2>
            <div className="score-block">
              <strong>{run.score}</strong>
              <span>/ {run.total || "—"}</span>
            </div>
            <p>{run.objective}</p>
            <div className="run-card-meta">
              <span>活动 {timeAgo(run.last_activity_at, now)}</span>
              <span>得分 {timeAgo(run.last_score_at, now)}</span>
            </div>
          </button>
        ))}
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
  const events = runDetail?.events ?? [];
  const [cursorSequence, setCursorSequence] = useState<number | null>(null);
  const [playing, setPlaying] = useState(false);
  const [speed, setSpeed] = useState(1);

  useEffect(() => {
    setCursorSequence(null);
    setPlaying(false);
  }, [run?.id]);

  useEffect(() => {
    if (
      cursorSequence !== null &&
      events.length > 0 &&
      !events.some((event) => event.sequence === cursorSequence)
    ) {
      queueMicrotask(() => {
        setCursorSequence(null);
        setPlaying(false);
      });
    }
  }, [cursorSequence, events]);

  const cursorIndex = cursorSequence === null
    ? events.length - 1
    : events.findIndex((event) => event.sequence === cursorSequence);
  const activeIndex = cursorIndex < 0 ? events.length - 1 : cursorIndex;
  const activeEvent = events[activeIndex] ?? null;
  const previousEvent = activeIndex > 0 ? events[activeIndex - 1] : null;
  const isLatest = activeIndex >= events.length - 1;
  const followingLive = cursorSequence === null;

  useEffect(() => {
    if (!playing || !events.length || isLatest) {
      if (playing && isLatest) queueMicrotask(() => setPlaying(false));
      return;
    }
    const timer = window.setTimeout(() => {
      setCursorSequence(events[activeIndex + 1]?.sequence ?? null);
    }, 900 / speed);
    return () => window.clearTimeout(timer);
  }, [activeIndex, events, isLatest, playing, speed]);

  if (!run) return <EmptyState title="正在接入测试" body="等待权威 sidecar 状态。" />;
  const detail = run as RunDetail;
  const state = withSharedGameState(
    run.game,
    activeEvent?.state ?? detail.state ?? {},
    detail.state ?? {},
  );
  const previousState = previousEvent?.state
    ? withSharedGameState(run.game, previousEvent.state, detail.state ?? {})
    : null;
  const eventDescription = activeEvent
    ? describeGameEvent(run.game, activeEvent, previousEvent)
    : null;
  const meta = GAME_META[run.game];

  function moveCursor(index: number) {
    const bounded = Math.max(0, Math.min(events.length - 1, index));
    setCursorSequence(events[bounded]?.sequence ?? null);
    setPlaying(false);
  }

  function togglePlayback() {
    if (playing) {
      setPlaying(false);
      return;
    }
    if (!events.length) return;
    if (isLatest) setCursorSequence(events[0].sequence);
    setPlaying(true);
  }

  function followLive() {
    setCursorSequence(null);
    setPlaying(false);
  }

  return (
    <div className="detail-page" style={{ "--accent": meta.accent } as React.CSSProperties}>
      <header className="detail-heading">
        <button className="back-button" onClick={onBack}>
          ← 返回 {meta.short} 大盘
        </button>
        <div className="detail-title-row">
          <div>
            <p className="eyebrow">
              {run.task} / {run.effort.toUpperCase()}
            </p>
            <h1>{run.model}</h1>
            <p className="page-subtitle">{run.objective}</p>
          </div>
          <div className="score-hero">
            <span>SCORE</span>
            <strong>{run.score}</strong>
            <small>/ {run.total || "—"}</small>
          </div>
        </div>
        <div className="detail-strip">
          <span
            className={`status-badge ${run.live ? "live" : run.sidecar_only ? "waiting" : "finished"}`}
          >
            {run.live ? "LIVE" : run.sidecar_only ? "SIDECAR ONLY" : "FINISHED"}
          </span>
          <span>最近活动 {timeAgo(run.last_activity_at, now)}</span>
          <span>最近得分 {timeAgo(run.last_score_at, now)}</span>
          <span>SEQ {run.latest_sequence}</span>
          <span>{run.agent}</span>
        </div>
      </header>

      <section className="detail-grid">
        <div className="environment-card">
          <div className="section-title">
            <div>
              <span>{cursorSequence === null ? "AUTHORITATIVE LIVE STATE" : "AUTHORITATIVE REPLAY"}</span>
              <strong>{cursorSequence === null ? "当前环境" : `回放事件 #${activeEvent?.sequence}`}</strong>
            </div>
            <small>{isLatest ? "最新状态" : `窗口内第 ${activeIndex + 1} / ${events.length} 步`}</small>
          </div>
          <GameState game={run.game} state={state} previousState={previousState} />
        </div>
        <div className={`operation-card ${eventDescription?.tone ?? "neutral"}`}>
          <div className="section-title">
            <div>
              <span>{eventDescription?.label ?? "WAITING FOR EVENT"}</span>
              <strong>{eventDescription?.title ?? "等待权威事件"}</strong>
            </div>
            <small>{activeEvent ? clockTime(activeEvent.timestamp_ms) : "—"}</small>
          </div>
          <div className="operation-summary">
            <p>{eventDescription?.detail ?? "Sidecar 尚未产生可回放状态。"}</p>
            <dl>
              <div><dt>事件</dt><dd>#{activeEvent?.sequence ?? "—"}</dd></div>
              <div><dt>命令</dt><dd>{actionLabel(activeEvent?.action)}</dd></div>
              <div><dt>得分</dt><dd>{activeEvent?.score ?? run.score}</dd></div>
              <div><dt>结果</dt><dd>{activeEvent?.result?.ok === false ? "未生效" : "已记录"}</dd></div>
            </dl>
          </div>
        </div>
      </section>

      <ReplayTimeline
        events={events}
        activeIndex={activeIndex}
        isLatest={isLatest}
        followingLive={followingLive}
        playing={playing}
        speed={speed}
        onMove={moveCursor}
        onTogglePlayback={togglePlayback}
        onSpeed={setSpeed}
        onFollowLive={followLive}
      />

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
              <span>NATIVE SESSION</span>
              <strong>Agent 当前活动</strong>
            </div>
            <small>仅展示保存 session 中的可见消息</small>
          </div>
          <div className="activity-list">
            {detail.agent_activity?.length ? (
              detail.agent_activity
                .slice()
                .reverse()
                .map((item, index) => (
                  <article key={`${item.timestamp}-${index}`}>
                    <time>{item.timestamp ? clockTime(Date.parse(item.timestamp)) : "—"}</time>
                    <p>{item.text}</p>
                  </article>
                ))
            ) : (
              <EmptyState
                title="暂无可见消息"
                body={
                  run.live
                    ? "Agent 正在推理，等待下一条可见活动。"
                    : "该 run 没有保存可展示的近期消息。"
                }
              />
            )}
          </div>
        </div>
        <div className="event-card">
          <div className="section-title">
            <div>
              <span>SIDECAR TRACE</span>
              <strong>最近事件</strong>
            </div>
            <small>append-only</small>
          </div>
          <div className="event-list">
            {events
              .slice()
              .reverse()
              .slice(0, 35)
              .map((event) => {
                const index = events.findIndex((candidate) => candidate.sequence === event.sequence);
                const description = describeGameEvent(
                  run.game,
                  event,
                  index > 0 ? events[index - 1] : null,
                );
                return (
                <button
                  className={`event-row ${event.sequence === activeEvent?.sequence ? "active" : ""}`}
                  key={event.sequence}
                  onClick={() => moveCursor(index)}
                  aria-current={event.sequence === activeEvent?.sequence ? "step" : undefined}
                >
                  <time>{clockTime(event.timestamp_ms)}</time>
                  <strong>{description.title}</strong>
                  <span>{event.score_delta ? `+${event.score_delta} 分` : description.label}</span>
                  <small>#{event.sequence}</small>
                </button>
              )})}
          </div>
        </div>
      </section>
    </div>
  );
}

function ReplayTimeline({
  events,
  activeIndex,
  isLatest,
  followingLive,
  playing,
  speed,
  onMove,
  onTogglePlayback,
  onSpeed,
  onFollowLive,
}: {
  events: ObserverEvent[];
  activeIndex: number;
  isLatest: boolean;
  followingLive: boolean;
  playing: boolean;
  speed: number;
  onMove: (index: number) => void;
  onTogglePlayback: () => void;
  onSpeed: (speed: number) => void;
  onFollowLive: () => void;
}) {
  const active = events[activeIndex];
  const scored = events
    .map((event, index) => ({ event, index }))
    .filter(({ event }) => (event.score_delta ?? 0) > 0);
  return (
    <section className="replay-card" aria-label="状态回放时间轴">
      <div className="replay-heading">
        <div>
          <span>REPLAY WINDOW · {events.length} EVENTS</span>
          <strong>{events.length ? `#${events[0].sequence} — #${events.at(-1)?.sequence}` : "暂无可回放事件"}</strong>
        </div>
        <div className="replay-status" aria-live="polite">
          <i className={followingLive ? "live" : "replay"} />
          {followingLive
            ? "位于最新状态"
            : isLatest
              ? "回看最新事件 · 未跟随"
              : `回看 ${clockTime(active?.timestamp_ms)}`}
        </div>
      </div>
      <div className="replay-controls">
        <button onClick={() => onMove(0)} disabled={!events.length || activeIndex <= 0}>最早</button>
        <button onClick={() => onMove(activeIndex - 1)} disabled={!events.length || activeIndex <= 0}>上一步</button>
        <button className="play-button" onClick={onTogglePlayback} disabled={events.length < 2}>
          {playing ? "暂停回放" : "播放回放"}
        </button>
        <button onClick={() => onMove(activeIndex + 1)} disabled={!events.length || isLatest}>下一步</button>
        <button onClick={onFollowLive} disabled={!events.length || followingLive}>返回直播</button>
        <label>
          速度
          <select value={speed} onChange={(event) => onSpeed(Number(event.target.value))}>
            <option value={0.5}>0.5×</option>
            <option value={1}>1×</option>
            <option value={2}>2×</option>
            <option value={4}>4×</option>
          </select>
        </label>
        <label>
          得分事件
          <select
            value=""
            onChange={(event) => event.target.value && onMove(Number(event.target.value))}
            disabled={!scored.length}
          >
            <option value="">跳转…</option>
            {scored.map(({ event, index }) => (
              <option value={index} key={event.sequence}>#{event.sequence} · +{event.score_delta}</option>
            ))}
          </select>
        </label>
      </div>
      <div className="replay-scrubber">
        <input
          type="range"
          min="0"
          max={Math.max(0, events.length - 1)}
          value={Math.max(0, activeIndex)}
          onChange={(event) => onMove(Number(event.target.value))}
          disabled={!events.length}
          aria-label="选择回放事件"
        />
        <div>
          <span>{events[0] ? `#${events[0].sequence}` : "—"}</span>
          <strong>{active ? `#${active.sequence} · ${actionLabel(active.action)}` : "等待事件"}</strong>
          <span>{events.at(-1) ? `#${events.at(-1)?.sequence}` : "—"}</span>
        </div>
      </div>
      <p className="replay-note">回放窗口来自 sidecar 最近 120 条完整状态；更早动作仍保留在运行产物中，但不会在此窗口伪装为可逐步回放。</p>
    </section>
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
