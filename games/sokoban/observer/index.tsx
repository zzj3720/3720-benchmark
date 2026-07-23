import {
  Metric,
  asNumber,
  asRecord,
  asString,
  type GameObserverModule,
  type GameState,
  type Json,
  type ObserverEvent,
} from "../../../observer-platform/app/game-observer";

function SokobanState({ state }: { state: GameState }) {
  const campaign = asRecord(state.campaign);
  const level = asRecord(state.level);
  const board = asRecord(state.board);
  const tiers = records(state.tiers);
  const map = strings(board?.map);
  const width = asNumber(board?.width, 1);
  return (
    <div className="sokoban-state">
      <div className="sokoban-summary">
        <Metric
          label="SCORE"
          value={`${asNumber(campaign?.score)} / ${asNumber(campaign?.max_score)}`}
        />
        <Metric label="TIER" value={asString(level?.tier_title, "NOT SELECTED")} />
        <Metric label="LEVEL" value={asString(level?.id, "—").toUpperCase()} />
        <Metric label="MOVES / PUSHES" value={`${asNumber(board?.moves)} / ${asNumber(board?.pushes)}`} />
      </div>
      <div className="sokoban-main">
        <div className="sokoban-board-wrap">
          {map.length ? (
            <div
              className="sokoban-board"
              style={{ gridTemplateColumns: `repeat(${width}, minmax(12px, 32px))` }}
              aria-label={`Sokoban board for ${asString(level?.title)}`}
            >
              {map.flatMap((row, rowIndex) =>
                [...row].map((tile, columnIndex) => (
                  <span
                    className={`sokoban-tile tile-${tileName(tile)}`}
                    key={`${rowIndex}-${columnIndex}`}
                    title={`${rowIndex},${columnIndex}`}
                  />
                )),
              )}
            </div>
          ) : (
            <div className="sokoban-empty">SELECT A LEVEL TO BEGIN</div>
          )}
        </div>
        <aside className="sokoban-tiers">
          <h3>DIFFICULTY PROGRESSION</h3>
          {tiers.map((tier) => (
            <div className={`sokoban-tier ${asString(tier.status)}`} key={asString(tier.id)}>
              <span>{asString(tier.title)}</span>
              <strong>
                {asNumber(tier.solved)} / {asNumber(tier.total)}
              </strong>
              <small>
                {asString(tier.status).toUpperCase()}
                {asString(tier.status) === "unlocked" && asNumber(tier.remaining_to_unlock_next) > 0
                  ? ` · ${asNumber(tier.remaining_to_unlock_next)} TO NEXT`
                  : ""}
              </small>
            </div>
          ))}
        </aside>
      </div>
    </div>
  );
}

function records(value: Json | undefined) {
  return Array.isArray(value)
    ? value.map((item) => asRecord(item)).filter((item) => item !== null)
    : [];
}

function strings(value: Json | undefined) {
  return Array.isArray(value) ? value.filter((item): item is string => typeof item === "string") : [];
}

function tileName(tile: string) {
  return (
    {
      "#": "wall",
      ".": "goal",
      "$": "box",
      "*": "box-goal",
      "@": "player",
      "+": "player-goal",
      " ": "floor",
    }[tile] ?? "floor"
  );
}

export default {
  id: "sokoban",
  meta: {
    label: "Sokoban Classics",
    short: "SOKOBAN",
    accent: "#f0b44d",
  },
  State: SokobanState,
  stateContext(state) {
    const campaign = asRecord(state.campaign);
    const level = asRecord(state.level);
    return `${asString(level?.id, "no level")} · ${asNumber(campaign?.score)}/${asNumber(campaign?.max_score)}`;
  },
  describeEvent(event: ObserverEvent) {
    const command = asString(event.action?.command, event.type ?? "state").toUpperCase();
    return {
      label: event.score_delta ? "LEVEL CLEARED" : "WAREHOUSE ACTION",
      title: command,
      detail: event.score_delta
        ? `Solved a new level. Score is now ${event.score ?? 0}.`
        : `Authoritative board state recorded at event #${event.sequence}.`,
      tone: event.score_delta ? "success" : "neutral",
    };
  },
} satisfies GameObserverModule;
