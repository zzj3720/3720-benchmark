"use client";
// Replays, kept apart from the live view: a library of levels with previews,
// a watch page per level that compares every model's attempts, and the player.

import { ChevronLeft, ChevronRight, ChevronsLeft, ChevronsRight, Download, Pause, Play, Rewind } from "lucide-react";
import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";

import { GAME_META, GameState, clearedGameState, describeGameEvent, resolveGameFrameState, type GameId } from "./game-registry";
import { EmptyState, REPLAY_CAPTURE_ATTRIBUTE, type ObserverEvent } from "./game-observer";
import { LiveActionMenu, LiveSelect, LiveSlider } from "./live-controls";
import type { LoadedAttemptReplay, ReplayFrame, ReplayGroupSummary, ReplayOperation, RunDetail, RunSummary } from "./live-contract";
import { RequestGate } from "./live-client";
import {
  EXPORT_HOLD_MS,
  FRAME_MS,
  ReplayIconButton,
  actionLabel,
  captionFrame,
  clockTime,
  fetchRunDetail,
  fileSlug,
  gatewayUrl,
  type ExportChoice,
} from "./live-shared";
import { modelName } from "./run-labels";
import type { ReplayExportFormat } from "./replay-export";
import { canvasExportSource, type CanvasExportSession } from "./webgl/export-source";

export type LevelAttempt = { id: number; successful: boolean; score: number; status: string };
export type LevelSample = { run: string; attempt: number };
export type LevelEntry = {
  key: string;
  reference: string;
  title: string | null;
  kind: string;
  attempts: number;
  passed_runs: number;
  runs: { run: string; attempts: number; passed: boolean; best: number }[];
  sample: LevelSample | null;
};
type LevelDetail = { key: string; reference: string; title: string | null; kind: string; runs: { run: string; attempts: LevelAttempt[] }[] };

function levelName(level: { reference: string; title: string | null; kind: string }) {
  if (level.kind === "overworld") return level.title ?? "大地图";
  return level.title && level.title !== level.reference ? `${level.reference} · ${level.title}` : level.reference;
}

async function fetchJson<T>(path: string, signal?: AbortSignal): Promise<T> {
  const response = await fetch(gatewayUrl(path), { cache: "no-store", signal });
  if (!response.ok) throw new Error(response.status === 404 ? "还没有回放数据" : `加载失败（${response.status}）`);
  return await response.json() as T;
}

function useLevelIndex(game: GameId) {
  const [levels, setLevels] = useState<LevelEntry[] | null>(null);
  const [error, setError] = useState("");
  useEffect(() => {
    const controller = new AbortController();
    fetchJson<{ levels: LevelEntry[] }>(`/v1/games/${game}/levels`, controller.signal)
      .then(value => { setLevels(value.levels); setError(""); })
      .catch(reason => { if (!controller.signal.aborted) setError(reason instanceof Error ? reason.message : "回放目录加载失败"); });
    return () => controller.abort();
  }, [game]);
  return { levels, error };
}

// ---------------------------------------------------------------- thumbnails

type ThumbnailRequest = (sample: LevelSample) => Promise<string | null>;
const ThumbnailContext = createContext<ThumbnailRequest | null>(null);
const thumbnailCache = new Map<string, Promise<string | null>>();

/**
 * Renders level previews with the game's own renderer. A hidden game view
 * provides the renderer; previews are drawn one at a time and cached.
 */
export function ThumbnailProvider({ game, baseRun, children }: { game: GameId; baseRun: string | undefined; children: React.ReactNode }) {
  const stage = useRef<HTMLDivElement>(null);
  const queue = useRef<Promise<unknown>>(Promise.resolve());
  const [base, setBase] = useState<RunDetail | null>(null);
  const [ready, setReady] = useState(false);
  useEffect(() => {
    if (!baseRun) return;
    let current = true;
    fetchRunDetail(baseRun).then(detail => { if (current) setBase(detail); }).catch(() => {});
    return () => { current = false; };
  }, [baseRun]);
  // Previews are requested only once the hidden game view has a renderer.
  useEffect(() => {
    if (!base) return;
    let current = true;
    void waitFor(() => stage.current ? canvasExportSource(stage.current) : undefined, 20_000)
      .then(renderer => { if (current && renderer) setReady(true); });
    return () => { current = false; };
  }, [base]);

  const request = useCallback<ThumbnailRequest>((sample) => {
    const key = `${game}:${sample.run}:${sample.attempt}`;
    let pending = thumbnailCache.get(key);
    if (!pending) {
      // Data loads in parallel; drawing takes turns on the one renderer.
      const data = Promise.all([
        fetchJson<LoadedAttemptReplay>(`/v1/runs/${encodeURIComponent(sample.run)}?replay_attempt=${sample.attempt}&preview=1&v=${REPLAY_VERSION}`),
        fetchRunDetail(sample.run),
      ]);
      pending = queue.current.then(async () => {
        const renderer = stage.current ? canvasExportSource(stage.current) : undefined;
        if (!renderer) return null;
        const [replay, detail] = await data;
        const first = replay.frames[0];
        if (!first) return null;
        const frame = { state: resolveGameFrameState(game, first.event.state ?? {}, detail.state ?? {}), previous: null };
        const session = await renderer.createSession([frame], 640, 400, 640 * 400);
        try {
          return session.capture(frame).toDataURL("image/webp", 0.82);
        } finally {
          session.dispose();
        }
      }).catch(() => null);
      queue.current = pending;
      thumbnailCache.set(key, pending);
      // Only successful previews are kept; a failure may succeed next time.
      void pending.then(url => { if (!url) thumbnailCache.delete(key); });
    }
    return pending;
  }, [game]);

  return (
    <ThumbnailContext.Provider value={ready ? request : null}>
      {children}
      {base && (
        <div className="thumb-stage" ref={stage} aria-hidden="true">
          <GameState game={game} state={base.state} previousState={null} />
        </div>
      )}
    </ThumbnailContext.Provider>
  );
}

