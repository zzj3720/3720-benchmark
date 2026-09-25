import { asNumber, asRecord, type GameObserverModule, type GameState } from "../../../observer/web/app/game-observer";
import { GameCanvas } from "../../../observer/web/app/webgl/game-canvas";
import { clock } from "../../../observer/web/app/webgl/data";
import { buildKitchenScene } from "./webgl";
function KitchenState({ state }: { state: GameState }) { return <GameCanvas state={state} build={buildKitchenScene} label="Kitchen" />; }

export default {
  id: "kitchen",
  meta: { label: "Overcooked Kitchen", short: "KITCHEN", accent: "#ffcf54" },
  State: KitchenState,
  stateContext: (state) => {
    const campaign = asRecord(state.campaign);
    const shift = asRecord(state.shift);
    return `Kitchen level ${asNumber(campaign?.level)}, score ${asNumber(campaign?.score)}, ${clock(asNumber(shift?.remaining_ms))} remaining.`;
  },
} satisfies GameObserverModule;
