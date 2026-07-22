import { asNumber, asRecord, asString, type GameState, type Json } from "../../../observer-platform/app/game-observer";

export type Coord3 = { x: number; y: number; z: number };
export type SceneEntity = {
  id: number;
  kind: string;
  pos: Coord3;
  direction: string;
  rotation: number;
  cells: Coord3[];
  cookedFaces: number[] | null;
};
export type SceneTile = {
  pos: Coord3;
  kind: string;
  direction: string;
};
export type SausageSceneState = {
  levelKey: string;
  tileSet: number;
  entities: SceneEntity[];
  tiles: SceneTile[];
  exit: { pos: Coord3; direction: string; ready: boolean } | null;
};

function coord(value: Json | undefined): Coord3 {
  const record = asRecord(value);
  return {
    x: asNumber(record?.x),
    y: asNumber(record?.y),
    z: asNumber(record?.z),
  };
}

export function readSceneState(state: GameState): SausageSceneState {
  const level = asRecord(state.level);
  const entities = (Array.isArray(state.entities) ? state.entities : []).flatMap((value) => {
    const entity = asRecord(value);
    if (!entity) return [];
    return [{
      id: asNumber(entity.id),
      kind: asString(entity.kind),
      pos: coord(entity.pos),
      direction: asString(entity.direction, "none"),
      rotation: asNumber(entity.rotation),
      cells: (Array.isArray(entity.cells) ? entity.cells : []).map(coord),
      cookedFaces: Array.isArray(entity.cooked_faces)
        ? entity.cooked_faces.map((face) => asNumber(face)).slice(0, 4)
        : null,
    }];
  });
  const tiles = (Array.isArray(state.tiles) ? state.tiles : []).flatMap((value) => {
    const tile = asRecord(value);
    return tile
      ? [{ pos: coord(tile.pos), kind: asString(tile.kind), direction: asString(tile.direction, "none") }]
      : [];
  });
  const exit = asRecord(state.exit);
  return {
    levelKey: `${asNumber(level?.ordinal)}:${asString(level?.id)}`,
    tileSet: Math.max(0, Math.min(4, asNumber(level?.tile_set))),
    entities,
    tiles,
    exit: exit
      ? { pos: coord(exit.pos), direction: asString(exit.direction, "none"), ready: Boolean(exit.ready) }
      : null,
  };
}
