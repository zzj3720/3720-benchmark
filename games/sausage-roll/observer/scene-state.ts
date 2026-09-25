import { asNumber, asRecord, asString, type GameState, type Json } from "../../../observer/web/app/game-observer";

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
  sourceId: number;
  tileSet: number;
  variant: number;
};
export type SceneEntrance = {
  ordinal: number;
  title: string;
  pos: Coord3;
  direction: string;
  islandId: number;
  status: "complete" | "available";
};
export type SausageSceneState = {
  mode: string;
  levelKey: string;
  tileSet: number;
  entities: SceneEntity[];
  tiles: SceneTile[];
  entrances: SceneEntrance[];
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

function readTiles(value: Json | undefined): SceneTile[] {
  return (Array.isArray(value) ? value : []).flatMap((item) => {
    const tile = asRecord(item);
    return tile
      ? [{
          pos: coord(tile.pos),
          kind: asString(tile.kind),
          direction: asString(tile.direction, "none"),
          sourceId: asNumber(tile.source_id, -1),
          tileSet: Math.max(0, Math.min(4, asNumber(tile.tile_set))),
          variant: asNumber(tile.variant),
        }]
      : [];
  });
}

function readIslandPoses(value: Json | undefined) {
  return new Map(
    (Array.isArray(value) ? value : []).flatMap((item) => {
      const island = asRecord(item);
      return island ? [[asNumber(island.id), coord(island.pos)] as const] : [];
    }),
  );
}

function translated(pos: Coord3, from: Coord3 | undefined, to: Coord3 | undefined) {
  if (!from || !to) return pos;
  return {
    x: pos.x + to.x - from.x,
    y: pos.y + to.y - from.y,
    z: pos.z + to.z - from.z,
  };
}

export function readSceneState(state: GameState): SausageSceneState {
  const mode = asString(state.mode, "puzzle");
  const level = asRecord(state.level);
  const overworld = asRecord(state.overworld);
  const map = asRecord(state.overworld_map);
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

  let tiles = readTiles(state.tiles);
  let entrances: SceneEntrance[] = [];
  if (mode === "overworld" && map) {
    const initialIslands = readIslandPoses(map.islands);
    const currentIslands = readIslandPoses(overworld?.islands);
    const completed = new Set(
      (Array.isArray(overworld?.completed_islands) ? overworld.completed_islands : [])
        .map((id) => asNumber(id)),
    );
    tiles = readTiles(map.tiles)
      .filter((tile) => !(completed.has(tile.sourceId) && tile.variant === -1))
      .map((tile) => ({
        ...tile,
        pos: translated(tile.pos, initialIslands.get(tile.sourceId), currentIslands.get(tile.sourceId)),
      }));
    entrances = (Array.isArray(map.entrances) ? map.entrances : []).flatMap((value) => {
      const entrance = asRecord(value);
      if (!entrance) return [];
      const islandId = asNumber(entrance.island_id, -1);
      return [{
        ordinal: asNumber(entrance.ordinal),
        title: asString(entrance.title),
        pos: translated(coord(entrance.pos), initialIslands.get(islandId), currentIslands.get(islandId)),
        direction: asString(entrance.direction, "none"),
        islandId,
        status: completed.has(islandId) ? "complete" : "available",
      }];
    });
  }

  const exit = asRecord(state.exit);
  return {
    mode,
    levelKey: `${mode}:${asNumber(asRecord(state.campaign)?.solved)}:${asNumber(level?.ordinal)}:${asString(level?.id)}`,
    tileSet: Math.max(0, Math.min(4, asNumber(level?.tile_set))),
    entities,
    tiles,
    entrances,
    exit: exit
      ? { pos: coord(exit.pos), direction: asString(exit.direction, "none"), ready: Boolean(exit.ready) }
      : null,
  };
}
