import { asNumber, asRecord, asString, type GameObserverModule, type GameState, type ObserverEvent } from "../../../observer-platform/app/game-observer";
import { GameCanvas } from "../../../observer-platform/app/webgl/game-canvas";
import { tileBoardScene } from "../../../observer-platform/app/webgl/boards";
import type { SceneBuilder } from "../../../observer-platform/app/webgl/scene";

const buildScene: SceneBuilder = (state, view) => tileBoardScene("sokoban", state, view);
function SokobanState({ state }: { state: GameState }) { return <GameCanvas state={state} build={buildScene} label="Sokoban" />; }

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
