import { num, rec, str, records, strings, clock } from "../../../observer/web/app/webgl/data";
import { Painter, type SceneBuilder } from "../../../observer/web/app/webgl/scene";

const colors: Record<string, string> = { police: "#719eff", medical: "#ff7189", fire: "#ffbd58" };
const roleColor = (role: string) => colors[role] ?? "#9ebbae";
const short = (value: string, width: number, size = 12) => value.length > Math.floor(width / (size * .62)) ? value.slice(0, Math.max(1, Math.floor(width / (size * .62)) - 1)) + "…" : value;

export const buildOperatorScene: SceneBuilder = (state, view) => {
  const p = new Painter(view.width, "#0a1314"), shift = rec(state.shift), duty = rec(shift?.duty), campaign = rec(state.campaign);
  const calls = records(state.calls), incidents = records(state.incidents), units = records(state.units), alarms = records(state.alarms);
  const activeCalls = calls.filter(call => ["ringing", "active"].includes(str(call.status)));
  const open = incidents.filter(incident => str(incident.status) === "reported"), call = activeCalls.find(call => str(call.status) === "active") ?? activeCalls[0];
  const stats = [
    ["DUTY", duty ? `C${num(duty.chapter)} · D${num(duty.number)}` : str(shift?.status).toUpperCase()],
    ["CITY", str(duty?.city)], ["CLOCK", clock(num(shift?.elapsed_ms))],
    ["SCORE", `${num(campaign?.score).toLocaleString()} / ${num(campaign?.max_score).toLocaleString()}`],
    ["LIVE", `${activeCalls.length} CALL · ${open.length} SCENE`],
  ];
  const statCols = view.width < 520 ? 3 : 5, statWidth = view.width / statCols, header = Math.ceil(stats.length / statCols) * 48;
  p.rect(0, 0, view.width, header, "#11201f");
  stats.forEach(([label, value], i) => { const x = (i % statCols) * statWidth + 12, y = Math.floor(i / statCols) * 48 + 8; p.text(x, y, label, 10, "#829d92"); p.text(x, y + 17, short(value, statWidth - 24, 12), 12, "#d7e8df", true); p.hit({ x, y, width: statWidth - 20, height: 36 }, `${label}: ${value}`); });
  const wide = view.width >= 680, mapWidth = wide ? Math.floor(view.width * .58) - 24 : view.width - 32;
  const map = { x: 16, y: header + 16, width: mapWidth, height: Math.max(260, Math.min(390, mapWidth * .7)) };
  p.panel(map.x, map.y, map.width, map.height, str(duty?.city, "Dispatch"));
  const grid = { x: map.x + 28, y: map.y + 38, width: map.width - 48, height: map.height - 66 };
  for (let i = 0; i <= 16; i++) { const x = grid.x + grid.width * i / 16, y = grid.y + grid.height * i / 16; p.line([x, grid.y, x, grid.y + grid.height], i % 4 ? "#1b302d" : "#30473f"); p.line([grid.x, y, grid.x + grid.width, y], i % 4 ? "#1b302d" : "#30473f"); }
  [-8, -4, 0, 4, 8].forEach(value => { p.text(grid.x + (value + 8) / 16 * grid.width, grid.y + grid.height + 6, String(value), 9, "#6e8a7b", false, "center"); p.text(grid.x - 6, grid.y + (8 - value) / 16 * grid.height - 5, String(value), 9, "#6e8a7b", false, "right"); });
  const position = (location: ReturnType<typeof rec>, offset = 0) => ({ x: grid.x + (Math.max(-8, Math.min(8, num(location?.x) + offset)) + 8) / 16 * grid.width, y: grid.y + (8 - Math.max(-8, Math.min(8, num(location?.y)))) / 16 * grid.height });
  p.clipped(grid, () => {
    for (const incident of open) {
      const at = position(rec(incident.location)), role = str(records(incident.requirements)[0]?.role, "police"), color = roleColor(role);
      p.circle(at.x, at.y, 10, "#162723", color, 2); p.text(at.x, at.y - 6, role[0].toUpperCase(), 10, color, true, "center");
      p.text(at.x + 14, at.y - 6, short(str(incident.title), Math.max(70, grid.x + grid.width - at.x - 18), 10), 10, "#b7ccc1");
      p.hit({ x: at.x - 12, y: at.y - 12, width: 24, height: 24 }, `${str(incident.id)} · ${str(incident.title)} · ${str(incident.status)}`);
    }
    units.forEach((unit, i) => {
      const at = position(rec(unit.location), i % 2 ? .22 : -.22), color = roleColor(str(unit.role));
      p.sprite(`unit:${str(unit.id, String(i))}`, { x: at.x - 7, y: at.y - 7, width: 14, height: 14 }, () => {
        p.rect(at.x - 7, at.y - 7, 14, 14, color, 2, "#e1eae4", 1);
        p.text(at.x, at.y - 5, str(unit.id).replace(/[^0-9]/g, "") || "·", 8, "#091311", true, "center");
      });
      p.hit({ x: at.x - 9, y: at.y - 9, width: 18, height: 18 }, `${str(unit.label)} · ${str(unit.status)}${unit.eta_ms != null ? ` · ETA ${clock(num(unit.eta_ms))}` : ""}`);
    });
  });
  let y = map.y + map.height + 16;
  const sideX = wide ? map.x + map.width + 16 : 16, sideWidth = view.width - sideX - 16;
  const drawUnits = (x: number, top: number, width: number) => {
    p.text(x, top, "RESPONSE UNITS", 12, "#9bb3a5", true); top += 24;
    for (const unit of units) {
      p.rect(x, top + 2, 3, 32, roleColor(str(unit.role)));
      p.text(x + 10, top, short(str(unit.label), width - 10), 12, "#d4e3da", true);
      p.text(x + 10, top + 17, short(`${str(unit.status)}${unit.eta_ms != null ? ` · ${clock(num(unit.eta_ms))}` : ""} · ${str(unit.incident, "Dispatch base")}`, width - 10, 10), 10, "#8da899");
      p.hit({ x, y: top, width, height: 38 }, `${str(unit.label)} · ${str(unit.incident, "Dispatch base")} · ${str(unit.status)}`); top += 46;
    }
    return top;
  };
  if (call) {
    let cy = wide ? map.y : y;
    p.text(sideX, cy, `${str(call.status).toUpperCase()} · ${str(call.caller)}`, 13, "#ff849a", true); cy += 24;
    p.text(sideX, cy, `${call.answer_deadline_ms ? "ANSWER" : "END"} BY ${clock(num(call.answer_deadline_ms ?? call.conversation_deadline_ms))}`, 10, "#92aa9e"); cy += 24;
    const transcript = records(call.transcript);
    for (const line of transcript.slice(-7)) { p.text(sideX, cy, str(line.speaker).toUpperCase(), 10, "#6f9483", true); cy = p.paragraph(sideX, cy + 15, str(line.text), sideWidth, 12, "#cadbd0", 5) + 10; }
    if (!transcript.length) { p.text(sideX, cy, "Incoming call", 12, "#d5b57e"); cy += 26; }
    for (const choice of records(call.choices)) { cy = p.paragraph(sideX, cy, `${str(choice.id)}  ${str(choice.text)}`, sideWidth, 12, "#a9ceba", 4) + 8; }
    y = Math.max(y, cy + 16);
  } else if (wide) y = Math.max(y, drawUnits(sideX, map.y + 4, sideWidth));
  const columns = wide ? (call ? 3 : 2) : 1, gap = 20, columnWidth = (view.width - 32 - gap * (columns - 1)) / columns;
  const detailTop = y + 4;
  p.text(16, detailTop, "INCIDENT SCENES", 12, "#9bb3a5", true);
  let incidentY = detailTop + 24;
  // One column stacks everything, so narrow views keep only the latest entries.
  const visible = incidents.filter(incident => str(incident.status) !== "hidden").slice(wide ? -8 : -4).reverse();
  for (const incident of visible) {
    p.text(16, incidentY, short(str(incident.title), columnWidth, 12), 12, "#d4e3da", true); incidentY += 18;
    p.text(16, incidentY, str(incident.status).toUpperCase(), 10, "#83a694"); incidentY += 18;
    const requirements = records(incident.requirements), barWidth = (columnWidth - 8 * Math.max(0, requirements.length - 1)) / Math.max(1, requirements.length);
    requirements.forEach((req, i) => { const x = 16 + i * (barWidth + 8); p.text(x, incidentY, `${str(req.role).slice(0, 3).toUpperCase()} ${clock(num(req.remaining_work_ms))}`, 9, roleColor(str(req.role))); p.bar(x, incidentY + 15, barWidth, 1 - num(req.remaining_work_ms) / Math.max(1, num(req.total_work_ms)), roleColor(str(req.role)), 3); });
    if (requirements.length) incidentY += 28;
    for (const element of records(incident.elements).filter(item => item.active).slice(0, 6)) {
      const text = `${str(element.kind).toUpperCase()} · ${str(element.label)}${element.health_milli != null ? ` · ${Math.max(0, num(element.health_milli) / 1000).toFixed(0)}%` : ""}${element.remaining_timer_ms != null ? ` · T−${clock(num(element.remaining_timer_ms))}` : ""}${element.bill != null ? ` · BILL ${num(element.bill).toLocaleString()}` : ""}`;
      incidentY = p.paragraph(16, incidentY, text, columnWidth, 10, "#96b1a2", 3) + 4;
    }
    p.line([16, incidentY + 3, 16 + columnWidth, incidentY + 3], "#263b32"); incidentY += 18;
  }
  if (!visible.length) { p.text(16, incidentY, "No reported incidents", 11, "#718d7d"); incidentY += 28; }
  let unitBottom = detailTop;
  if (call || !wide) unitBottom = drawUnits(wide ? 16 + columnWidth + gap : 16, wide ? detailTop : incidentY + 16, columnWidth);
  const memoryX = wide ? 16 + (columns - 1) * (columnWidth + gap) : 16;
  let memoryY = wide ? detailTop : Math.max(incidentY, unitBottom) + 16;
  const reports = calls.flatMap(call => strings(call.after_action_report)).slice(wide ? -6 : -3).reverse();
  const facts = calls.flatMap(call => Object.entries(rec(call.facts) ?? {})).slice(wide ? -6 : -3).reverse();
  const pending = alarms.filter(alarm => ["pending", "due"].includes(str(alarm.status))).slice(-12);
  if (reports.length || facts.length || pending.length) {
    p.text(memoryX, memoryY, "MEMORY & AFTER ACTION", 12, "#9bb3a5", true); memoryY += 26;
    for (const report of reports) memoryY = p.paragraph(memoryX, memoryY, `AAR · ${report}`, columnWidth, 11, "#afc9b9", 6) + 12;
    for (const [key, value] of facts) memoryY = p.paragraph(memoryX, memoryY, `${key.toUpperCase()} · ${str(value)}`, columnWidth, 11, "#93b5a0", 4) + 10;
    for (const alarm of pending) memoryY = p.paragraph(memoryX, memoryY, `ALARM ${clock(num(alarm.due_ms))} · ${str(alarm.note, "Agent reminder")}`, columnWidth, 11, "#dfb66f", 4) + 10;
  }
  return p.finish(Math.max(incidentY, unitBottom, memoryY) + 16, `Operator，${activeCalls.length} 通来电，${open.length} 个事件，${units.length} 个单位，得分 ${num(campaign?.score)}`, `operator:${str(duty?.city)}`);
};