async function waitFor<T>(probe: () => T | undefined, timeoutMs = 8000): Promise<T | undefined> {
  const deadline = performance.now() + timeoutMs;
  for (;;) {
    const value = probe();
    if (value || performance.now() > deadline) return value;
    await new Promise(resolve => setTimeout(resolve, 100));
  }
}

function LevelThumbnail({ sample, label }: { sample: LevelSample | null; label: string }) {
  const request = useContext(ThumbnailContext);
  const holder = useRef<HTMLDivElement>(null);
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    const element = holder.current;
    if (!element || !sample || !request) return;
    let current = true;
    const observer = new IntersectionObserver(entries => {
      if (!entries.some(entry => entry.isIntersecting)) return;
      observer.disconnect();
      void request(sample).then(value => { if (current) setUrl(value); });
    }, { rootMargin: "200px" });
    observer.observe(element);
    return () => { current = false; observer.disconnect(); };
  }, [request, sample]);
  return (
    <div className="level-thumb" ref={holder}>
      {url ? <img src={url} alt="" /> : <span>{label}</span>}
    </div>
  );
}

// ---------------------------------------------------------------- library

function levelOutcome(level: LevelEntry, run?: string) {
  const runs = run ? level.runs.filter(item => item.run === run) : level.runs;
  const passed = runs.filter(item => item.passed).length;
  if (run) return passed ? { tone: "passed", text: "已通过" } : { tone: "failed", text: "未通过" };
  if (level.kind === "overworld") return { tone: "map", text: `${level.attempts} 段` };
  return passed ? { tone: "passed", text: `${passed} 个模型通过` } : { tone: "failed", text: "无人通过" };
}

export function LevelCard({ level, run, selected, onOpen }: { level: LevelEntry; run?: string; selected?: boolean; onOpen: () => void }) {
  const outcome = levelOutcome(level, run);
  const attempts = run ? level.runs.find(item => item.run === run)?.attempts ?? 0 : level.attempts;
  return (
    <button type="button" className={`level-card ${outcome.tone} ${selected ? "selected" : ""}`} aria-pressed={selected} onClick={onOpen} title={levelName(level)}>
      <LevelThumbnail sample={level.sample} label={level.kind === "overworld" ? "大地图" : level.reference} />
      <span className="level-card-badge">{outcome.text}</span>
      <span className="level-card-count">{attempts} 次尝试</span>
      {selected && <span className="level-card-now">正在播放</span>}
      <strong>{levelName(level)}</strong>
    </button>
  );
}

