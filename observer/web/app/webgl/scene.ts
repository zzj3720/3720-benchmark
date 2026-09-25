import type { GameState } from "../game-observer";

export type Rect = { x: number; y: number; width: number; height: number };
export type Viewport = { width: number; height: number; compact: boolean };
type Common = { clip?: Rect; alpha?: number };
export type DrawCommand = Common & (
  | { kind: "rect"; x: number; y: number; width: number; height: number; color?: string; radius?: number; stroke?: string; strokeWidth?: number }
  | { kind: "circle"; x: number; y: number; radius: number; color?: string; stroke?: string; strokeWidth?: number }
  | { kind: "line"; points: number[]; color: string; width: number }
  | { kind: "polygon"; points: number[]; color: string; stroke?: string; strokeWidth?: number }
  | { kind: "text"; x: number; y: number; text: string; color: string; size: number; bold?: boolean; align?: "left" | "center" | "right" }
);
export type GameScene = { width: number; height: number; background: string; description: string; commands: DrawCommand[]; hits: { bounds: Rect; text: string }[] };
export type SceneBuilder = (state: GameState, viewport: Viewport, previous?: GameState | null) => GameScene;

export function intersect(a: Rect, b: Rect): Rect {
  const x = Math.max(a.x, b.x), y = Math.max(a.y, b.y);
  return { x, y, width: Math.max(0, Math.min(a.x + a.width, b.x + b.width) - x), height: Math.max(0, Math.min(a.y + a.height, b.y + b.height) - y) };
}

export class Painter {
  readonly commands: DrawCommand[] = [];
  readonly hits: { bounds: Rect; text: string }[] = [];
  private clip?: Rect;
  constructor(readonly width: number, readonly background = "#0d1515") {}
  private add(command: DrawCommand) {
    if (this.commands.length >= 50_000) throw new Error("场景超出绘制预算");
    if (this.clip && (!this.clip.width || !this.clip.height)) return;
    this.commands.push({ ...command, clip: this.clip });
  }
  hit(bounds: Rect, text: string) { this.hits.push({ bounds: this.clip ? intersect(this.clip, bounds) : bounds, text }); }
  clipped(rect: Rect, draw: () => void) {
    const before = this.clip;
    this.clip = before ? intersect(before, rect) : rect;
    try { draw(); } finally { this.clip = before; }
  }
  rect(x: number, y: number, width: number, height: number, color?: string, radius = 0, stroke?: string, strokeWidth = 1, alpha = 1) {
    if (width <= 0 || height <= 0) return;
    if (this.clip) { const visible = intersect(this.clip, { x, y, width, height }); if (!visible.width || !visible.height) return; }
    this.add({ kind: "rect", x, y, width, height, color, radius, stroke, strokeWidth, alpha });
  }
  circle(x: number, y: number, radius: number, color?: string, stroke?: string, strokeWidth = 1, alpha = 1) {
    if (radius > 0) this.add({ kind: "circle", x, y, radius, color, stroke, strokeWidth, alpha });
  }
  line(points: number[], color: string, width = 1, alpha = 1) { this.add({ kind: "line", points, color, width, alpha }); }
  polygon(points: number[], color: string, stroke?: string, strokeWidth = 1, alpha = 1) { this.add({ kind: "polygon", points, color, stroke, strokeWidth, alpha }); }
  text(x: number, y: number, text: string, size = 12, color = "#dce6e1", bold = false, align: "left" | "center" | "right" = "left") {
    if (text) this.add({ kind: "text", x, y, text, size, color, bold, align });
  }
  paragraph(x: number, y: number, text: string, width: number, size = 12, color = "#a4b5ad", maxLines = 5) {
    const lines = wrap(text, Math.max(4, Math.floor(width / (size * .62))), maxLines);
    for (const line of lines) { this.text(x, y, line, size, color); y += Math.ceil(size * 1.45); }
    return y;
  }
  panel(x: number, y: number, width: number, height: number, title?: string) {
    this.rect(x, y, width, height, "#101b1b", 4, "#293c38");
    if (title) this.text(x + 12, y + 10, title, 12, "#dbe5df", true);
  }
  bar(x: number, y: number, width: number, fraction: number, color: string, height = 4) {
    this.rect(x, y, width, height, "#293731", 2);
    this.rect(x, y, width * Math.max(0, Math.min(1, fraction)), height, color, 2);
  }
  finish(height: number, description: string): GameScene { return { width: this.width, height: Math.max(1, Math.ceil(height)), background: this.background, description, commands: this.commands, hits: this.hits }; }
}

export function wrap(text: string, columns: number, maxLines = 5) {
  const lines: string[] = []; let line = "", units = 0;
  const input = [...text.replace(/\s+/g, " ").trim()];
  for (let i = 0; i < input.length; i++) {
    const char = input[i], size = char.charCodeAt(0) > 255 ? 2 : 1;
    if (units + size > columns) {
      if (lines.length === maxLines - 1) { lines.push(line.slice(0, -1) + "…"); return lines; }
      lines.push(line); line = ""; units = 0;
    }
    line += char; units += size;
  }
  if (line) lines.push(line);
  return lines;
}

export function rgb(value: unknown, fallback: string) {
  if (!Array.isArray(value) || value.length !== 3) return fallback;
  return "#" + value.map(channel => Math.round(Math.max(0, Math.min(255, Number(channel) || 0))).toString(16).padStart(2, "0")).join("");
}
export function mix(a: string, b: string, amount: number) {
  const one = Number.parseInt(a.slice(1), 16), two = Number.parseInt(b.slice(1), 16);
  return "#" + [16, 8, 0].map(shift => Math.round(((one >> shift) & 255) * amount + ((two >> shift) & 255) * (1 - amount)).toString(16).padStart(2, "0")).join("");
}
