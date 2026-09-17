"use client";
import { useEffect, useRef, useState } from "react";
import type { GameState } from "../game-observer";
import type { GameScene, SceneBuilder, Viewport } from "./scene";
import type { WebGLSurface } from "./runtime";
import { exportScale, registerCanvasExport } from "./export-source";

export function GameCanvas({ state, previousState, build, label }: { state: GameState; previousState?: GameState | null; build: SceneBuilder; label: string }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const rendererRef = useRef<WebGLSurface | null>(null);
  const latest = useRef({ state, previousState, build });
  const redraw = useRef<() => void>(() => {});
  const [error, setError] = useState("");
  const visibleScene = useRef<GameScene | null>(null);
  const [tooltip, setTooltip] = useState<{ text: string; x: number; y: number } | null>(null);
  useEffect(() => { latest.current = { state, previousState, build }; redraw.current(); }, [state, previousState, build]);
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    let disposed = false;
    let unregister = () => {};
    const viewport = (): Viewport => ({ width: Math.max(240, canvas.parentElement?.clientWidth ?? 800), height: window.innerHeight, compact: window.innerWidth <= 820 });
    const render = () => {
      if (disposed || !rendererRef.current) return;
      try {
        const scene = latest.current.build(latest.current.state, viewport(), latest.current.previousState);
        visibleScene.current = scene;
        canvas.style.height = `${scene.height}px`;
        rendererRef.current.render(scene);
        canvas.setAttribute("aria-label", scene.description || label);
        setError("");
      } catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
    };
    redraw.current = render;
    const observer = new ResizeObserver(render);
    if (canvas.parentElement) observer.observe(canvas.parentElement);
    window.addEventListener("resize", render);
    const lost = (event: Event) => { event.preventDefault(); canvas.dataset.ready = "false"; setError("显卡上下文丢失，正在恢复…"); };
    const restored = () => { window.requestAnimationFrame(render); };
    canvas.addEventListener("webglcontextlost", lost);
    canvas.addEventListener("webglcontextrestored", restored);
    import("./runtime").then(async ({ WebGLSurface }) => {
      const surface = await WebGLSurface.create(canvas, viewport().width, 300, Math.min(window.devicePixelRatio, 2));
      if (disposed) { surface.destroy(); return; }
      rendererRef.current = surface;
      unregister = registerCanvasExport(canvas, {
        async createSession(frames, maxWidth, maxHeight, pixelBudget) {
          const view = viewport(), builder = latest.current.build;
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
    return () => { disposed = true; canvas.removeEventListener("webglcontextlost", lost); canvas.removeEventListener("webglcontextrestored", restored); observer.disconnect(); window.removeEventListener("resize", render); unregister(); rendererRef.current?.destroy(); rendererRef.current = null; redraw.current = () => {}; };
  }, [label]);
  return <div className="game-webgl-surface" data-replay-capture><canvas ref={canvasRef} role="img" aria-label={label} onPointerLeave={() => setTooltip(null)} onPointerMove={event => {
    const scene = visibleScene.current, box = event.currentTarget.getBoundingClientRect();
    if (!scene) return;
    const x = (event.clientX - box.left) * scene.width / box.width, y = (event.clientY - box.top) * scene.height / box.height;
    const hit = scene.hits.findLast(({ bounds }) => x >= bounds.x && x <= bounds.x + bounds.width && y >= bounds.y && y <= bounds.y + bounds.height);
    setTooltip(previous => hit ? previous?.text === hit.text ? previous : { text: hit.text, x: Math.max(4, Math.min(x + 12, scene.width - 264)), y: Math.max(4, Math.min(y + 12, scene.height - 76)) } : null);
  }} />{tooltip && <div className="game-canvas-tooltip" role="tooltip" style={{ left: tooltip.x, top: tooltip.y }}>{tooltip.text}</div>}{error && <div className="game-render-error" role="alert">WebGL 渲染失败：{error}</div>}</div>;
}