export function ReplayLibrary({
  game,
  runs,
  labels,
  initialRun,
  onOpenLevel,
}: {
  game: GameId;
  runs: RunSummary[];
  labels: Map<string, string>;
  initialRun?: string | null;
  onOpenLevel: (key: string) => void;
}) {
  const { levels, error } = useLevelIndex(game);
  const [outcome, setOutcome] = useState<"all" | "passed" | "failed">("all");
  const [run, setRun] = useState(initialRun ?? "all");
  const runFilter = run === "all" ? undefined : run;
  const shown = (levels ?? []).filter(level => {
    const runs = runFilter ? level.runs.filter(item => item.run === runFilter) : level.runs;
    if (!runs.length) return false;
    const passed = runs.some(item => item.passed);
    return outcome === "all" || (outcome === "passed" ? passed : !passed);
  });
  const passedLevels = (levels ?? []).filter(level => level.kind !== "overworld" && level.passed_runs > 0).length;
  const totalLevels = (levels ?? []).filter(level => level.kind !== "overworld").length;
  return (
    <ThumbnailProvider game={game} baseRun={runs[0]?.id}>
      <section className="library">
        <div className="library-filters">
          <p>{levels ? `${totalLevels} 个关卡有回放，${passedLevels} 个至少有一个模型通过。点开一关，可以对比所有模型在这一关的每次尝试。` : "正在载入回放目录…"}</p>
          <div className="scale-switch" role="group" aria-label="按结果筛选">
            {([["all", "全部"], ["passed", "有人通过"], ["failed", "无人通过"]] as const).map(([value, text]) => (
              <button key={value} aria-pressed={outcome === value} onClick={() => setOutcome(value)}>{text}</button>
            ))}
          </div>
          <LiveSelect label="按模型筛选" value={run} onChange={setRun} options={[{ value: "all", label: "全部模型" }, ...runs.map(item => ({ value: item.id, label: labels.get(item.id) ?? modelName(item.model) }))]} />
        </div>
        {error && <EmptyState title="回放目录暂不可用" body={error} />}
        <div className="level-grid">
          {shown.map(level => <LevelCard key={level.key} level={level} run={runFilter} onOpen={() => onOpenLevel(level.key)} />)}
        </div>
        {levels && !shown.length && <EmptyState title="没有符合条件的关卡" body="换一个结果或模型筛选试试。" />}
      </section>
    </ThumbnailProvider>
  );
}

// ---------------------------------------------------------------- one run's shelf

/** Level index key: the first 16 hex digits of sha256(reference), as the publisher writes it. */
export async function levelKey(reference: string) {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(reference));
  return [...new Uint8Array(digest)].slice(0, 8).map(byte => byte.toString(16).padStart(2, "0")).join("");
}

const SHELF_SIZE = 12;

/**
 * The levels one run played, newest first, under its live view. Each card
 * opens that level with this run's latest attempt playing.
 */
export function RunReplayShelf({ run, onPlay, onAll }: { run: RunDetail; onPlay: (key: string, sample: LevelSample) => void; onAll: () => void }) {
  const groups = run.replay_groups ?? [];
  if (!groups.length) return null;
  const recent = groups.slice(-SHELF_SIZE).reverse();
  const levels = groups.filter(group => group.kind === "level");
  const passed = levels.filter(group => group.attempts.some(attempt => attempt.successful)).length;
  const partial = run.replay_catalog_more || groups.length > SHELF_SIZE;
  const open = async (group: ReplayGroupSummary) => {
    const attempt = group.attempts.at(-1);
    if (attempt) onPlay(await levelKey(group.reference), { run: run.id, attempt: attempt.id });
  };
  return (
    <ThumbnailProvider game={run.game} baseRun={run.id}>
      <section className="run-shelf">
        <div className="section-title">
          <strong>这个 Agent 的回放</strong>
          <small>{run.replay_catalog_more ? `最近 ${recent.length} 关` : `${levels.length} 关 · 通过 ${passed}`}</small>
        </div>
        <div className="level-grid">
          {recent.map(group => {
            const attempt = group.attempts.at(-1);
            const running = attempt?.status === "running";
            const won = group.attempts.some(item => item.successful);
            const tone = running ? "map" : group.kind === "overworld" ? "map" : won ? "passed" : "failed";
            const badge = running ? "进行中" : group.kind === "overworld" ? `${group.attempts.length} 段` : won ? "已通过" : "未通过";
            return (
              <button type="button" key={`${group.kind}:${group.reference}`} className={`level-card ${tone}`} onClick={() => void open(group)} title={levelName({ reference: group.reference, title: group.title ?? null, kind: group.kind })}>
                <LevelThumbnail sample={attempt ? { run: run.id, attempt: attempt.id } : null} label={group.kind === "overworld" ? "大地图" : group.reference} />
                <span className="level-card-badge">{badge}</span>
                <span className="level-card-count">{group.attempts.length} 次尝试</span>
                <strong>{levelName({ reference: group.reference, title: group.title ?? null, kind: group.kind })}</strong>
              </button>
            );
          })}
        </div>
        {partial && <button type="button" className="run-shelf-all" onClick={onAll}>查看这个 Agent 的全部回放</button>}
      </section>
    </ThumbnailProvider>
  );
}

// ---------------------------------------------------------------- watch page

