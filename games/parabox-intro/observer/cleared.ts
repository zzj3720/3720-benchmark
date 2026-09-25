import { asRecord, asString, type GameState, type Json, type ObserverEvent } from "../../../observer/web/app/game-observer";

const STEPS: Record<string, [number, number]> = { up: [-1, 0], down: [1, 0], left: [0, -1], right: [0, 1] };

type Block = { kind?: Json; row?: Json; column?: Json; definition_id?: Json; [key: string]: Json | undefined };

/**
 * Parabox moves straight on to the next level when one is solved, so the
 * recorded state of the solving move already shows the next level. Replay the
 * move on the last board of the solved level instead: push the player and the
 * boxes ahead of it one cell, and keep the result only if every goal of the
 * focused space is then covered. Otherwise show that last board as it was.
 */
export function clearedParaboxState(before: GameState, event: ObserverEvent): GameState | null {
  const reference = (state?: GameState | null) => asString(asRecord(state?.level)?.reference, "");
  if (reference(event.state) === reference(before)) return null;
  return pushed(before, asString(event.action?.direction, "")) ?? before;
}

function pushed(state: GameState, direction: string): GameState | null {
  const step = STEPS[direction];
  const scene = asRecord(state.observer_scene);
  // A mirrored camera swaps what left and right mean on screen; do not guess.
  if (!step || !scene || scene.camera_flip_h === true || !Array.isArray(scene.spaces)) return null;
  const focus = scene.focus_space;
  const space = scene.spaces.map(asRecord).find(item => item?.id === focus);
  if (!space || !Array.isArray(space.map) || !Array.isArray(space.blocks)) return null;
  const strings = space.map.every(row => typeof row === "string");
  const map = space.map.map(row => typeof row === "string" ? [...row] : Array.isArray(row) ? row.map(cell => asString(cell, " ")) : []);
  const blocks = space.blocks.map(block => ({ ...(asRecord(block) ?? {}) }) as Block);
  const pieces = blocks.filter(block => block.kind === "player" || block.kind === "box");
  const at = (row: number, column: number) => pieces.find(block => block.row === row && block.column === column);
  const player = pieces.find(block => block.kind === "player");
  if (!player || typeof player.row !== "number" || typeof player.column !== "number") return null;

  const chain = [player];
  let row = player.row + step[0], column = player.column + step[1];
  for (;;) {
    if (row < 0 || column < 0 || row >= map.length || column >= (map[row]?.length ?? 0) || map[row][column] === "#") return null;
    const next = at(row, column);
    if (!next) break;
    chain.push(next);
    row += step[0]; column += step[1];
  }
  for (const block of chain) { block.row = (block.row as number) + step[0]; block.column = (block.column as number) + step[1]; }

  for (let r = 0; r < map.length; r++) for (let c = 0; c < map[r].length; c++) {
    const symbol = map[r][c], piece = at(r, c);
    if ("+P".includes(symbol) && piece?.kind !== "player") return null;
    if (".X".includes(symbol) && piece?.kind !== "box") return null;
  }
  const cleared = map.map((cells, r) => cells.map((symbol, c) => {
    if (symbol === "#") return symbol;
    const goal = "+P".includes(symbol) ? "player" : ".X".includes(symbol) ? "box" : null, piece = at(r, c);
    if (piece?.kind === "player") return goal === "player" ? "P" : "@";
    if (piece?.kind === "box") return goal === "box" ? "X" : String((typeof piece.definition_id === "number" ? piece.definition_id : 0) % 10);
    return goal === "player" ? "+" : goal === "box" ? "." : " ";
  }));
  return {
    ...state,
    observer_scene: {
      ...scene,
      spaces: scene.spaces.map(item => asRecord(item)?.id === focus
        ? { ...(asRecord(item) ?? {}), blocks: blocks as Json[], map: strings ? cleared.map(cells => cells.join("")) : cleared }
        : item),
    },
  };
}
