import { asNumber, asRecord, asString, type GameObserverModule, type GameState, type ObserverEvent } from "../../../observer/web/app/game-observer";
import { GameCanvas } from "../../../observer/web/app/webgl/game-canvas";
import { buildSwarmScene } from "./webgl";
function SwarmState({ state }: { state: GameState }) { return <GameCanvas state={state} build={buildSwarmScene} label="Swarm" />; }

export default {
  id: "swarm",
  meta: { label: "Swarm Farming", short: "SWARM", accent: "#5ed7ff" },
  State: SwarmState,
  stateContext(state) {
    const robots = Array.isArray(state.robots) ? state.robots.length : 0;
    return `TICK ${asNumber(state.tick).toLocaleString()} / ${asNumber(state.deadline_ticks).toLocaleString()} · ${robots} ROBOTS`;
  },
  describeEvent(event: ObserverEvent) {
    const command = asString(event.action?.command, event.type ?? "state").toLowerCase();
    const argument = asRecord(event.action?.argument);
    const title =
      command === "run"
        ? "运行群控程序"
        : command === "advance"
          ? `推进 ${asNumber(argument?.ticks).toLocaleString()} tick`
          : command === "submit"
            ? "提交结果"
            : command === "status"
              ? "查询状态"
              : command.toUpperCase();
    return {
      label: event.score_delta ? "OBJECTIVE COMPLETE" : "SWARM COMMAND",
      title,
      detail: event.score_delta
        ? `目标达成，当前得分 ${event.score ?? 0}。`
        : `权威状态已记录为事件 #${event.sequence}。`,
      tone: event.score_delta ? "success" : "neutral",
    };
  },
} satisfies GameObserverModule;
