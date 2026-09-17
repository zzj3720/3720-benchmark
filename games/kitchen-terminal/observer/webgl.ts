import type { GameState } from "../../../observer-platform/app/game-observer";
import { num, rec, str, records, point, friendly, clock } from "../../../observer-platform/app/webgl/data";
import { Painter, type SceneBuilder } from "../../../observer-platform/app/webgl/scene";

const labels: Record<string, string> = { ingredient_crate: "ING", workstation: "CHOP", cooker: "HEAT", delivery: "SERVE", sink: "WASH", plate_return: "PLATE", bin: "BIN", switch: "SWITCH", loose_item: "LOOSE", moving_barrier: "DOOR", counter: "", structure: "" };
const colors: Record<string, string> = { ingredient_crate: "#46705e", workstation: "#826c4f", cooker: "#8b493e", delivery: "#d89b3f", sink: "#427080", plate_return: "#6b7180", bin: "#4d5a48", switch: "#725c8f", loose_item: "#4d5e64", moving_barrier: "#4f5d70", counter: "#4b443b", structure: "#4b443b" };
function itemShort(item: GameState | null) {
  if (!item) return "";
  const kind = str(item.kind, ""), contents = Array.isArray(item.contents) ? item.contents : [];
  if (kind === "plate") return contents.length ? String(contents.length) : "P";
  if (kind === "dirty_plate_stack") return `D${num(item.count)}`;
  if (kind === "extinguisher") return "EXT";
  if (kind === "container") return contents.length ? String(contents.length) : "POT";
  return friendly(str(item.name, "?")).slice(0, 2).toUpperCase();
}
function hash(value: string) { let result = 0; for (const char of value) result = (result * 31 + char.charCodeAt(0)) | 0; return Math.abs(result); }

