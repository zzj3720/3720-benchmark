import {
  EmptyState,
  asNumber,
  asRecord,
  asString,
  type EventDescription,
  type GameObserverModule,
  type GameState,
  type Json,
  type ObserverEvent,
} from "../../../observer-platform/app/game-observer";

const BOX_COLORS = [
  "#40d9b5",
  "#f2aa3b",
  "#4dbbff",
  "#e94b82",
  "#9fd43c",
  "#ff795e",
  "#8c7cf4",
  "#36cfe8",
  "#efcc4f",
  "#64d477",
];

const MIN_RECURSIVE_SCALE = 1 / 512;

function observerColor(value: Json | undefined, fallback: string) {
  if (!Array.isArray(value) || value.length !== 3) return fallback;
  const channels = value.map((channel) => Math.max(0, Math.min(255, asNumber(channel, 0))));
  return `rgb(${channels.join(" ")})`;
}

function spaceMap(state?: GameState | null) {
  const space = asRecord(state?.space);
  return Array.isArray(space?.map) ? (space.map as Json[][]) : [];
}

function spacePath(state?: GameState | null) {
  const space = asRecord(state?.space);
  return Array.isArray(space?.path)
    ? space.path.map((part) => asString(part, "?")).filter(Boolean)
    : [];
}

function levelReference(state?: GameState | null) {
  return asString(asRecord(state?.level)?.reference, "");
}

function sameSpace(state: GameState, previousState?: GameState | null) {
  return (
    previousState &&
    levelReference(state) === levelReference(previousState) &&
    spacePath(state).join("/") === spacePath(previousState).join("/")
  );
}

function positionOf(map: Json[][], symbols: string[]) {
  for (let row = 0; row < map.length; row += 1) {
    for (let column = 0; column < (map[row]?.length ?? 0); column += 1) {
      if (symbols.includes(asString(map[row][column], " "))) return { row, column };
    }
  }
  return null;
}

function completedGoals(map: Json[][]) {
  return map.flat().filter((cell) => ["X", "P"].includes(asString(cell, " "))).length;
}

function boardChanges(state?: GameState | null, previousState?: GameState | null) {
  const map = spaceMap(state);
  const previousMap = spaceMap(previousState);
  if (!state || !previousState || !sameSpace(state, previousState)) return [];
  return map.flatMap((row, rowIndex) =>
    row.flatMap((cell, columnIndex) =>
      asString(cell, " ") === asString(previousMap[rowIndex]?.[columnIndex], " ")
        ? []
        : [`${rowIndex}:${columnIndex}`],
    ),
  );
}

function cellKind(symbol: string) {
  if (/^\d$/.test(symbol)) return "box";
  return (
    {
      "#": "wall",
      "!": "portal-wall",
      "@": "player",
      P: "player player-goal occupied",
      ".": "box-goal",
      "+": "player-goal",
      X: "box box-goal occupied",
      " ": "empty",
    }[symbol] ?? "unknown"
  );
}

function isWall(symbol: Json | undefined) {
  return ["#", "!"].includes(asString(symbol, " "));
}

function wallEdges(map: Json[][], row: number, column: number) {
  if (!isWall(map[row]?.[column])) return null;
  return {
    top: !isWall(map[row - 1]?.[column]),
    right: !isWall(map[row]?.[column + 1]),
    bottom: !isWall(map[row + 1]?.[column]),
    left: !isWall(map[row]?.[column - 1]),
  };
}

function cellLabel(symbol: string, row: number, column: number) {
  const label = /^\d$/.test(symbol)
    ? `盒子 ${symbol}`
    : {
        "#": "墙",
        "!": "可进入墙体",
        "@": "玩家",
        P: "位于玩家目标上的玩家",
        ".": "盒子目标",
        "+": "玩家目标",
        X: "已占据的盒子目标",
        " ": "空地",
      }[symbol] ?? `未知符号 ${symbol}`;
  return `第 ${row + 1} 行第 ${column + 1} 列：${label}`;
}

type SceneGraph = {
  rootSpace: number;
  focusSpace: number;
  cameraFlipH: boolean;
  spaces: Map<number, GameState>;
};

