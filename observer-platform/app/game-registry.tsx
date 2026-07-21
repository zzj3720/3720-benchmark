import operator from "../../games/emergency-operator/observer";
import kitchen from "../../games/kitchen-terminal/observer";
import parabox from "../../games/parabox-intro/observer";
import sausage from "../../games/sausage-roll/observer";
import swarm from "../../games/swarm-farming/observer";

import type { GameState as State } from "./game-observer";

export const GAME_OBSERVERS = {
  parabox,
  swarm,
  sausage,
  operator,
  kitchen,
} as const;

export type GameId = keyof typeof GAME_OBSERVERS;
export const GAME_IDS = Object.keys(GAME_OBSERVERS) as GameId[];
export const GAME_META = Object.fromEntries(
  GAME_IDS.map((id) => [id, GAME_OBSERVERS[id].meta]),
) as { [Game in GameId]: (typeof GAME_OBSERVERS)[Game]["meta"] };

export function GameState({ game, state }: { game: GameId; state: State }) {
  const StateView = GAME_OBSERVERS[game].State;
  return <StateView state={state} />;
}
