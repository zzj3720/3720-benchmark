"use client";
import { useEffect, useRef, useState } from "react";
import type { GameState } from "../game-observer";
import type { GameScene, Rect, SceneBuilder, Viewport } from "./scene";
import type { WebGLSurface } from "./runtime";
import { COVER_HEIGHT, COVER_WIDTH, exportScale, registerCanvasExport } from "./export-source";
import { easeInOut, lerpRect, pairSprites, placeSprites, sameRect } from "./motion";

/** Longest tween; faster replays shorten it to fit between frames. */
const MOTION_MS = 260;

export function GameCanvas({ state, previousState, build, label }: { state: GameState; previousState?: GameState | null; build: SceneBuilder; label: string }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const rendererRef = useRef<WebGLSurface | null>(null);
  const latest = useRef({ state, previousState, build });
  const redraw = useRef<(animate?: boolean) => void>(() => {});
  const [error, setError] = useState("");
  const visibleScene = useRef<GameScene | null>(null);
  const [tooltip, setTooltip] = useState<{ text: string; x: number; y: number } | null>(null);
  useEffect(() => { latest.current = { state, previousState, build }; redraw.current(true); }, [state, previousState, build]);
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    let disposed = false;
    let unregister = () => {};
    const viewport = (): Viewport => ({ width: Math.max(240, canvas.parentElement?.clientWidth ?? 800), height: window.innerHeight, compact: window.innerWidth <= 820 });
    // What is on screen: the scene and where each of its sprites is drawn,
    // so a new state tweens from there even when it interrupts a tween.
    let shown: { scene: GameScene; placed: Rect[] } | null = null;
    let frame = 0, lastState = 0;
    const reduced = window.matchMedia?.("(prefers-reduced-motion: reduce)");
    const draw = (scene: GameScene, placed: Rect[], alpha?: number[]) => {
      rendererRef.current?.render(alpha ? placeSprites(scene, placed, alpha) : scene);
      shown = { scene, placed };
    };
    const render = (animate = false) => {
      if (disposed || !rendererRef.current) return;
      try {
        const scene = latest.current.build(latest.current.state, viewport(), latest.current.previousState);
        visibleScene.current = scene;
        canvas.style.height = `${scene.height}px`;
        canvas.setAttribute("aria-label", scene.description || label);
        window.cancelAnimationFrame(frame);
        const now = performance.now(), interval = now - lastState;
        if (animate) lastState = now;
        const target = scene.sprites.map(sprite => sprite.bounds);
        const before = shown as { scene: GameScene; placed: Rect[] } | null;
        const starts = animate && before && !reduced?.matches && scene.continuity && before.scene.continuity === scene.continuity && before.scene.width === scene.width
          ? pairSprites(before.scene.sprites.map((sprite, index) => ({ key: sprite.key, bounds: before.placed[index] ?? sprite.bounds })), scene.sprites)
          : null;
        if (!starts || starts.every((start, index) => start && sameRect(start, target[index]))) { draw(scene, target); setError(""); return; }
        const duration = Math.max(90, Math.min(MOTION_MS, interval * .75));
        const step = () => {
          if (disposed) return;
          const t = Math.min(1, (performance.now() - now) / duration), e = easeInOut(t);
          draw(scene, target.map((to, index) => starts[index] ? lerpRect(starts[index]!, to, e) : to), starts.map(start => start ? 1 : e));
          if (t < 1) frame = window.requestAnimationFrame(step);
          else draw(scene, target);
        };
        step();
        setError("");
      } catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
    };
    redraw.current = render;
    const observer = new ResizeObserver(() => render());
    if (canvas.parentElement) observer.observe(canvas.parentElement);
    const resize = () => render();
    window.addEventListener("resize", resize);
    const lost = (event: Event) => { event.preventDefault(); canvas.dataset.ready = "false"; setError("显卡上下文丢失，正在恢复…"); };
    const restored = () => { window.requestAnimationFrame(() => render()); };
    canvas.addEventListener("webglcontextlost", lost);
    canvas.addEventListener("webglcontextrestored", restored);
    import("./runtime").then(async ({ WebGLSurface }) => {
      const surface = await WebGLSurface.create(canvas, viewport().width, 300, Math.min(window.devicePixelRatio, 2));
      if (disposed) { surface.destroy(); return; }
      rendererRef.current = surface;
      unregister = registerCanvasExport(canvas, {
        async createSession(frames, maxWidth, maxHeight, pixelBudget, options) {
          const view = options?.cover ? { width: COVER_WIDTH, height: COVER_HEIGHT, compact: false, cover: true } : viewport(), builder = latest.current.build;
          let height = 1;
          for (const frame of frames) height = Math.max(height, builder(frame.state, view, frame.previous).height);
          const scale = exportScale(view.width, height, frames.length, maxWidth, maxHeight, pixelBudget);
          const output = document.createElement("canvas");
          const renderer = await WebGLSurface.create(output, view.width, height, scale);
          return { capture(frame) { renderer.render(builder(frame.state, view, frame.previous), { width: view.width, height }); return output; }, dispose() { renderer.destroy(); } };
        },
      });
      render();
    }).catch(reason => { if (!disposed) setError(reason instanceof Error ? reason.message : String(reason)); });
    return () => { disposed = true; canvas.removeEventListener("webglcontextlost", lost); canvas.removeEventListener("webglcontextrestored", restored); observer.disconnect(); window.cancelAnimationFrame(frame); window.removeEventListener("resize", resize); unregister(); rendererRef.current?.destroy(); rendererRef.current = null; redraw.current = () => {}; };
  }, [label]);
  return <div className="game-webgl-surface" data-replay-capture><canvas ref={canvasRef} role="img" aria-label={label} onPointerLeave={() => setTooltip(null)} onPointerMove={event => {
    const scene = visibleScene.current, box = event.currentTarget.getBoundingClientRect();
    if (!scene) return;
    const x = (event.clientX - box.left) * scene.width / box.width, y = (event.clientY - box.top) * scene.height / box.height;
    const hit = scene.hits.findLast(({ bounds }) => x >= bounds.x && x <= bounds.x + bounds.width && y >= bounds.y && y <= bounds.y + bounds.height);
    setTooltip(previous => hit ? previous?.text === hit.text ? previous : { text: hit.text, x: Math.max(4, Math.min(x + 12, scene.width - 264)), y: Math.max(4, Math.min(y + 12, scene.height - 76)) } : null);
  }} />{tooltip && <div className="game-canvas-tooltip" role="tooltip" style={{ left: tooltip.x, top: tooltip.y }}>{tooltip.text}</div>}{error && <div className="game-render-error" role="alert">WebGL 渲染失败：{error}</div>}</div>;
}