function observerScene(state?: GameState | null): SceneGraph | null {
  const raw = asRecord(state?.observer_scene);
  if (
    asString(raw?.schema, "") !== "parabox-observer-scene-v1" ||
    !Array.isArray(raw?.spaces)
  ) {
    return null;
  }
  const spaces = new Map<number, GameState>();
  for (const candidate of raw.spaces) {
    const space = asRecord(candidate);
    const id = asNumber(space?.id, -1);
    if (space && id >= 0) spaces.set(id, space);
  }
  const rootSpace = asNumber(raw.root_space, -1);
  const focusSpace = asNumber(raw.focus_space, -1);
  if (!spaces.has(rootSpace) || !spaces.has(focusSpace)) return null;
  return {
    rootSpace,
    focusSpace,
    cameraFlipH: raw.camera_flip_h === true,
    spaces,
  };
}

function sceneMap(scene: SceneGraph, spaceId: number) {
  const map = scene.spaces.get(spaceId)?.map;
  return Array.isArray(map)
    ? map.map((row) => typeof row === "string" ? [...row] : Array.isArray(row) ? row : [])
    : [];
}

function blocksIn(space?: GameState) {
  return Array.isArray(space?.blocks)
    ? space.blocks.map((block) => asRecord(block)).filter((block) => block !== null)
    : [];
}

function blockAt(space: GameState | undefined, row: number, column: number) {
  return blocksIn(space).find(
    (block) => asNumber(block?.row, -1) === row && asNumber(block?.column, -1) === column,
  );
}

function changedSceneCells(
  scene: SceneGraph,
  previousScene: SceneGraph | null,
  spaceId: number,
) {
  if (!previousScene) return new Set<string>();
  const map = sceneMap(scene, spaceId);
  const previousMap = sceneMap(previousScene, spaceId);
  return new Set(
    map.flatMap((row, rowIndex) =>
      row.flatMap((cell, columnIndex) =>
        asString(cell, " ") === asString(previousMap[rowIndex]?.[columnIndex], " ")
          ? []
          : [`${rowIndex}:${columnIndex}`],
      ),
    ),
  );
}

function sceneCellLabel(
  symbol: string,
  block: GameState | null | undefined,
  row: number,
  column: number,
) {
  const definition = asNumber(block?.definition_id, Number(symbol));
  const kind = asString(block?.kind, "");
  const content = kind === "box"
    ? `递归盒子 ${definition}，内部空间已渲染`
    : kind === "player"
      ? symbol === "P" ? "位于玩家目标上的玩家" : "玩家"
      : kind === "portal_wall"
        ? "可进入墙体"
        : cellLabel(symbol, row, column).replace(/^.*：/, "");
  return `第 ${row + 1} 行第 ${column + 1} 列：${content}`;
}

function PlayerFace({ block }: { block?: GameState | null }) {
  return (
    <span
      className="parabox-player-face"
      style={{
        "--player-color": observerColor(block?.color, "#e74678"),
      } as React.CSSProperties}
    >
      <i aria-hidden="true" />
      <i aria-hidden="true" />
    </span>
  );
}

function BoxFace({
  block,
  scene,
  previousScene,
  depth,
  scale,
  flipH,
  legacy = false,
}: {
  block?: GameState | null;
  scene?: SceneGraph;
  previousScene?: SceneGraph | null;
  depth: number;
  scale: number;
  flipH: boolean;
  legacy?: boolean;
}) {
  const definition = asNumber(block?.definition_id, 0);
  const fallback = BOX_COLORS[((definition % BOX_COLORS.length) + BOX_COLORS.length) % BOX_COLORS.length];
  const color = observerColor(block?.color, fallback);
  const subspace = asNumber(block?.subspace, -1);
  const nestedSpace = scene?.spaces.get(subspace);
  const nestedSpan = Math.max(
    asNumber(nestedSpace?.width, 1),
    asNumber(nestedSpace?.height, 1),
  );
  const canRender = nestedSpace && scale / nestedSpan >= MIN_RECURSIVE_SCALE && depth < 12;
  return (
    <span
      className={`parabox-box-face ${legacy ? "legacy" : ""}`}
      data-expanded={canRender || undefined}
      style={{ "--box-color": color } as React.CSSProperties}
    >
      {canRender ? (
        <SpaceGrid
          scene={scene}
          previousScene={previousScene}
          spaceId={subspace}
          depth={depth + 1}
          scale={scale}
          flipH={flipH !== (block?.flip_h === true)}
          compact
        />
      ) : legacy ? (
        <span className="parabox-unrecorded">未记录</span>
      ) : null}
    </span>
  );
}

