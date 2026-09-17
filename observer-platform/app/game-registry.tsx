import { memo } from "react";

import operator from "../../games/emergency-operator/observer";
import kitchen from "../../games/kitchen-terminal/observer";
import minesweeper from "../../games/minesweeper/observer";
import parabox from "../../games/parabox-intro/observer";
import sausage from "../../games/sausage-roll/observer";
import sokoban from "../../games/sokoban/observer";
import swarm from "../../games/swarm-farming/observer";

import {
  asString,
  type EventDescription,
  type GameObserverModule,
  type GameState as State,
  type ObserverEvent,
} from "./game-observer";

export const GAME_OBSERVERS = {
  parabox,
  swarm,
  sausage,
  sokoban,
  minesweeper,
  operator,
  kitchen,
} as const;

export type GameId = keyof typeof GAME_OBSERVERS;
export const GAME_IDS = Object.keys(GAME_OBSERVERS) as GameId[];
export const GAME_META = Object.fromEntries(
  GAME_IDS.map((id) => [id, GAME_OBSERVERS[id].meta]),
) as { [Game in GameId]: (typeof GAME_OBSERVERS)[Game]["meta"] };

export const GameState = memo(function GameState({
  game,
  state,
  previousState,
}: {
  game: GameId;
  state: State;
  previousState?: State | null;
}) {
  const StateView = GAME_OBSERVERS[game].State as GameObserverModule["State"];
  return <StateView state={state} previousState={previousState} />;
});

export function gameStateContext(game: GameId, state: State) {
  const observer = GAME_OBSERVERS[game] as GameObserverModule;
  return observer.stateContext?.(state) ?? null;
}

export function resolveGameFrameState(game: GameId, frameState: State, latestState: State) {
  const observer = GAME_OBSERVERS[game] as GameObserverModule;
  return observer.resolveFrameState?.(frameState, latestState) ?? frameState;
}

export function describeGameEvent(
  game: GameId,
  event: ObserverEvent,
  previous?: ObserverEvent | null,
): EventDescription {
  const observer = GAME_OBSERVERS[game] as GameObserverModule;
  return (
    observer.describeEvent?.(event, previous) ?? {
      label: "ENVIRONMENT EVENT",
      title: asString(event.action?.command, event.type ?? "state").toUpperCase(),
      detail: event.score_delta
        ? `本步得分增加 ${event.score_delta}。`
        : `权威状态已记录为事件 #${event.sequence}。`,
      tone: event.score_delta ? "success" : "neutral",
    }
  );
}
