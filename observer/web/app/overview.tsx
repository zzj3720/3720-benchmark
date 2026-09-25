"use client";
import { GAME_IDS, GAME_META, type GameId } from "./game-registry";
import type { RunSummary } from "./live-contract";
import { clockTime } from "./live-shared";
import { harnessName, modelFamily, modelName } from "./run-labels";

/** How far a run got, as a share of the game's total. */
function completion(run: RunSummary) {
  return run.total > 0 ? Math.max(0, Math.min(1, run.score / run.total)) : 0;
}

function lastSeen(run: RunSummary) {
  return run.live ? Date.now() : run.last_activity_at ?? run.finished_at ?? run.started_at ?? 0;
}

function ago(timestamp: number, now: number) {
  const minutes = Math.max(0, Math.round((now - timestamp) / 60_000));
  if (minutes < 1) return "刚刚";
  if (minutes < 60) return `${minutes} 分钟前`;
  const hours = Math.round(minutes / 60);
  if (hours < 48) return `${hours} 小时前`;
  return `${Math.round(hours / 24)} 天前`;
}

type ModelRow = { model: string; family: string; best: Map<GameId, RunSummary>; runs: number; live: boolean };

/**
 * One row per model, one column per game; a cell is the model's best run in
 * that game. Games score differently and most models played only some of
 * them, so there is no combined score: rows are ordered by how many games a
 * model played, then by its mean completion in those games.
 */
function modelRows(runs: RunSummary[]) {
  const rows = new Map<string, ModelRow>();
  for (const run of runs) {
    const model = modelName(run.model);
    const row = rows.get(model) ?? { model, family: modelFamily(run.model), best: new Map(), runs: 0, live: false };
    row.runs += 1;
    row.live ||= run.live;
    const current = row.best.get(run.game);
    if (!current || run.score > current.score || (run.score === current.score && run.live && !current.live)) row.best.set(run.game, run);
    rows.set(model, row);
  }
  const mean = (row: ModelRow) => [...row.best.values()].reduce((sum, run) => sum + completion(run), 0) / Math.max(1, row.best.size);
  return [...rows.values()].sort((a, b) => b.best.size - a.best.size || mean(b) - mean(a) || a.model.localeCompare(b.model));
}

/** Rows grouped by family, families in the order of their first row. */
function familyGroups(rows: ModelRow[]) {
  const groups = new Map<string, ModelRow[]>();
  for (const row of rows) groups.set(row.family, [...(groups.get(row.family) ?? []), row]);
  return [...groups.entries()].sort(([a], [b]) => Number(a === "其他") - Number(b === "其他"));
}