function SpaceGrid({
  scene,
  previousScene,
  spaceId,
  depth,
  scale,
  flipH,
  compact = false,
  focusSubspace = -1,
}: {
  scene: SceneGraph;
  previousScene: SceneGraph | null;
  spaceId: number;
  depth: number;
  scale: number;
  flipH: boolean;
  compact?: boolean;
  focusSubspace?: number;
}) {
  const space = scene.spaces.get(spaceId);
  const map = sceneMap(scene, spaceId);
  const width = asNumber(space?.width, map[0]?.length ?? 1);
  const height = asNumber(space?.height, map.length || 1);
  const span = Math.max(width, height);
  const changed = changedSceneCells(scene, previousScene, spaceId);
  const displayedMap = flipH ? map.map((row) => [...row].reverse()) : map;
  return (
    <span
      className={`parabox-grid ${compact ? "compact" : ""}`}
      role={compact ? undefined : "grid"}
      aria-hidden={compact || undefined}
      aria-label={compact ? undefined : `${width} 列 ${height} 行的 Parabox 当前场景`}
      style={{
        "--cols": width,
        "--rows": height,
        "--scene-color": observerColor(space?.color, "#2f86cf"),
        gridTemplateColumns: `repeat(${width}, ${100 / span}%)`,
        gridTemplateRows: `repeat(${height}, ${100 / span}%)`,
      } as React.CSSProperties}
    >
      {displayedMap.flatMap((row, rowIndex) =>
        row.map((cell, columnIndex) => {
          const sourceColumn = flipH ? width - 1 - columnIndex : columnIndex;
          const symbol = asString(cell, " ");
          const key = `${rowIndex}:${columnIndex}`;
          const changeKey = `${rowIndex}:${sourceColumn}`;
          const block = blockAt(space, rowIndex, sourceColumn);
          const kind = asString(block?.kind, "");
          const subspace = asNumber(block?.subspace, -1);
          const childScale = scale / Math.max(width, height);
          const focusContainer = focusSubspace >= 0 && subspace === focusSubspace;
          const edges = wallEdges(displayedMap, rowIndex, columnIndex);
          return (
            <span
              className={`parabox-cell ${depth === 0 && changed.has(changeKey) ? "changed" : ""}`}
              key={key}
              role={compact ? undefined : "gridcell"}
              aria-label={compact ? undefined : sceneCellLabel(symbol, block, rowIndex, columnIndex)}
              data-kind={cellKind(symbol)}
              data-focus-container={focusContainer || undefined}
              data-wall-top={edges?.top || undefined}
              data-wall-right={edges?.right || undefined}
              data-wall-bottom={edges?.bottom || undefined}
              data-wall-left={edges?.left || undefined}
            >
              {focusContainer ? null : kind === "player" ? (
                <PlayerFace block={block} />
              ) : kind === "box" ? (
                <BoxFace
                  block={block}
                  scene={scene}
                  previousScene={previousScene}
                  depth={depth}
                  scale={childScale}
                  flipH={flipH}
                />
              ) : [".", "+"].includes(symbol) ? (
                <span className="parabox-target-mark" />
              ) : null}
            </span>
          );
        }),
      )}
    </span>
  );
}

function LegacyGrid({
  map,
  width,
  height,
  changed,
}: {
  map: Json[][];
  width: number;
  height: number;
  changed: Set<string>;
}) {
  const span = Math.max(width, height);
  return (
    <div
      className="parabox-grid legacy-grid"
      role="grid"
      aria-label={`${width} 列 ${height} 行的 Parabox 当前场景；盒内和外层未记录`}
      style={{
        "--cols": width,
        "--rows": height,
        gridTemplateColumns: `repeat(${width}, ${100 / span}%)`,
        gridTemplateRows: `repeat(${height}, ${100 / span}%)`,
      } as React.CSSProperties}
    >
      {map.flatMap((row, rowIndex) =>
        row.map((cell, columnIndex) => {
          const symbol = asString(cell, " ");
          const key = `${rowIndex}:${columnIndex}`;
          const edges = wallEdges(map, rowIndex, columnIndex);
          return (
            <span
              className={`parabox-cell ${changed.has(key) ? "changed" : ""}`}
              key={key}
              role="gridcell"
              aria-label={cellLabel(symbol, rowIndex, columnIndex)}
              data-kind={cellKind(symbol)}
              data-wall-top={edges?.top || undefined}
              data-wall-right={edges?.right || undefined}
              data-wall-bottom={edges?.bottom || undefined}
              data-wall-left={edges?.left || undefined}
            >
              {["@", "P"].includes(symbol) ? (
                <PlayerFace />
              ) : /^\d$/.test(symbol) || symbol === "X" ? (
                <BoxFace
                  block={{ definition_id: /^\d$/.test(symbol) ? Number(symbol) : 0 }}
                  depth={0}
                  scale={0}
                  flipH={false}
                  legacy
                />
              ) : [".", "+"].includes(symbol) ? (
                <span className="parabox-target-mark" />
              ) : null}
            </span>
          );
        }),
      )}
    </div>
  );
}

