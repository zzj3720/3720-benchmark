import type { GameState } from "../game-observer";

export type ExportFrame = { state: GameState; previous?: GameState | null };
export type CanvasExportSession = { capture(frame: ExportFrame): HTMLCanvasElement; dispose(): void };
/** `cover` asks for a 640x400 still of the board alone: no headers, side panels or overlays. */
export type ExportOptions = { cover?: boolean };
export const COVER_WIDTH = 640, COVER_HEIGHT = 400;
export type CanvasExportSource = { createSession(frames: ExportFrame[], maxWidth: number, maxHeight: number, pixelBudget: number, options?: ExportOptions): Promise<CanvasExportSession> };
const sources = new WeakMap<HTMLCanvasElement, CanvasExportSource>();
export function registerCanvasExport(canvas: HTMLCanvasElement, source: CanvasExportSource) {
  sources.set(canvas, source);
  return () => sources.delete(canvas);
}
export function canvasExportSource(element: HTMLElement): CanvasExportSource | undefined {
  const canvas = element instanceof HTMLCanvasElement ? element : element.querySelector("canvas");
  return canvas ? sources.get(canvas) : undefined;
}
export function exportScale(width: number, height: number, frames: number, maxWidth: number, maxHeight: number, pixelBudget: number) {
  return Math.min(1, maxWidth / Math.max(1, width), maxHeight / Math.max(1, height), Math.sqrt(pixelBudget / Math.max(1, frames * width * height)));
}
