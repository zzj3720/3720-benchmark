import { useEffect, useRef, useState } from "react";

import {
  asNumber,
  asRecord,
  asString,
  type GameObserverModule,
  type GameState,
  type ObserverEvent,
} from "../../../observer/web/app/game-observer";
import { COVER_HEIGHT, COVER_WIDTH, exportScale, registerCanvasExport } from "../../../observer/web/app/webgl/export-source";
import type { SausageScene as SausageSceneRuntime } from "./scene";
import { readSceneState } from "./scene-state";

export function SausageState({ state }: { state: GameState }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const runtimeRef = useRef<SausageSceneRuntime | null>(null);
  const latestRef = useRef(state);
  const [renderError, setRenderError] = useState("");
  latestRef.current = state;
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    let cancelled = false;
    let unregister = () => {};
    let runtime: SausageSceneRuntime | null = null;
    import("./scene").then(async ({ SausageScene }) => {
      if (cancelled) return;
      runtime = new SausageScene(canvas);
      runtime.update(readSceneState(latestRef.current), latestRef.current);
      await runtime.initializeOverlay();
      if (cancelled) { runtime.destroy(); return; }
      runtimeRef.current = runtime;
      runtime.update(readSceneState(latestRef.current), latestRef.current);
      unregister = registerCanvasExport(canvas, {
        async createSession(frames, maxWidth, maxHeight, pixelBudget, options) {
          // A cover is the scene alone at cover size, framed on the player, without the HUD.
          const cover = options?.cover === true;
          const width = cover ? COVER_WIDTH : canvas.clientWidth, height = cover ? COVER_HEIGHT : canvas.clientHeight;
          const output = document.createElement("canvas");
          const renderer = new SausageScene(output, { width, height, hud: !cover, resolution: cover ? 1 : exportScale(width, height, frames.length, maxWidth, maxHeight, pixelBudget) });
          if (!cover) try { await renderer.initializeOverlay(); } catch (error) { renderer.destroy(); throw error; }
          let first = true;
          return { capture(frame) {
            renderer.update(readSceneState(frame.state), frame.state);
            if (first && runtimeRef.current && !cover) { renderer.copyViewFrom(runtimeRef.current); first = false; }
            renderer.prepareExportFrame();
            return output;
          }, dispose() { renderer.destroy(); } };
        },
      });
      setRenderError("");
    }).catch(reason => { runtime?.destroy(); if (!cancelled) setRenderError(reason instanceof Error ? reason.message : String(reason)); });
    return () => { cancelled = true; unregister(); runtimeRef.current?.destroy(); runtimeRef.current = null; };
  }, []);
  useEffect(() => {
    try { runtimeRef.current?.update(readSceneState(state), state); }
    catch (reason) { setRenderError(reason instanceof Error ? reason.message : String(reason)); }
  }, [state]);
  return <div className="sausage-state"><div className="sausage-stage">
    <canvas ref={canvasRef} data-replay-capture role="img" aria-label="香肠三维关卡与烤制状态" />
    {renderError && <div className="sausage-render-error" role="alert">WebGL 渲染失败：{renderError}</div>}
    <div className="sausage-view-controls" aria-label="三维视角控制">
      <button type="button" onClick={() => runtimeRef.current?.focusPlayer()}>跟随玩家</button>
      <button type="button" onClick={() => runtimeRef.current?.showOverview()}>完整地图</button>
    </div>
  </div></div>;
}

function describeEvent(event: ObserverEvent) {
  const action = event.action ?? {};
  const type = asString(action.command, asString(event.type, "event"));
  const state = asRecord(event.state);
  const level = asRecord(state?.level);
  if ((event.score_delta ?? 0) > 0) {
    return {
      label: "PUZZLE SOLVED",
      title: asString(level?.title, "完成关卡"),
      detail: `总分增加 ${event.score_delta}，玩家已返回大地图。`,
      tone: "success" as const,
    };
  }
  if (type === "move") {
    const directions = Array.isArray(action.directions)
      ? action.directions.map((value) => asString(value)).join(" → ")
      : asString(action.direction, "move");
    const accepted = event.result?.accepted;
    return {
      label: accepted === false ? "BLOCKED MOVE" : "AGENT MOVE",
      title: directions.toUpperCase(),
      detail: accepted === false ? "该方向未改变权威状态。" : "三维状态已按该指令更新。",
      tone: accepted === false ? "warning" as const : "neutral" as const,
    };
  }
  if (type === "undo") return { label: "UNDO", title: "回退操作", detail: "恢复到之前的权威三维状态。", tone: "warning" as const };
  if (type === "restart") return { label: "RESTART", title: "重置当前关卡", detail: "关卡已恢复到入口状态。", tone: "warning" as const };
  return { label: type.toUpperCase(), title: "状态已记录", detail: "Sidecar 已记录新的权威状态。" };
}

function stateContext(state: GameState) {
  const level = asRecord(state.level);
  if (asString(state.mode, "") === "overworld") {
    const overworld = asRecord(state.overworld);
    const entrances = Array.isArray(overworld?.entrances) ? overworld.entrances.length : 0;
    return `大地图 · ${entrances} 个入口`;
  }
  if (level) return `第 ${asNumber(level.ordinal)} 关 · ${state.exit_ready ? "出口已开启" : "烤制中"}`;
  return null;
}

// Historical frames recorded inside a puzzle omit the large shared overworld
// map; borrow it from the latest authoritative state so overworld replay
// frames still render the full map.
function resolveFrameState(frameState: GameState, latestState: GameState) {
  if (frameState.overworld_map || !latestState.overworld_map) return frameState;
  return { ...frameState, overworld_map: latestState.overworld_map };
}

// A cleared puzzle returns to the overworld in the same step, so the recorded
// state of that step shows the overworld. The last puzzle state already has
// every sausage cooked and the player on the exit; the step only walks out.
function clearedState(before: GameState, event: ObserverEvent) {
  const level = (state?: GameState | null) => asString(asRecord(state?.level)?.id, "");
  return level(event.state) === level(before) ? null : before;
}

export default {
  id: "sausage",
  meta: {
    label: "Stephen’s Sausage Roll",
    short: "SAUSAGE",
    accent: "#ffad57",
  },
  State: SausageState,
  stateContext,
  describeEvent,
  resolveFrameState,
  clearedState,
} satisfies GameObserverModule;
