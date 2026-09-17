import type { GameState } from "../game-observer";
import { num, rec, str, records, strings } from "./data";
import { Painter, type Viewport } from "./scene";

export function tileBoardScene(kind: "sokoban" | "minesweeper", state: GameState, view: Viewport) {
  const mines = kind === "minesweeper", accent = mines ? "#56d6c4" : "#f0b44d";
  const board = rec(state.board), level = rec(state.level), campaign = rec(state.campaign);
  const map = strings(board?.map), cols = Math.max(1, num(board?.width, map[0]?.length ?? 1)), rows = Math.max(1, map.length);
  const tiers = records(state.tiers), wide = view.width >= 680, aside = wide && tiers.length ? 220 : 0;
  const boardArea = { x: 16, y: 60, width: view.width - aside - 32, height: Math.max(240, Math.min(410, view.height * .43)) };
  const tile = Math.min(mines ? 36 : 32, (boardArea.width - 20) / cols, (boardArea.height - 20) / rows);
  const left = boardArea.x + (boardArea.width - cols * tile) / 2, top = boardArea.y + (boardArea.height - rows * tile) / 2;
  const p = new Painter(view.width, mines ? "#09100f" : "#0b0d0d");
  p.rect(0, 0, view.width, 46, "#111d1b");
  p.text(16, 10, `${str(level?.id, "等待关卡")}  ${num(campaign?.score)}/${num(campaign?.max_score)}`, 13, accent, true);
  p.text(view.width - 16, 29, mines ? `${str(board?.status, "READY").toUpperCase()} · SAFE ${num(board?.remaining_safe)}` : `MOVE ${num(board?.moves)} · PUSH ${num(board?.pushes)}`, 11, "#9aafa5", false, "right");
  if (!map.length) p.text(boardArea.x + boardArea.width / 2, top + 80, "等待棋盘状态", 14, "#91a69c", false, "center");
  if (map.length) p.rect(left - 1, top - 1, cols * tile + 2, rows * tile + 2, "#394943");
  for (let row = 0; row < map.length; row++) for (let col = 0; col < cols; col++) {
    const symbol = map[row]?.[col] ?? " ", x = left + col * tile, y = top + row * tile;
    if (mines) {
      const covered = symbol === "?", flagged = symbol === "F", exploded = symbol === "X";
      p.rect(x + .5, y + .5, tile - 1, tile - 1, covered ? "#2d5a53" : flagged ? accent : exploded ? "#ff6f70" : symbol === "." ? "#b9cfca" : "#d8e8e3");
      if (covered) { p.line([x + 1, y + tile - 1, x + 1, y + 1, x + tile - 1, y + 1], "#59847a", 1); }
      else if (flagged) { p.line([x + tile * .3, y + tile * .78, x + tile * .3, y + tile * .22], "#102b24", 2); p.polygon([x + tile * .33, y + tile * .22, x + tile * .75, y + tile * .35, x + tile * .33, y + tile * .48], "#102b24"); }
      else if (exploded) { p.circle(x + tile / 2, y + tile / 2, tile * .23, "#f9f1e9"); for (let i = 0; i < 4; i++) { const a = i * Math.PI / 4, dx = Math.cos(a) * tile * .36, dy = Math.sin(a) * tile * .36; p.line([x + tile / 2 - dx, y + tile / 2 - dy, x + tile / 2 + dx, y + tile / 2 + dy], "#f9f1e9", 1.5); } }
      else if (/^[1-8]$/.test(symbol)) p.text(x + tile / 2, y + tile * .18, symbol, Math.max(8, tile * .48), ["", "#1763a6", "#13733e", "#c5483f", "#513c9c", "#8d322b", "#08777b", "#202629", "#66706d"][Number(symbol)], true, "center");
    } else {
      p.rect(x + .5, y + .5, tile - 1, tile - 1, symbol === "#" ? "#55534d" : "#191b1a");
      if (symbol === "#") p.line([x + 1, y + tile - 1, x + tile - 1, y + 1], "#68655e", Math.max(1, tile * .04));
      if ([".", "*", "+"].includes(symbol)) p.circle(x + tile / 2, y + tile / 2, tile * .18, undefined, accent, 2);
      if (["$", "*"].includes(symbol)) { const color = symbol === "*" ? "#d6f05c" : accent; p.rect(x + tile * .13, y + tile * .13, tile * .74, tile * .74, color, 1, "#a97c31", 2); p.line([x + tile * .23, y + tile * .23, x + tile * .77, y + tile * .77], "#8d672d", 1.5); }
      if (["@", "+"].includes(symbol)) p.circle(x + tile / 2, y + tile / 2, tile * .3, "#58a7d8", "#bce3ff", Math.max(1, tile * .05));
    }
  }
  let tierY = wide ? 62 : boardArea.y + boardArea.height + 18;
  const tierX = wide ? view.width - aside + 12 : 16, tierWidth = wide ? aside - 28 : view.width - 32;
  if (tiers.length) p.text(tierX, tierY, "PROGRESSION", 12, "#91a69c", true);
  tierY += tiers.length ? 26 : 0;
  for (const tier of tiers) {
    const locked = str(tier.status) === "locked";
    p.text(tierX, tierY, str(tier.title), 12, locked ? "#63736b" : "#c7d3cb");
    p.text(tierX + tierWidth, tierY, `${num(tier.solved)}/${num(mines ? tier.wins_required : tier.total)}`, 12, accent, true, "right");
    p.text(tierX, tierY + 19, mines ? `${str(tier.status)} · ${num(tier.failed)} failed · ${num(tier.attempts_remaining)} left` : `${str(tier.status)}${num(tier.remaining_to_unlock_next) > 0 ? ` · ${num(tier.remaining_to_unlock_next)} to next` : ""}`, 10, "#839a8d");
    p.bar(tierX, tierY + 38, tierWidth, num(tier.solved) / Math.max(1, num(mines ? tier.wins_required : tier.total)), accent, 3);
    tierY += 54;
  }
  const bottom = Math.max(boardArea.y + boardArea.height, tierY) + 14;
  if (mines) p.text(16, bottom, `NO-GUESS VERIFIED · ${num(rec(state.guarantee)?.safe_radius, 1) === 1 ? "3×3 FIRST-CLICK SAFE" : "FIRST CLICK SAFE"}`, 10, accent);
  return p.finish(bottom + (mines ? 30 : 4), `${mines ? "Minesweeper" : "Sokoban"} ${str(level?.title)}，${cols} 列 ${rows} 行，得分 ${num(campaign?.score)}`);
}
