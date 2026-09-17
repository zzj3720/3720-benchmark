import type { GameState, Json } from "../../../observer-platform/app/game-observer";
import { num, rec, str, records } from "../../../observer-platform/app/webgl/data";
import { Painter, intersect, mix, rgb, type Rect, type SceneBuilder } from "../../../observer-platform/app/webgl/scene";

const BOX_COLORS = ["#40d9b5", "#f2aa3b", "#4dbbff", "#e94b82", "#9fd43c", "#ff795e", "#8c7cf4", "#36cfe8", "#efcc4f", "#64d477"];
const mapRows = (value: Json | undefined) => Array.isArray(value) ? value.map(row => typeof row === "string" ? [...row] : Array.isArray(row) ? row : []) : [];
const wall = (value: Json | undefined) => ["#", "!"].includes(str(value, " "));

export const buildParaboxScene: SceneBuilder = (state, view) => {
  const focusSize = Math.max(80, Math.min(view.width - 32, view.height * (view.compact ? .32 : .36), view.compact ? 340 : 400));
  const height = focusSize + 32, p = new Painter(view.width, "#081827");
  const focusRect = { x: (view.width - focusSize) / 2, y: 16, width: focusSize, height: focusSize };
  const raw = rec(state.observer_scene), spaces = new Map<number, GameState>();
  for (const space of records(raw?.spaces)) spaces.set(num(space.id, -1), space);
  const focusId = num(raw?.focus_space, -1), rootId = num(raw?.root_space, -1);
  const recursive = str(raw?.schema) === "parabox-observer-scene-v1" && spaces.has(focusId) && spaces.has(rootId);
  const legacy = rec(state.space), legacyMap = mapRows(legacy?.map);
  if (!recursive && !legacyMap.length) {
    p.text(16, 28, "尚无棋盘快照", 14, "#b4c9d7");
    return p.finish(120, "Parabox：尚无棋盘快照");
  }
  const blockMaps = new Map<number, Map<string, GameState>>();
  for (const [id, space] of spaces) blockMaps.set(id, new Map(records(space.blocks).map(block => [`${num(block.row)}:${num(block.column)}`, block])));

  const player = (x: number, y: number, tile: number, color: string, nested: boolean) => {
    const bevel = Math.min(nested ? 1 : 5, tile * .07);
    if (!nested) {
      p.rect(x - tile * .08, y + tile * .42, tile * .08, tile * .18, mix(color, "#0b243a", .7));
      p.rect(x + tile, y + tile * .42, tile * .08, tile * .18, mix(color, "#0b243a", .7));
    }
    p.rect(x, y + Math.min(4, tile * .06), tile, tile, "#041421", 0, undefined, 0, .6);
    p.rect(x, y, tile, tile, mix(color, "#ffffff", .82), 0, mix(color, "#28101d", .62), Math.min(1, tile * .04));
    p.rect(x, y, tile, bevel, mix(color, "#ffffff", .58));
    p.rect(x, y, bevel, tile, mix(color, "#ffffff", .58));
    p.rect(x + tile - bevel, y + bevel, bevel, tile - bevel, mix(color, "#28101d", .68));
    p.rect(x + bevel, y + tile - bevel, tile - bevel, bevel, mix(color, "#28101d", .68));
    p.circle(x + tile * .34, y + tile * .5, tile * .115, "#42132a");
    p.circle(x + tile * .66, y + tile * .5, tile * .115, "#42132a");
  };
  const drawSpace = (id: number, area: Rect, depth: number, flip: boolean, clip: Rect, skipSubspace = -1, old = false) => {
    const space = old ? legacy : spaces.get(id);
    const map = old ? legacyMap : mapRows(space?.map);
    const cols = Math.max(1, num(space?.width, map[0]?.length ?? 1)), rows = Math.max(1, num(space?.height, map.length));
    const span = Math.max(cols, rows), tile = area.width / span;
    const originX = area.x + (area.width - cols * tile) / 2, originY = area.y + (area.height - rows * tile) / 2;
    const sceneColor = rgb(space?.color, "#2f86cf"), floor = mix(sceneColor, "#071521", .34), wallColor = mix(sceneColor, "#ffffff", .8);
    p.clipped(clip, () => {
      p.rect(area.x, area.y, area.width, area.height, floor);
      for (let row = 0; row < rows; row++) for (let col = 0; col < cols; col++) {
        const sourceCol = flip ? cols - 1 - col : col, symbol = str(map[row]?.[sourceCol], " ");
        const x = originX + col * tile, y = originY + row * tile;
        const cell = { x, y, width: tile, height: tile }, visible = intersect(cell, clip);
        if (!visible.width || !visible.height) continue;
        const block = blockMaps.get(id)?.get(`${row}:${sourceCol}`), kind = str(block?.kind, ""), subspace = num(block?.subspace, -1);
        const goal = [".", "+", "X", "P"].includes(symbol);
        p.rect(x, y, tile, tile, goal ? mix(sceneColor, floor, .28) : floor);
        if (wall(symbol)) {
          const leftCol = flip ? sourceCol + 1 : sourceCol - 1, rightCol = flip ? sourceCol - 1 : sourceCol + 1;
          const top = !wall(map[row - 1]?.[sourceCol]), bottom = !wall(map[row + 1]?.[sourceCol]), left = !wall(map[row]?.[leftCol]), right = !wall(map[row]?.[rightCol]);
          const radius = tile * .22, corners = [top && right, bottom && right, bottom && left, top && left];
          const centers = [[x + tile - radius, y + radius], [x + tile - radius, y + tile - radius], [x + radius, y + tile - radius], [x + radius, y + radius]];
          const points: number[] = [];
          for (let corner = 0; corner < 4; corner++) {
            if (!corners[corner]) { points.push(corner < 2 ? x + tile : x, corner === 0 || corner === 3 ? y : y + tile); continue; }
            for (let step = 0; step <= 6; step++) { const angle = (corner - 1) * Math.PI / 2 + step * Math.PI / 12; points.push(centers[corner][0] + Math.cos(angle) * radius, centers[corner][1] + Math.sin(angle) * radius); }
          }
          p.polygon(points, wallColor);
          const edge = Math.min(3, tile * .06);
          if (top) p.line([x + (left ? radius : 0), y + edge / 2, x + tile - (right ? radius : 0), y + edge / 2], mix(wallColor, "#ffffff", .72), edge);
          if (left) p.line([x + edge / 2, y + (top ? radius : 0), x + edge / 2, y + tile - (bottom ? radius : 0)], mix(wallColor, "#ffffff", .82), edge);
          if (right) p.line([x + tile - edge / 2, y + (top ? radius : 0), x + tile - edge / 2, y + tile - (bottom ? radius : 0)], mix(wallColor, "#071521", .76), edge);
          if (bottom) p.line([x + (left ? radius : 0), y + tile - edge / 2, x + tile - (right ? radius : 0), y + tile - edge / 2], mix(wallColor, "#071521", .68), edge);
          continue;
        }
        if (goal) {
          const inset = depth ? .15 : .19, stroke = depth ? Math.min(1, tile * .04) : Math.min(4, tile * .05);
          if (["+", "P"].includes(symbol)) p.circle(x + tile / 2, y + tile / 2, tile * (.5 - inset), undefined, "#88c7ed", stroke, .82);
          else p.rect(x + tile * inset, y + tile * inset, tile * (1 - inset * 2), tile * (1 - inset * 2), undefined, 0, "#88c7ed", stroke, .82);
        }
        if (subspace === skipSubspace && skipSubspace >= 0) continue;
        if (kind === "player" || (old && ["@", "P"].includes(symbol))) player(x, y, tile, rgb(block?.color, "#e74678"), depth > 0);
        else if (kind === "box" || (old && (/^\d$/.test(symbol) || symbol === "X"))) {
          const definition = num(block?.definition_id, Number(symbol) || 0), color = rgb(block?.color, BOX_COLORS[((definition % BOX_COLORS.length) + BOX_COLORS.length) % BOX_COLORS.length]);
          const child = spaces.get(subspace), childSpan = Math.max(num(child?.width, 1), num(child?.height, 1));
          if (!old && child && depth < 12 && tile / childSpan >= focusSize / 512) drawSpace(subspace, cell, depth + 1, flip !== (block?.flip_h === true), visible);
          else {
            p.rect(x, y, tile, tile, old ? mix(color, "#526170", .72) : color);
            if (old && tile > 24) { p.rect(x + tile * .23, y + tile * .23, tile * .54, tile * .54, mix(color, "#173653", .67)); p.text(x + tile / 2, y + tile * .4, "未记录", Math.min(9, tile * .16), "#10253a", true, "center"); }
          }
        } else if ([".", "+"].includes(symbol)) { p.circle(x + tile / 2, y + tile / 2, tile * .14, "#6ec6ff", undefined, 0, .12); p.circle(x + tile / 2, y + tile / 2, tile * .065, "#9bd8ff"); }
      }
    });
  };
  if (recursive) {
    const focus = spaces.get(focusId), parent = rec(focus?.parent), parentId = num(parent?.space, -1), parentSpace = spaces.get(parentId), flip = raw?.camera_flip_h === true;
    if (parentSpace) {
      const cols = num(parentSpace.width, 1), rows = num(parentSpace.height, 1), span = Math.max(cols, rows);
      const col = flip ? cols - 1 - num(parent?.column) : num(parent?.column), row = num(parent?.row);
      drawSpace(parentId, { x: focusRect.x - ((span - cols) / 2 + col) * focusSize, y: focusRect.y - ((span - rows) / 2 + row) * focusSize, width: span * focusSize, height: span * focusSize }, 0, flip, { x: 0, y: 0, width: view.width, height }, focusId);
    }
    drawSpace(focusId, focusRect, 0, flip, focusRect);
  } else {
    drawSpace(-1, focusRect, 0, false, focusRect, -1, true);
    p.rect(8, 4, Math.min(view.width - 16, 320), 20, "#2d2007", 2, "#9f7920");
    p.text(14, 7, "旧记录：盒内与外层场景未记录", 10, "#ffe6a3");
  }
  return p.finish(height, `Parabox ${str(rec(state.level)?.reference, "")}，${recursive ? "递归空间" : "历史当前空间"}`);
};