export function LevelWatch({
  game,
  levelKey,
  play,
  runs,
  labels,
  colors,
  onOpenLevel,
  onPlay,
}: {
  game: GameId;
  levelKey: string;
  play: LevelSample | null;
  runs: RunSummary[];
  labels: Map<string, string>;
  colors: Map<string, string>;
  onOpenLevel: (key: string) => void;
  onPlay: (sample: LevelSample) => void;
}) {
  const { levels } = useLevelIndex(game);
  const [level, setLevel] = useState<LevelDetail | null>(null);
  const [error, setError] = useState("");
  useEffect(() => {
    const controller = new AbortController();
    setLevel(null);
    fetchJson<LevelDetail>(`/v1/games/${game}/levels/${levelKey}`, controller.signal)
      .then(value => { setLevel(value); setError(""); })
      .catch(reason => { if (!controller.signal.aborted) setError(reason instanceof Error ? reason.message : "关卡加载失败"); });
    return () => controller.abort();
  }, [game, levelKey]);

  // Order models like the dashboard does, and default to the best run's
  // first passing attempt (or its latest one).
  const rank = new Map(runs.map((run, index) => [run.id, index]));
  const levelRuns = (level?.runs ?? []).filter(item => rank.has(item.run)).sort((a, b) => rank.get(a.run)! - rank.get(b.run)!);
  const fallback = levelRuns.find(item => item.attempts.some(attempt => attempt.successful)) ?? levelRuns[0];
  const selected = play ?? (fallback ? { run: fallback.run, attempt: (fallback.attempts.find(attempt => attempt.successful) ?? fallback.attempts[fallback.attempts.length - 1]).id } : null);
  const selectedRun = levelRuns.find(item => item.run === selected?.run);
  const attemptIndex = selectedRun?.attempts.findIndex(attempt => attempt.id === selected?.attempt) ?? -1;
  const attempt = attemptIndex >= 0 ? selectedRun!.attempts[attemptIndex] : null;

  const [detail, setDetail] = useState<RunDetail | null>(null);
  useEffect(() => {
    if (!selected) return;
    let current = true;
    fetchRunDetail(selected.run).then(value => { if (current) setDetail(value); }).catch(() => {});
    return () => { current = false; };
  }, [selected?.run]);

  const position = levels?.findIndex(item => item.key === levelKey) ?? -1;
  const more = levels && position >= 0
    ? [...levels.slice(position + 1), ...levels.slice(0, position)].slice(0, 12)
    : [];
  const meta = GAME_META[game];
  const entry = position >= 0 ? levels![position] : null;

  return (
    <ThumbnailProvider game={game} baseRun={runs[0]?.id}>
      <div className="watch-page" style={{ "--accent": meta.accent } as React.CSSProperties}>
        <header className="watch-heading">
          <div>
            <h1>{level ? levelName(level) : entry ? levelName(entry) : "关卡回放"}</h1>
            <p>{level ? `${levelRuns.length} 个模型尝试过这一关，${levelRuns.filter(item => item.attempts.some(value => value.successful)).length} 个通过。` : "正在载入…"}</p>
          </div>
          <div className="watch-steps">
            <button type="button" disabled={position <= 0} onClick={() => onOpenLevel(levels![position - 1].key)}><ChevronLeft size={15} aria-hidden="true" />上一关</button>
            <button type="button" disabled={!levels || position < 0 || position >= levels.length - 1} onClick={() => onOpenLevel(levels![position + 1].key)}>下一关<ChevronRight size={15} aria-hidden="true" /></button>
          </div>
        </header>
        {error && <EmptyState title="关卡暂不可用" body={error} />}
        <div className="watch-layout">
          <div className="watch-main">
            {selected && detail && detail.id === selected.run && attempt ? (
              <ReplayPlayer
                key={`${selected.run}:${selected.attempt}`}
                run={detail}
                attemptId={selected.attempt}
                label={labels.get(selected.run) ?? modelName(detail.model)}
                caption={{ level: level ? levelName(level) : "", kind: level?.kind ?? "level", index: attemptIndex, successful: attempt.successful, score: attempt.score }}
              />
            ) : (
              <div className="environment-card watch-placeholder"><EmptyState title={level && !levelRuns.length ? "这一关还没有回放" : "正在载入回放…"} body="" /></div>
            )}
          </div>
          <aside className="attempt-board" aria-label="各模型在这一关的尝试">
            <h2>各模型的尝试</h2>
            {levelRuns.map(item => {
              const passed = item.attempts.some(value => value.successful);
              return (
                <section className="attempt-run" key={item.run} style={{ "--series": colors.get(item.run) } as React.CSSProperties}>
                  <header>
                    <i />
                    <strong>{labels.get(item.run) ?? item.run}</strong>
                    <span className={passed ? "passed" : "failed"}>{passed ? "通过" : "未通过"}</span>
                  </header>
                  <div className="attempt-chips">
                    {item.attempts.map((value, index) => {
                      const current = selected?.run === item.run && selected.attempt === value.id;
                      return (
                        <button
                          type="button"
                          key={value.id}
                          className={`${value.successful ? "scored" : value.status === "running" ? "running" : "failed"} ${current ? "selected" : ""}`}
                          aria-pressed={current}
                          title={`第 ${index + 1} 次尝试 · ${value.successful ? `通过 +${value.score}` : value.status === "running" ? "进行中" : "未通过"}`}
                          onClick={() => onPlay({ run: item.run, attempt: value.id })}
                        >
                          <strong>{index + 1}</strong>
                          <small>{value.successful ? `+${value.score}` : value.status === "running" ? "…" : "✕"}</small>
                        </button>
                      );
                    })}
                  </div>
                </section>
              );
            })}
          </aside>
        </div>
        {more.length > 0 && (
          <section className="more-levels">
            <h2>接着看</h2>
            <div className="level-grid">
              {more.map(item => <LevelCard key={item.key} level={item} onOpen={() => onOpenLevel(item.key)} />)}
            </div>
          </section>
        )}
      </div>
    </ThumbnailProvider>
  );
}

