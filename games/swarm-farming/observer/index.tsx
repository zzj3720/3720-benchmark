import {
  Metric,
  asNumber,
  type GameObserverModule,
  type GameState,
} from "../../../observer-platform/app/game-observer";

function SwarmState({ state }: { state: GameState }) {
  const robots = Array.isArray(state.robots) ? state.robots : [];
  return (
    <div className="swarm-state">
      <Metric label="TICK" value={asNumber(state.tick).toLocaleString()} />
      <Metric label="DEADLINE" value={asNumber(state.deadline_ticks).toLocaleString()} />
      <Metric label="ROBOTS" value={String(robots.length)} />
      <Metric label="PROGRAM" value={state.program_running ? "RUNNING" : "IDLE"} />
    </div>
  );
}

export default {
  id: "swarm",
  meta: { label: "Swarm Farming", short: "SWARM", accent: "#5ed7ff" },
  State: SwarmState,
} satisfies GameObserverModule;