export const buildKitchenScene: SceneBuilder = (state, view) => {
  const p = new Painter(view.width, "#100f0c"), shift = rec(state.shift), campaign = rec(state.campaign), map = rec(state.map);
  const chefs = records(state.chefs), orders = records(state.orders), hazards = records(state.hazards), works = records(state.works);
  if (!works.length && rec(state.work)) works.push(rec(state.work)!);
  const chefHeights = chefs.map(chef => point(chef.world)?.y).filter((value): value is number => value !== undefined);
  const visible = (row: GameState) => !chefHeights.length || (point(row.world) !== null && chefHeights.some(height => Math.abs(point(row.world)!.y - height) <= 2.5));
  const allCells = records(map?.walkable), cells = allCells.filter(visible), objects = records(map?.objects).filter(visible);
  const systems = records(map?.systems).filter(system => ["ConveyorStation", "FireballSpawner", "MeteorManager", "PressureSwitchCosmeticDecisions", "SwitchCosmeticDecisions", "TriggerZone"].includes(str(system.kind)) && visible(system));
  const stats = [["KITCHEN", `LEVEL ${num(campaign?.level)}`], ["TIME LEFT", clock(num(shift?.remaining_ms))], ["SCORE", String(num(campaign?.score))], ["STARS", `${num(campaign?.stars)}/3`], ["ORDERS", String(orders.length)]];
  const cols = view.width < 520 ? 3 : 5, statWidth = view.width / cols, header = Math.ceil(stats.length / cols) * 44;
  p.rect(0, 0, view.width, header, "#211e16");
  stats.forEach(([label, value], i) => { const x = i % cols * statWidth + 12, y = Math.floor(i / cols) * 44 + 7; p.text(x, y, label, 10, "#a69d87"); p.text(x, y + 17, value, 12, "#ffe4a1", true); });
  const wide = view.width >= 680, mapWidth = wide ? Math.floor(view.width * .66) - 24 : view.width - 32;
  const area = { x: 16, y: header + 16, width: mapWidth, height: Math.max(270, Math.min(410, mapWidth * .68)) };
  p.rect(area.x, area.y, area.width, area.height, "#12120f", 4, "#39372b");
  p.text(area.x + 10, area.y + 9, `${cells.length === allCells.length ? cells.length : `${cells.length}/${allCells.length}`} WALKABLE CELLS`, 11, "#bbaa86", true);
  const points = [...cells, ...objects, ...systems, ...chefs, ...hazards].map(row => point(row.world)).filter(value => value !== null);
  if (points.length) {
    const minX = Math.min(...points.map(pt => pt.x)) - 1.2, maxX = Math.max(...points.map(pt => pt.x)) + 1.2;
    const minZ = Math.min(...points.map(pt => pt.z)) - 1.2, maxZ = Math.max(...points.map(pt => pt.z)) + 1.2;
    const scale = Math.min((area.width - 16) / Math.max(4, maxX - minX), (area.height - 46) / Math.max(4, maxZ - minZ));
    const left = area.x + (area.width - (maxX - minX) * scale) / 2, top = area.y + 30 + (area.height - 38 - (maxZ - minZ) * scale) / 2;
    const at = (row: GameState) => { const pt = point(row.world); return pt ? { x: left + (pt.x - minX) * scale, y: top + (pt.z - minZ) * scale } : null; };
    p.clipped({ x: area.x + 4, y: area.y + 28, width: area.width - 8, height: area.height - 32 }, () => {
      for (const cell of cells) { const pt = at(cell); if (!pt) continue; p.rect(pt.x - .47 * scale, pt.y - .47 * scale, .94 * scale, .94 * scale, cell.moving ? "#355c5d" : ["#3a3932", "#373932", "#3c3730", "#34383a"][hash(str(cell.grid_manager)) % 4], .08 * scale, cell.moving ? "#79cbd0" : "#4a4940", .025 * scale); }
      const workTargets = new Set(works.map(work => str(work.target)));
      for (const object of objects) {
        const pt = at(object); if (!pt) continue;
        const kind = str(object.kind, "structure"), supply = str(object.supply, ""), item = rec(object.item);
        const label = supply ? friendly(supply).slice(0, 3).toUpperCase() : labels[kind] ?? kind.slice(0, 4).toUpperCase(), held = itemShort(item);
        p.rect(pt.x - .52 * scale, pt.y - .52 * scale, 1.04 * scale, 1.04 * scale, colors[kind] ?? "#605548", .1 * scale, workTargets.has(str(object.id)) ? "#ffe071" : "#1b1814", (workTargets.has(str(object.id)) ? .14 : .08) * scale);
        if (label) p.text(pt.x, pt.y - .1 * scale, label, Math.max(4, .19 * scale), "#fff4de", true, "center");
        if (held) { p.circle(pt.x, pt.y - .02 * scale, .31 * scale, "#f4df89", "#4d3e21", .05 * scale); p.text(pt.x, pt.y - .1 * scale, held, Math.max(4, .18 * scale), "#2b2418", true, "center"); }
        const count = object.plate_count ?? object.dirty_plate_count;
        if (typeof count === "number") p.text(pt.x + .43 * scale, pt.y - .45 * scale, String(count), Math.max(5, .19 * scale), "#ffffff", true, "center");
        if (typeof object.fire_strength === "number") {
          p.circle(pt.x, pt.y, (.34 + object.fire_strength * .15) * scale, "#f74e23", "#ff6b2d", .05 * scale, .35);
          p.polygon([pt.x, pt.y - .38 * scale, pt.x + .22 * scale, pt.y + .05 * scale, pt.x, pt.y + .25 * scale, pt.x - .2 * scale, pt.y + .05 * scale], "#ff9a32", "#ffe06c", .04 * scale);
        }
        p.hit({ x: pt.x - .52 * scale, y: pt.y - .52 * scale, width: scale, height: scale }, `${str(object.name)}${supply ? ` · supplies ${supply}` : ""}${item ? ` · ${str(item.name)}` : ""}`);
      }
      for (const system of systems) {
        const pt = at(system); if (!pt) continue;
        if (str(system.kind) === "ConveyorStation") {
          const target = objects.find(object => str(object.id) === str(system.target)), to = target ? at(target) : null;
          if (to) p.line([pt.x, pt.y, to.x, to.y], "#69c7d1", .07 * scale);
          p.circle(pt.x, pt.y, .3 * scale, "#2b5960", "#69c7d1", .05 * scale);
          p.circle(pt.x - .1 * scale, pt.y, .04 * scale, "#b8f6f3"); p.circle(pt.x + .1 * scale, pt.y, .04 * scale, "#b8f6f3");
          if (system.active) p.circle(pt.x, pt.y, (.12 + Math.min(1, num(system.progress)) * .12) * scale, "#ffcf54");
        } else p.polygon([pt.x, pt.y - .36 * scale, pt.x + .36 * scale, pt.y, pt.x, pt.y + .36 * scale, pt.x - .36 * scale, pt.y], system.active ? "#ffcf54" : "#ef634a", "#ef634a", .06 * scale, .35);
        p.hit({ x: pt.x - .4 * scale, y: pt.y - .4 * scale, width: .8 * scale, height: .8 * scale }, `${str(system.kind)} · ${str(system.name)}`);
      }
      for (const hazard of hazards) {
        const pt = at(hazard); if (!pt) continue;
        const kind = str(hazard.kind), seconds = Math.max(0, Math.ceil(num(hazard.remaining_ms) / 1000));
        if (kind === "meteor") { p.circle(pt.x, pt.y, 1.65 * scale, "#ee4b2f", "#ef634a", .07 * scale, .18); p.circle(pt.x, pt.y, .26 * scale, "#ef634a", "#ffd07b", .05 * scale); p.text(pt.x, pt.y - .12 * scale, String(seconds), Math.max(6, .25 * scale), "#fff3d0", true, "center"); }
        else if (kind === "fireball") p.polygon([pt.x - .42 * scale, pt.y, pt.x - .12 * scale, pt.y - .22 * scale, pt.x + .34 * scale, pt.y, pt.x - .12 * scale, pt.y + .22 * scale], "#ff7a32", "#ffe38b", .06 * scale);
        else p.circle(pt.x, pt.y, .42 * scale, "#ff5427", "#ff773b", .07 * scale, .3);
        p.hit({ x: pt.x - .5 * scale, y: pt.y - .5 * scale, width: scale, height: scale }, `${friendly(kind)} · ${str(hazard.id)} · ${seconds}s`);
      }
      for (const chef of chefs) {
        const pt = at(chef); if (!pt) continue;
        const respawn = typeof chef.respawning_ms === "number", alpha = respawn ? .45 : 1, held = itemShort(rec(chef.held));
        if (chef.active) p.circle(pt.x, pt.y, .68 * scale, undefined, "#ffcf54", .04 * scale);
        p.circle(pt.x, pt.y, .43 * scale, chef.active ? "#ffcf54" : "#f3eee5", "#25201b", .08 * scale, alpha);
        p.polygon([pt.x - .3 * scale, pt.y - .34 * scale, pt.x - .3 * scale, pt.y - .53 * scale, pt.x - .15 * scale, pt.y - .6 * scale, pt.x, pt.y - .53 * scale, pt.x + .15 * scale, pt.y - .6 * scale, pt.x + .3 * scale, pt.y - .53 * scale, pt.x + .3 * scale, pt.y - .34 * scale], "#ffffff", "#25201b", .04 * scale, alpha);
        p.text(pt.x, pt.y - .06 * scale, String(num(chef.id) + 1), Math.max(6, .28 * scale), "#29221a", true, "center");
        if (held) { p.circle(pt.x + .43 * scale, pt.y - .43 * scale, .25 * scale, "#89d2a8", "#17281e", .04 * scale); p.text(pt.x + .43 * scale, pt.y - .52 * scale, held, Math.max(4, .18 * scale), "#2b2418", true, "center"); }
        if (respawn) p.text(pt.x, pt.y + .55 * scale, `${Math.ceil(num(chef.respawning_ms) / 1000)}s`, Math.max(6, .24 * scale), "#ff8a65", true, "center");
        p.hit({ x: pt.x - .5 * scale, y: pt.y - .5 * scale, width: scale, height: scale }, `Chef ${num(chef.id) + 1} · ${held || "hands free"}${respawn ? ` · respawning ${num(chef.respawning_ms)}ms` : ""}`);
      }
    });
  } else p.text(area.x + 12, area.y + 60, "等待厨房快照", 13, "#a79b80");
  const orderX = wide ? area.x + area.width + 16 : 16, orderWidth = view.width - orderX - 16;
  let orderY = wide ? area.y : area.y + area.height + 20;
  if (orders.length) { p.text(orderX, orderY, "ORDER RAIL", 12, "#c8b482", true); orderY += 26; }
  for (const order of orders) {
    const fraction = Math.max(0, Math.min(1, num(order.remaining_ms) / Math.max(1, num(order.deadline_ms) - num(order.opened_ms))));
    p.text(orderX, orderY, str(order.id).toUpperCase(), 10, "#92836a"); orderY += 17;
    orderY = p.paragraph(orderX, orderY, friendly(str(order.recipe)), orderWidth, 13, "#ffe4a1", 2) + 3;
    const ingredients = records(order.requirements).sort((a, b) => str(a.id).localeCompare(str(b.id))).map(item => `${num(item.quantity, 1) > 1 ? `${num(item.quantity)}× ` : ""}${friendly(str(item.id))}`);
    const plan = [...ingredients, ...(str(order.cooking_step, "") ? [friendly(str(order.cooking_step))] : [])].join(" → ");
    if (plan) orderY = p.paragraph(orderX, orderY, plan, orderWidth, 10, "#c0aa7a", 3) + 4;
    p.text(orderX, orderY, `DUE ${clock(Math.max(0, num(order.deadline_ms) - num(shift?.elapsed_ms)))}`, 10, fraction < .25 ? "#ff8d68" : "#adbc98");
    p.bar(orderX, orderY + 18, orderWidth, fraction, fraction < .25 ? "#ff8054" : "#ffcf54", 4); orderY += 38;
  }
  let y = Math.max(orderY, area.y + area.height) + 20;
  const lowerWidth = wide ? (view.width - 48) / 2 : view.width - 32;
  p.text(16, y, "CHEF CONTROL", 12, "#c8b482", true); let chefY = y + 26;
  chefs.forEach((chef, index) => {
    const held = rec(chef.held), respawn = typeof chef.respawning_ms === "number", work = works.find(work => num(work.chef, -1) === num(chef.id));
    p.text(16, chefY, `C${num(chef.id) + 1}  ${respawn ? `RESPAWNING ${Math.ceil(num(chef.respawning_ms) / 1000)}s` : index === num(state.active_chef) ? "ACTIVE" : "STANDBY"}`, 12, index === num(state.active_chef) ? "#ffcf54" : "#c9c2ae", true);
    chefY = p.paragraph(16, chefY + 19, held ? `Holding ${str(held.name)}` : "Hands free", lowerWidth, 11, "#a89c82", 2) + 4;
    if (work) { p.text(16, chefY, str(work.kind).toUpperCase(), 10, "#eac876"); p.bar(16, chefY + 17, lowerWidth, num(work.progress_ms) / Math.max(1, num(work.required_ms)), "#ffcf54"); chefY += 28; }
    else { p.text(16, chefY, str(chef.facing).toUpperCase(), 10, "#8c9c86"); chefY += 20; }
    chefY += 12;
  });
  const eventX = wide ? 32 + lowerWidth : 16; let eventY = wide ? y : chefY + 12;
  const events = records(state.recent_events), alarms = records(state.alarms);
  if (events.length || alarms.length) { p.text(eventX, eventY, "RECENT ACTIVITY", 12, "#c8b482", true); eventY += 26; }
  for (const event of events.slice(0, 6)) { eventY = p.paragraph(eventX, eventY, `${clock(num(event.elapsed_ms))} ${friendly(str(event.kind))}`, lowerWidth, 11, "#d5bd87", 2) + 3; eventY = p.paragraph(eventX, eventY, str(event.message), lowerWidth, 11, "#aca38e", 4) + 12; }
  for (const alarm of alarms.slice(0, 3)) { p.text(eventX, eventY, `ALARM ${str(alarm.id)} · ${clock(num(alarm.due_ms))}`, 11, "#e7b866"); eventY += 24; }
  y = Math.max(chefY, eventY);
  return p.finish(y + 12, `Kitchen，第 ${num(campaign?.level)} 关，${chefs.length} 位厨师，${orders.length} 份订单，得分 ${num(campaign?.score)}`);
};
