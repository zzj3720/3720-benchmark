import { asNumber, asRecord, asString, type GameObserverModule, type GameState, type ObserverEvent } from "../../../observer/web/app/game-observer";
import { GameCanvas } from "../../../observer/web/app/webgl/game-canvas";
import { tileBoardScene } from "../../../observer/web/app/webgl/boards";
import type { SceneBuilder } from "../../../observer/web/app/webgl/scene";

const buildScene: SceneBuilder = (state, view) => tileBoardScene("minesweeper", state, view);
function MinesweeperState({ state }: { state: GameState }) { return <GameCanvas state={state} build={buildScene} label="Minesweeper" />; }

export default {
  id: "minesweeper",
  meta: {
    label: "No-Guess Minesweeper",
    short: "MINES",
    accent: "#56d6c4",
  },
  State: MinesweeperState,
  stateContext(state) {
    const campaign = asRecord(state.campaign);
    const level = asRecord(state.level);
    return `${asString(level?.id, "no level")} · ${asNumber(campaign?.score)}/${asNumber(campaign?.max_score)}`;
  },
  describeEvent(event: ObserverEvent) {
    const command = asString(event.action?.command, event.type ?? "state").toUpperCase();
    return {
      label: event.score_delta ? "DIFFICULTY PASSED" : "SWEEP ACTION",
      title: command,
      detail: event.score_delta
        ? `Passed a no-guess difficulty tier. Score is now ${event.score ?? 0}.`
        : `Authoritative partial-observation state recorded at event #${event.sequence}.`,
      tone: event.score_delta ? "success" : "neutral",
    };
  },
} satisfies GameObserverModule;
