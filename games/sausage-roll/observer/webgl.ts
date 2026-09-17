import type { GameState } from "../../../observer-platform/app/game-observer";
import { Painter } from "../../../observer-platform/app/webgl/scene";
import { rec, str, num } from "../../../observer-platform/app/webgl/data";
import type { SausageSceneState } from "./scene-state";

export function sausageHud(state: GameState, scene: SausageSceneState, width: number, height: number) {
  const p = new Painter(width), map = scene.mode === "overworld";
  const level = rec(map ? state.overworld : state.level) ?? {};
  p.rect(0, 0, width, 66, "#0d181b");
  p.paragraph(12, 9, `${map ? "大地图" : `关卡 ${num(level.ordinal)}`} · ${str(level.title, "Sausage Roll")}`, width - 24, 14, "#f1e4cc", 1);
  p.text(12, 36, `移动 ${num(level.actions)}  ·  叉 ${scene.entities.some(e => e.kind === "fork") ? "落地" : "持有"}  ·  ${map ? `开放 ${scene.entrances.filter(e => e.status === "available").length}` : `出口 ${state.exit_ready ? "开启" : "锁定"}`}`, width < 400 ? 10 : 12, "#adc0b9");
  const sausages = scene.entities.filter(e => e.kind === "sausage");
  const columns = Math.max(1, Math.floor((width - 24) / 158));
  const footer = 28 + Math.ceil(sausages.length / columns) * 24;
  p.rect(0, height - footer, width, footer, "#0d181b");
  const colors = ["#e98468", "#a95d38", "#75412e", "#271d1a"];
  p.text(12, height - footer + 7, "生 · 横烤 · 纵烤 · 焦黑", 11, "#c6b9a5");
  sausages.forEach((sausage, i) => {
    const x = 12 + (i % columns) * 158, y = height - footer + 28 + Math.floor(i / columns) * 24;
    p.text(x, y, `S${sausage.id}`, 11);
    (sausage.cookedFaces ?? [0, 0, 0, 0]).forEach((face, j) => {
      p.rect(x + 34 + j * 27, y - 1, 24, 18, colors[Math.max(0, Math.min(3, face))], 2);
      p.text(x + 38 + j * 27, y + 1, `${j + 1}`, 10, "#fff2de");
    });
  });
  return { scene: p.finish(height, "香肠关卡与四面烤制状态"), footer };
}
