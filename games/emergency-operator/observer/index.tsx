import type { GameObserverModule, GameState } from "../../../observer-platform/app/game-observer";
import { GameCanvas } from "../../../observer-platform/app/webgl/game-canvas";
import { buildOperatorScene } from "./webgl";
export function OperatorState({ state }: { state: GameState }) { return <GameCanvas state={state} build={buildOperatorScene} label="Operator" />; }

export default {
  id: "operator",
  meta: {
    label: "Emergency Operator",
    short: "OPERATOR",
    accent: "#ff6f7d",
  },
  State: OperatorState,
} satisfies GameObserverModule;
