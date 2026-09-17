import type { Json } from "../../../observer-platform/app/game-observer";
import { num, rec, str, records } from "../../../observer-platform/app/webgl/data";
import { Painter, type SceneBuilder } from "../../../observer-platform/app/webgl/scene";

export const buildSwarmScene: SceneBuilder = (state, view) => {
  const p = new Painter(view.width, "#08100f"), tick = num(state.tick), deadline = Math.max(1, num(state.deadline_ticks, 1));
  const robots = records(state.robots).map(robot => {
    const location = rec(robot.location), coords = Array.isArray(location?._planar) ? location._planar : [];
    return { raw: robot, id: num(robot.id), name: str(robot.name, "robot"), x: num(coords[0]), y: num(coords[1]), active: robot.active === true, waiting: typeof robot.waiting_until === "number" };
  });
  const goals: { text: string; done: boolean }[] = [];
  const visit = (value: Json | undefined) => {
    if (Array.isArray(value)) { value.forEach(visit); return; }
    const item = rec(value); if (!item) return;
    if (typeof item._objectiveGoal === "string") goals.push({ text: item._objectiveGoal, done: item._objectiveAchievement != null });
    Object.values(item).forEach(visit);
  }; visit(state.objectives);
  p.text(16, 12, `TICK ${tick.toLocaleString()} / ${deadline.toLocaleString()}`, 12, "#a2b6ac", true);
  p.text(view.width - 16, 32, `${state.won ? "WON" : `SCORE ${num(state.score)}`} · ${state.program_running ? "RUNNING" : "IDLE"}`, 11, state.won ? "#a3ff62" : "#5ed7ff", true, "right");
  p.bar(16, 52, view.width - 32, tick / deadline, "#5ed7ff");
  const wide = view.width >= 680, mapWidth = wide ? Math.floor(view.width * .58) - 24 : view.width - 32;
  const map = { x: 16, y: 72, width: mapWidth, height: Math.max(220, Math.min(340, mapWidth * .65)) };
  p.rect(map.x, map.y, map.width, map.height, "#091313", 0, "#213531");
  for (let x = map.x + 24; x < map.x + map.width; x += 24) p.line([x, map.y, x, map.y + map.height], "#172927");
  for (let y = map.y + 24; y < map.y + map.height; y += 24) p.line([map.x, y, map.x + map.width, y], "#172927");
  const minX = Math.min(0, ...robots.map(robot => robot.x)), maxX = Math.max(0, ...robots.map(robot => robot.x));
  const minY = Math.min(0, ...robots.map(robot => robot.y)), maxY = Math.max(0, ...robots.map(robot => robot.y));
  for (const robot of robots) {
    const x = map.x + 24 + (robot.x - minX) / Math.max(1, maxX - minX) * (map.width - 48);
    const y = map.y + 24 + (maxY - robot.y) / Math.max(1, maxY - minY) * (map.height - 48);
    const color = robot.active ? "#5ed7ff" : robot.waiting ? "#e8b669" : "#6d7d74";
    if (robot.id === 0) p.rect(x - 5, y - 5, 10, 10, "#08100f", 0, "#e0eee5", 2);
    else p.circle(x, y, 3.5, color);
    p.hit({ x: x - 7, y: y - 7, width: 14, height: 14 }, `${robot.name} #${robot.id} @ (${robot.x}, ${robot.y}) · ${robot.active ? "active" : robot.waiting ? `waiting until ${num(robot.raw.waiting_until)}` : "idle"}`);
  }
  const active = robots.filter(robot => robot.active).length;
  p.text(16, map.y + map.height + 10, `活跃 ${active} · 待命 ${robots.length - active} · 共 ${robots.length} 台`, 11, "#8fa69a");
  const sideX = wide ? map.x + map.width + 20 : 16, sideWidth = view.width - sideX - 16;
  let y = wide ? 76 : map.y + map.height + 44;
  p.text(sideX, y, "OBJECTIVES", 12, "#9cb2a6", true); y += 26;
  for (const goal of goals) { y = p.paragraph(sideX, y, `${goal.done ? "✓" : "·"} ${goal.text}`, sideWidth, 12, goal.done ? "#a3ff62" : "#c0d0c6", 12) + 12; }
  if (!goals.length) { p.text(sideX, y, "无可见目标", 12, "#82998b"); y += 26; }
  const base = robots.find(robot => robot.id === 0) ?? robots[0];
  if (base) {
    p.text(sideX, y, "BASE ROBOT", 12, "#9cb2a6", true); y += 26;
    const inventory = records(base.raw.inventory).filter(item => num(item.count) > 0).slice(0, 8);
    for (const item of inventory) { y = p.paragraph(sideX, y, `${str(item.name)} ×${num(item.count)}`, sideWidth, 12, "#d1dfd7", 2) + 4; }
    if (!inventory.length) { p.text(sideX, y, "库存为空", 12, "#82998b"); y += 22; }
    const log = Array.isArray(base.raw.log) ? base.raw.log.filter((line): line is string => typeof line === "string") : [];
    if (log.length) y = p.paragraph(sideX, y + 8, log.at(-1)!, sideWidth, 11, "#94aca0", 8);
  }
  y = Math.max(y + 16, map.y + map.height + 42);
  if (robots.length > 1) {
    const chipWidth = 100, columns = Math.max(1, Math.floor((view.width - 32) / chipWidth));
    robots.forEach((robot, index) => {
      const x = 16 + index % columns * chipWidth, top = y + Math.floor(index / columns) * 26;
      p.circle(x + 5, top + 10, 3, robot.active ? "#5ed7ff" : robot.waiting ? "#e8b669" : "#6d7d74");
      p.text(x + 14, top + 3, robot.name.length > 10 ? robot.name.slice(0, 9) + "…" : robot.name, 10, "#a7bbaf");
      p.hit({ x, y: top, width: chipWidth - 4, height: 24 }, `#${robot.id} ${robot.name} @ (${robot.x}, ${robot.y})`);
    });
    y += Math.ceil(robots.length / columns) * 26;
  }
  return p.finish(y + 12, `Swarm，${robots.length} 台机器人，活跃 ${active}，tick ${tick}`);
};
