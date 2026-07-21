import {
  Metric,
  asNumber,
  asRecord,
  asString,
  type GameObserverModule,
  type GameState,
} from "../../../observer-platform/app/game-observer";

function SausageState({ state }: { state: GameState }) {
  const entities = Array.isArray(state.entities) ? state.entities : [];
  const player = asRecord(state.player);
  const pos = asRecord(player?.pos);
  return (
    <div className="sausage-state">
      <Metric
        label="PLAYER"
        value={`${asNumber(pos?.x)}, ${asNumber(pos?.y)}, ${asNumber(pos?.z)}`}
      />
      <Metric
        label="SAUSAGES"
        value={String(
          entities.filter((entity) => asRecord(entity)?.kind === "sausage").length,
        )}
      />
      <Metric label="EXIT" value={state.exit_ready ? "READY" : "LOCKED"} />
      <Metric label="STATUS" value={asString(state.status)} />
    </div>
  );
}

export default {
  id: "sausage",
  meta: {
    label: "Stephen’s Sausage Roll",
    short: "SAUSAGE",
    accent: "#ffad57",
  },
  State: SausageState,
} satisfies GameObserverModule;