export function ParaboxState({
  state,
  previousState,
}: {
  state: GameState;
  previousState?: GameState | null;
}) {
  const space = asRecord(state.space);
  const map = spaceMap(state);
  const path = spacePath(state);
  const changed = new Set(boardChanges(state, previousState));
  const scene = observerScene(state);
  const previousScene = observerScene(previousState);
  if (!map.length) {
    return (
      <EmptyState
        title="状态摘要已接入"
        body="该旧版 run 没有记录完整二维地图；分数、关卡和动作流仍为权威数据。"
      />
    );
  }
  const width = asNumber(space?.width, map[0]?.length ?? 1);
  const height = asNumber(space?.height, map.length);
  return (
    <div className="parabox-state">
      <header className="parabox-space-header">
        <div>
          <span>CONTAINER PATH</span>
          <nav aria-label="当前容器路径">
            {path.map((part, index) => (
              <span key={`${part}-${index}`}>
                {index > 0 && <i aria-hidden="true">›</i>}
                <b>
                  {part === "root"
                    ? "ROOT"
                    : part === "…cycle…"
                      ? "CYCLE"
                      : part.replace("box:", "BOX ")}
                </b>
              </span>
            ))}
          </nav>
        </div>
        <div className="parabox-depth">
          <span>{scene ? "完整场景" : "旧事件"}</span>
          <strong>{Math.max(0, path.length - 1)}</strong>
        </div>
      </header>

      <div className={`parabox-stage ${scene ? "recursive" : "legacy"}`}>
        {scene ? (() => {
          const focus = scene.spaces.get(scene.focusSpace);
          const parent = asRecord(focus?.parent);
          const parentId = asNumber(parent?.space, -1);
          const parentSpace = scene.spaces.get(parentId);
          const parentWidth = asNumber(parentSpace?.width, 1);
          const parentHeight = asNumber(parentSpace?.height, 1);
          const parentSpan = Math.max(parentWidth, parentHeight);
          const parentRow = asNumber(parent?.row, 0);
          const parentColumn = asNumber(parent?.column, 0);
          const displayedParentColumn = scene.cameraFlipH
            ? parentWidth - 1 - parentColumn
            : parentColumn;
          const parentX = (parentSpan - parentWidth) / 2 + displayedParentColumn;
          const parentY = (parentSpan - parentHeight) / 2 + parentRow;
          return (
            <div className="parabox-camera" data-flipped={scene.cameraFlipH || undefined}>
              {parentSpace && (
                <span
                  className="parabox-parent-context"
                  aria-hidden="true"
                  style={{
                    width: `${parentSpan * 100}%`,
                    height: `${parentSpan * 100}%`,
                    left: `${-parentX * 100}%`,
                    top: `${-parentY * 100}%`,
                  }}
                >
                  <SpaceGrid
                    scene={scene}
                    previousScene={previousScene}
                    spaceId={parentId}
                    depth={0}
                    scale={parentSpan}
                    flipH={scene.cameraFlipH}
                    compact
                    focusSubspace={scene.focusSpace}
                  />
                </span>
              )}
              <span className="parabox-focus-space">
                <SpaceGrid
                  scene={scene}
                  previousScene={previousScene}
                  spaceId={scene.focusSpace}
                  depth={0}
                  scale={1}
                  flipH={scene.cameraFlipH}
                />
              </span>
            </div>
          );
        })() : (
          <>
            <div className="parabox-legacy-notice" role="note">
              旧事件只记录了当前空间；盒子内部与外层场景未记录。
            </div>
            <LegacyGrid map={map} width={width} height={height} changed={changed} />
          </>
        )}
      </div>

      <footer className="parabox-legend" aria-label="场景图例">
        <span><i data-legend="player" />玩家</span>
        <span><i data-legend="box" />递归盒子（内部为真实子空间）</span>
        <span><i data-legend="box-goal" />盒子目标</span>
        <span><i data-legend="player-goal" />玩家目标</span>
        <span><i data-legend="changed" />相较上一步有变化</span>
      </footer>
    </div>
  );
}