// ---------------------------------------------------------------- player

type Caption = { level: string; kind: string; index: number; successful: boolean; score: number };

export function ReplayPlayer({ run, attemptId, label, caption }: { run: RunDetail; attemptId: number; label: string; caption: Caption }) {
  const [replay, setReplay] = useState<LoadedAttemptReplay | null>(null);
  const [showUndone, setShowUndone] = useState(false);
  useEffect(() => { if (readShowUndone()) setShowUndone(true); }, []);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [cursor, setCursor] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [speed, setSpeed] = useState(1);
  const [exportOpen, setExportOpen] = useState(false);
  const [exportChoice, setExportChoice] = useState<ExportChoice>({ format: "video", speed: 2, caption: true });
  const [exporting, setExporting] = useState<{ format: ReplayExportFormat; completed: number; total: number } | null>(null);
  const [exportNotice, setExportNotice] = useState("");
  const stageRef = useRef<HTMLDivElement>(null);
  const exportAbort = useRef<AbortController | null>(null);
  const gate = useRef(new RequestGate());

  const load = useCallback(async () => {
    setLoading(true);
    setError("");
    const request = gate.current.begin();
    try {
      const payload = await fetchJson<LoadedAttemptReplay>(
        `/v1/runs/${encodeURIComponent(run.id)}?replay_attempt=${attemptId}&v=${REPLAY_VERSION}`,
        AbortSignal.any([request.signal, AbortSignal.timeout(60_000)]),
      );
      if (!request.current()) return;
      setReplay(payload);
      setCursor(0);
      setPlaying(payload.frames.length > 1);
    } catch {
      if (request.current()) setError("回放加载失败，请重新选择这次尝试");
    } finally {
      if (request.current()) setLoading(false);
    }
  }, [run.id, attemptId]);

  useEffect(() => {
    const requests = gate.current;
    void load();
    return () => { requests.cancel(); exportAbort.current?.abort(); };
  }, [load]);

  // The default view is the path the agent kept; the toggle adds what it undid.
  const allFrames = useMemo(() => replay?.frames ?? [], [replay]);
  const undoneCount = useMemo(() => allFrames.filter(frame => frame.undone).length, [allFrames]);
  const shownFrames = useMemo(() => showUndone ? allFrames : allFrames.filter(frame => !frame.undone), [allFrames, showUndone]);
  // Games that move on in the step that clears a level recorded the next
  // level there; show the cleared board instead.
  const frames = useMemo(() => {
    const final = shownFrames.at(-1);
    if (!replay?.successful || !final || shownFrames.length < 2 || !(final.event.score_delta ?? 0)) return shownFrames;
    const cleared = clearedGameState(run.game, shownFrames[shownFrames.length - 2].event.state ?? {}, final.event);
    return cleared ? [...shownFrames.slice(0, -1), { ...final, event: { ...final.event, state: cleared } }] : shownFrames;
  }, [shownFrames, replay, run.game]);
  const operations = useMemo(() => replayOperations(frames), [frames]);
  const last = frames.length - 1;
  const active = frames[Math.min(cursor, Math.max(0, last))];
  const toggleUndone = () => {
    const next = !showUndone;
    // Stay on the same moment: the same frame, or the last kept one before it.
    const position = active ? allFrames.findIndex(frame => frame.key === active.key) : -1;
    const shown = next ? allFrames : allFrames.filter(frame => !frame.undone);
    setCursor(Math.max(0, shown.findLastIndex(frame => allFrames.indexOf(frame) <= position)));
    setShowUndone(next);
    try { window.localStorage.setItem(SHOW_UNDONE_KEY, next ? "1" : "0"); } catch {}
  };

  useEffect(() => {
    if (!playing || exporting) return;
    if (cursor >= last) {
      queueMicrotask(() => setPlaying(false));
      return;
    }
    const timer = window.setTimeout(() => setCursor(value => value + 1), FRAME_MS / speed);
    return () => window.clearTimeout(timer);
  }, [playing, cursor, last, speed, exporting]);

  const move = useCallback((index: number) => {
    if (exporting) return;
    setPlaying(false);
    setCursor(Math.max(0, Math.min(last, index)));
  }, [exporting, last]);
  const toggle = useCallback(() => {
    if (exporting || frames.length < 2) return;
    if (playing) { setPlaying(false); return; }
    if (cursor >= last) setCursor(0);
    setPlaying(true);
  }, [exporting, frames.length, playing, cursor, last]);

  // Space plays or pauses, arrows step, Home goes to the start.
  const keys = useRef({ toggle, move, cursor });
  keys.current = { toggle, move, cursor };
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      if (event.metaKey || event.ctrlKey || event.altKey || target?.closest("input, textarea, select, [role=combobox], [role=menu], [contenteditable=true]")) return;
      const current = keys.current;
      if (event.key === " ") { event.preventDefault(); current.toggle(); }
      else if (event.key === "ArrowLeft") { event.preventDefault(); current.move(current.cursor - 1); }
      else if (event.key === "ArrowRight") { event.preventDefault(); current.move(current.cursor + 1); }
      else if (event.key === "Home") { event.preventDefault(); current.move(0); }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const state = useMemo(() => resolveGameFrameState(run.game, active?.event.state ?? {}, run.state ?? {}), [run.game, run.state, active]);
  const previousFrame = cursor > 0 ? frames[cursor - 1] : null;
  const previousState = useMemo(() => previousFrame ? resolveGameFrameState(run.game, previousFrame.event.state ?? {}, run.state ?? {}) : null, [run.game, run.state, previousFrame]);
  const operationStart = (direction: -1 | 1) => {
    const sequence = active?.operation_sequence;
    if (direction < 0) return frames.findLastIndex((frame, index) => index < cursor && frame.operation_sequence !== sequence);
    return frames.findIndex((frame, index) => index > cursor && frame.operation_sequence !== sequence);
  };
  const frameIndex = useMemo(() => new Map(frames.map((frame, index) => [frame.key, index])), [frames]);
  const meta = GAME_META[run.game];
  const attemptText = caption.kind === "overworld"
    ? `大地图 · 第 ${caption.index + 1} 段`
    : `${caption.level} · 第 ${caption.index + 1} 次尝试 · ${caption.successful ? `通过 +${caption.score}` : "未通过"}`;
  const exportSeconds = Math.round((frames.length * FRAME_MS / exportChoice.speed + EXPORT_HOLD_MS) / 1000);

  async function exportSegment({ format, speed: exportSpeed, caption: withCaption }: ExportChoice) {
    const element = stageRef.current?.querySelector<HTMLElement>(`[${REPLAY_CAPTURE_ATTRIBUTE}]`);
    if (!element || !frames.length || exporting) return;
    const abort = new AbortController();
    exportAbort.current = abort;
    setPlaying(false);
    setExportNotice("");
    setExporting({ format, completed: 0, total: frames.length });
    const captionCanvas = document.createElement("canvas");
    let session: CanvasExportSession | undefined;
    try {
      const { exportReplaySegment } = await import("./replay-export");
      const renderer = canvasExportSource(element);
      if (!renderer) throw new Error("画布尚未准备好，请稍后重试");
      const exportFrames = frames.map((frame, index) => ({
        state: resolveGameFrameState(run.game, frame.event.state ?? {}, run.state ?? {}),
        previous: index > 0 ? resolveGameFrameState(run.game, frames[index - 1].event.state ?? {}, run.state ?? {}) : null,
      }));
      session = await renderer.createSession(exportFrames, 900, 900, 80_000_000);
      const activeSession = session;
      const lines = { title: `${meta.label} · ${label}`, segment: attemptText };
      await exportReplaySegment({
        fileName: fileSlug(`${run.game}-${label}-${caption.level}-attempt-${caption.index + 1}`),
        format,
        frameCount: frames.length,
        frameDelayMs: FRAME_MS / exportSpeed,
        lastFrameHoldMs: EXPORT_HOLD_MS,
        signal: abort.signal,
        captureFrame: async (index) => {
          const frame = await activeSession.capture(exportFrames[index]);
          return withCaption ? captionFrame(frame, captionCanvas, { ...lines, step: `${index + 1} / ${frames.length}` }) : frame;
        },
        onProgress: (completed) => setExporting({ format, completed, total: frames.length }),
      });
      setExportNotice(format === "gif" ? "GIF 已导出，已开始下载" : "视频已导出，已开始下载");
    } catch (reason) {
      setExportNotice(reason instanceof DOMException && reason.name === "AbortError" ? "导出已取消" : reason instanceof Error ? reason.message : "回放导出失败");
    } finally {
      exportAbort.current = null;
      session?.dispose();
      setExporting(null);
    }
  }

  return (
    <>
      <div className="environment-card replay-player">
        <div className="replay-export-stage" ref={stageRef}>
          <div className="section-title environment-title">
            <strong>{label}</strong>
            <small>{attemptText}</small>
          </div>
          {active ? <GameState game={run.game} state={state} previousState={previousState} /> : <EmptyState title={error || "正在载入回放…"} body="" />}
          {replay?.successful && active && cursor === last && (active.event.score_delta ?? 0) > 0 && (
            <div className="replay-cleared" role="status">通关 +{active.event.score_delta}</div>
          )}
        </div>
        <section className="replay-panel" aria-label="回放控制">
          <div className="replay-toolbar">
            <div className="replay-controls" role="group" aria-label="回放操作" inert={exporting !== null}>
              <ReplayIconButton label="上一步" icon={ChevronLeft} onClick={() => move(cursor - 1)} disabled={cursor <= 0} />
              <ReplayIconButton className="play-button" label={playing ? "暂停" : "播放"} icon={playing ? Pause : Play} onClick={toggle} disabled={frames.length < 2} />
              <ReplayIconButton label="下一步" icon={ChevronRight} onClick={() => move(cursor + 1)} disabled={cursor >= last} />
              <LiveSelect className="replay-speed" label="回放速度" value={String(speed)} onChange={value => setSpeed(Number(value))} options={[0.5, 1, 2, 4, 8, 16].map(value => ({ value: String(value), label: `${value}×` }))} />
              <LiveActionMenu items={[
                { label: "跳到开头", icon: Rewind, onSelect: () => move(0), disabled: cursor <= 0 },
                { label: "上一组操作", icon: ChevronsLeft, onSelect: () => move(operationStart(-1)), disabled: operationStart(-1) < 0 },
                { label: "下一组操作", icon: ChevronsRight, onSelect: () => move(operationStart(1)), disabled: operationStart(1) < 0 },
              ]} />
              {undoneCount > 0 && (
                <button type="button" className="replay-undone-toggle" aria-pressed={showUndone} onClick={toggleUndone} disabled={exporting !== null} title="模型撤销掉的操作默认不显示；打开后按真实顺序播放，包括撤销">
                  撤销过程 {undoneCount}
                </button>
              )}
              <button type="button" className="replay-export-button" aria-expanded={exportOpen} onClick={() => setExportOpen(open => !open)} disabled={!frames.length} title="把这次尝试导出为视频或 GIF">
                <Download size={15} aria-hidden="true" />导出
              </button>
              <div className="replay-inline-scrubber" title="快捷键：空格 播放/暂停，← → 单步，Home 回到开头">
                <LiveSlider max={Math.max(0, last)} value={Math.min(cursor, Math.max(0, last))} onChange={move} disabled={!frames.length} />
                <b>{active ? `${cursor + 1} / ${frames.length} · ${actionLabel(active.event.action)}` : "—"}</b>
              </div>
            </div>
            <div className="replay-status" aria-live="polite">
              <i className="replay" />
              {exporting
                ? <>正在导出{exporting.format === "gif" ? " GIF" : "视频"} · {Math.round(exporting.completed / Math.max(1, exporting.total) * 100)}%<button type="button" className="replay-export-cancel" onClick={() => exportAbort.current?.abort()}>取消</button></>
                : exportNotice || (loading ? "正在载入回放…" : error || `${label} · ${attemptText}${showUndone ? " · 含撤销过程" : ""}`)}
            </div>
          </div>
          {exportOpen && frames.length > 0 && (
            <div className="export-panel" role="group" aria-label="导出这次尝试">
              <div className="export-field">
                <span>格式</span>
                <div className="scale-switch" role="group" aria-label="导出格式">
                  {(["video", "gif"] as const).map(format => (
                    <button key={format} aria-pressed={exportChoice.format === format} onClick={() => setExportChoice(choice => ({ ...choice, format }))}>{format === "video" ? "视频（MP4）" : "GIF 动图"}</button>
                  ))}
                </div>
              </div>
              <div className="export-field">
                <span>速度</span>
                <div className="scale-switch" role="group" aria-label="导出速度">
                  {[1, 2, 4, 8].map(value => (
                    <button key={value} aria-pressed={exportChoice.speed === value} onClick={() => setExportChoice(choice => ({ ...choice, speed: value }))}>{value}×</button>
                  ))}
                </div>
              </div>
              <label className="export-field export-caption">
                <input type="checkbox" checked={exportChoice.caption} onChange={event => setExportChoice(choice => ({ ...choice, caption: event.target.checked }))} />
                <span>加标题栏（游戏、模型、关卡、第几次尝试）</span>
              </label>
              <p className="export-summary">{attemptText} · {frames.length} 帧 · 约 {exportSeconds} 秒{showUndone ? " · 含撤销过程" : ""}{exportChoice.format === "gif" ? " · GIF 体积较大，适合短片段" : ""}</p>
              <div className="export-actions">
                <button type="button" className="export-go" disabled={exporting !== null} onClick={() => { void exportSegment(exportChoice); setExportOpen(false); }}>
                  <Download size={15} aria-hidden="true" />导出{exportChoice.format === "video" ? "视频" : " GIF"}
                </button>
                <button type="button" onClick={() => setExportOpen(false)}>收起</button>
              </div>
            </div>
          )}
        </section>
      </div>
      <section className="activity-grid">
        <div className="activity-card">
          <div className="section-title"><strong>这次尝试里 Agent 说了什么</strong></div>
          <div className="activity-list">
            {replay?.activity?.length
              ? replay.activity.map((item, index) => <article key={`${item.timestamp_ms}-${index}`}><time>{clockTime(item.timestamp_ms)}</time><p>{item.text}</p></article>)
              : <EmptyState title="这次尝试没有可见消息" body="" />}
          </div>
        </div>
        <div className="event-card">
          <div className="section-title"><strong>操作</strong><small>{replay ? `${operations.length} 组，点一下跳过去` : ""}</small></div>
          <div className="event-list">
            {operations.map((operation, index) => {
              const start = frameIndex.get(operation.first_frame_key) ?? -1;
              const end = frameIndex.get(operation.last_frame_key);
              const previous = index > 0 ? operations[index - 1] : null;
              const previousEnd = previous ? frameIndex.get(previous.last_frame_key) : undefined;
              const event: ObserverEvent = {
                ...(end === undefined ? { sequence: operation.sequence } : frames[end].event),
                sequence: operation.sequence,
                timestamp_ms: operation.timestamp_ms,
                action: operation.action,
                score_delta: operation.score_delta,
              };
              const description = describeGameEvent(run.game, event, previousEnd === undefined ? null : frames[previousEnd].event);
              const current = operation.sequence === active?.operation_sequence;
              return (
                <button className={`event-row ${current ? "active" : ""} ${operation.undone ? "undone" : ""}`} key={operation.sequence} onClick={() => start >= 0 && move(start)} disabled={start < 0} aria-current={current ? "step" : undefined}>
                  <time>{clockTime(operation.timestamp_ms)}</time>
                  <strong>{description.title}</strong>
                  <span>{operation.undone ? "已撤销 · " : ""}{operation.score_delta ? `${operation.score_delta > 0 ? "+" : ""}${operation.score_delta} 分` : `${operation.frame_count} 帧`}</span>
                </button>
              );
            })}
          </div>
        </div>
      </section>
    </>
  );
}

