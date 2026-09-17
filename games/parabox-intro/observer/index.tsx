import { GameCanvas } from "../../../observer-platform/app/webgl/game-canvas";
import { buildParaboxScene } from "./webgl";
import {
  asNumber,
  asRecord,
  asString,
  type EventDescription,
  type GameObserverModule,
  type GameState,
  type Json,
  type ObserverEvent,
} from "../../../observer-platform/app/game-observer";

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

export function ParaboxState({ state, previousState }: { state: GameState; previousState?: GameState | null }) {
  return <GameCanvas state={state} previousState={previousState} build={buildParaboxScene} label="Parabox" />;
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
  stateContext: (state: GameState) => {
    const path = spacePath(state);
    if (!path.length) return null;
    return path
      .map((part) => part === "root" ? "ROOT" : part === "…cycle…" ? "CYCLE" : part.replace("box:", "BOX "))
      .join(" › ");
  },
  describeEvent: describeParaboxEvent,
} satisfies GameObserverModule;