function describeParaboxEvent(
  event: ObserverEvent,
  previous?: ObserverEvent | null,
): EventDescription {
  const command = asString(event.action?.command, event.type ?? "state").toLowerCase();
  const state = event.state ?? {};
  const previousState = previous?.state;
  const level = asRecord(state.level);
  const path = spacePath(state);
  const previousPath = spacePath(previousState);
  const map = spaceMap(state);
  const previousMap = spaceMap(previousState);
  const changed = boardChanges(state, previousState).length;
  const ok = event.result?.ok !== false;
  const score = event.score_delta ?? asNumber(event.result?.score_delta, 0);

  if (!ok) {
    return {
      label: "OPERATION BLOCKED",
      title: `${command.toUpperCase()} 未改变场景`,
      detail: `Sidecar 返回失败；权威状态仍停留在事件 #${event.sequence}。`,
      tone: "warning",
    };
  }
  if (score > 0) {
    return {
      label: "PUZZLE SOLVED",
      title: `完成 ${asString(level?.reference, "当前关卡")}`,
      detail: `本步增加 ${score} 分，累计分数 ${event.score ?? "—"}。`,
      tone: "success",
    };
  }
  if (command === "select") {
    return {
      label: "LEVEL SELECTED",
      title: `${asString(level?.reference, "关卡")} · ${asString(level?.title, "未命名")}`,
      detail: `进入 ${asString(level?.area, "未知区域")} 区域，从 ROOT 层开始。`,
    };
  }
  if (command === "restart") {
    return {
      label: "LEVEL RESET",
      title: "重新开始当前关卡",
      detail: `棋盘恢复到 ${asString(level?.reference, "当前关卡")} 的初始状态。`,
      tone: "warning",
    };
  }
  if (command === "inspect") {
    return {
      label: "BOX INSPECTION",
      title: "检查递归盒子的内部空间",
      detail: `当前玩家仍位于 ${path.join(" › ") || "ROOT"}；检查不会改变权威棋盘。`,
    };
  }
  if (command === "show") {
    return {
      label: "STATE CHECKED",
      title: "读取当前权威状态",
      detail: `棋盘没有推进；Agent 查看了 ${path.join(" › ") || "ROOT"} 的当前位置。`,
    };
  }
  if (path.join("/") !== previousPath.join("/") && previousPath.length) {
    return {
      label: "RECURSION TRANSITION",
      title: path.length > previousPath.length ? "进入更深一层盒子" : "返回外层容器",
      detail: `${previousPath.join(" › ")} → ${path.join(" › ")}。`,
    };
  }
  const player = positionOf(map, ["@", "P"]);
  const previousPlayer = positionOf(previousMap, ["@", "P"]);
  const goalDelta = completedGoals(map) - completedGoals(previousMap);
  const movement = player && previousPlayer
    ? `玩家从 r${previousPlayer.row + 1}c${previousPlayer.column + 1} 到 r${player.row + 1}c${player.column + 1}`
    : "玩家或盒子位置发生变化";
  return {
    label: command === "undo" ? "MOVE UNDONE" : "BOARD ADVANCED",
    title: command === "undo" ? "撤销上一步" : `${asNumber(event.action?.argument_count, 1)} 个方向输入已执行`,
    detail: `${movement}；共 ${changed} 个格子变化${goalDelta ? `，目标占用 ${goalDelta > 0 ? "+" : ""}${goalDelta}` : ""}。`,
  };
}

export default {
  id: "parabox",
  meta: { label: "Patrick’s Parabox", short: "PARABOX", accent: "#52c8ff" },
  State: ParaboxState,
  describeEvent: describeParaboxEvent,
} satisfies GameObserverModule;