/** Bumped when replay bodies change shape, so browsers do not reuse an immutable copy. */
const REPLAY_VERSION = 2;
const SHOW_UNDONE_KEY = "replay-show-undone";

function readShowUndone() {
  try { return window.localStorage.getItem(SHOW_UNDONE_KEY) === "1"; } catch { return false; }
}

type ShownOperation = ReplayOperation & { undone: boolean };

/** Consecutive frames of one operation, from the frames being shown. */
function replayOperations(frames: ReplayFrame[]): ShownOperation[] {
  const operations: ShownOperation[] = [];
  for (const frame of frames) {
    if (frame.operation_action?.command === "initial") continue;
    const current = operations.at(-1);
    if (current && current.sequence === frame.operation_sequence) {
      current.last_frame_key = frame.key;
      current.frame_count += 1;
      current.score_delta += frame.event.score_delta ?? 0;
      current.undone &&= Boolean(frame.undone);
      continue;
    }
    operations.push({
      sequence: frame.operation_sequence,
      timestamp_ms: frame.operation_timestamp_ms,
      action: frame.operation_action,
      first_frame_key: frame.key,
      last_frame_key: frame.key,
      frame_count: 1,
      score_delta: frame.event.score_delta ?? 0,
      undone: Boolean(frame.undone),
    });
  }
  return operations;
}

export function GameTabs({ view, onDashboard, onReplays }: { view: "dashboard" | "replays"; onDashboard: () => void; onReplays: () => void }) {
  return (
    <div className="game-tabs" role="tablist" aria-label="成绩或回放">
      <button role="tab" aria-selected={view === "dashboard"} onClick={onDashboard}>成绩</button>
      <button role="tab" aria-selected={view === "replays"} onClick={onReplays}>关卡回放</button>
    </div>
  );
}