export function Overview({
  runs,
  now,
  onRun,
  onGame,
}: {
  runs: RunSummary[];
  now: number;
  onRun: (run: RunSummary) => void;
  onGame: (game: GameId) => void;
}) {
  const games = GAME_IDS.filter(game => runs.some(run => run.game === game));
  const rows = modelRows(runs);
  const live = runs.filter(run => run.live);
  const latest = runs.slice().sort((a, b) => lastSeen(b) - lastSeen(a))[0];
  return (
    <div className="overview-page">
      <header className="page-heading">
        <div>
          <h1>3720 Benchmark</h1>
          <p className="page-subtitle">
            {runs.length
              ? `${new Set(rows.map(row => row.family)).size} 个家族的 ${rows.length} 个模型在 ${games.length} 个游戏上跑了 ${runs.length} 次。每格是这个模型在该游戏里最好的一次运行，点开看现场和回放。`
              : "新的运行开始后会出现在这里。"}
          </p>
        </div>
      </header>

      <section className={`overview-live ${live.length ? "on" : ""}`} aria-label="正在直播">
        {live.length ? (
          <>
            <strong className="overview-live-title"><i className="live-pulse" aria-hidden="true" />正在直播</strong>
            <div className="overview-live-list">
              {live.map(run => (
                <button key={run.id} type="button" className="overview-live-run" style={{ "--accent": GAME_META[run.game].accent } as React.CSSProperties} onClick={() => onRun(run)}>
                  <span>{GAME_META[run.game].short}</span>
                  <strong>{modelFamily(run.model)} · {modelName(run.model)}</strong>
                  <b>{run.score}<small> / {run.total || "—"}</small></b>
                </button>
              ))}
            </div>
          </>
        ) : latest ? (
          <p>现在没有正在进行的运行。最近一次活动在 {ago(lastSeen(latest), now)}（{clockTime(lastSeen(latest))}），是 {modelName(latest.model)} 在 {GAME_META[latest.game].short} 上。</p>
        ) : null}
      </section>

      {rows.length > 0 && (
        <section className="overview-matrix-card">
          <div className="section-title">
            <strong>模型 × 游戏</strong>
            <small>完成度 = 得分 / 该游戏满分</small>
          </div>
          <div className="overview-matrix-scroll">
            <table className="overview-matrix">
              <thead>
                <tr>
                  <th scope="col">模型</th>
                  {games.map(game => (
                    <th key={game} scope="col" style={{ "--accent": GAME_META[game].accent } as React.CSSProperties}>
                      <button type="button" onClick={() => onGame(game)} title={`${GAME_META[game].label} 的成绩页`}>{GAME_META[game].short}</button>
                    </th>
                  ))}
                </tr>
              </thead>
              {familyGroups(rows).map(([family, familyRows]) => (
              <tbody key={family}>
                <tr className="overview-family"><th scope="rowgroup" colSpan={games.length + 1}>{family}<small>{familyRows.length} 个模型</small></th></tr>
                {familyRows.map(row => (
                  <tr key={row.model}>
                    <th scope="row">
                      <strong>{row.model}</strong>
                      <small>{row.best.size} 个游戏 · {row.runs} 次运行</small>
                    </th>
                    {games.map(game => {
                      const run = row.best.get(game);
                      if (!run) return <td key={game} className="empty"><span aria-label="没有运行">—</span></td>;
                      const share = completion(run);
                      const detail = [run.effort !== "default" ? run.effort : "", harnessName(run.agent)].filter(Boolean).join(" · ");
                      return (
                        <td key={game} style={{ "--accent": GAME_META[game].accent } as React.CSSProperties}>
                          <button type="button" onClick={() => onRun(run)} title={`${row.model}${detail ? ` · ${detail}` : ""} · ${run.score} / ${run.total}`}>
                            <span className="overview-score">
                              {run.live && <i className="live-pulse" aria-label="直播中" />}
                              <b>{run.score}</b><small> / {run.total || "—"}</small>
                            </span>
                            <span className="overview-bar" aria-hidden="true"><i style={{ width: `${Math.max(2, share * 100)}%` }} /></span>
                            <small>{Math.round(share * 100)}%{detail ? ` · ${detail}` : ""}</small>
                          </button>
                        </td>
                      );
                    })}
                  </tr>
                ))}
              </tbody>
              ))}
            </table>
          </div>
        </section>
      )}

      <section className="overview-games" aria-label="各游戏">
        {games.map(game => {
          const gameRuns = runs.filter(run => run.game === game);
          const leader = gameRuns.slice().sort((a, b) => b.score - a.score)[0];
          const recent = gameRuns.slice().sort((a, b) => lastSeen(b) - lastSeen(a))[0];
          const liveCount = gameRuns.filter(run => run.live).length;
          return (
            <button key={game} type="button" className="overview-game" style={{ "--accent": GAME_META[game].accent } as React.CSSProperties} onClick={() => onGame(game)}>
              <span className="overview-game-name">{GAME_META[game].label}</span>
              <span className="overview-game-lead">
                <b>{leader.score}<small> / {leader.total || "—"}</small></b>
                <span>{modelName(leader.model)} 领先</span>
              </span>
              <span className="overview-game-meta">
                {gameRuns.length} 次运行{liveCount ? ` · ${liveCount} 个直播中` : ` · ${ago(lastSeen(recent), now)}活动`}
              </span>
            </button>
          );
        })}
      </section>
    </div>
  );
}
