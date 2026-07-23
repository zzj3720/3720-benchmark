import { useEffect, useMemo, useRef, useState } from "react";

import {
  asNumber,
  asRecord,
  asString,
  type GameObserverModule,
  type GameState,
  type ObserverEvent,
} from "../../../observer-platform/app/game-observer";
import type { SausageScene as SausageSceneRuntime } from "./scene";
import { readSceneState } from "./scene-state";

const TILE_SET_NAMES = ["GREEN", "SAND", "SNOW", "SWAMP", "TEMPLE"];
const COOK_LABELS = ["RAW", "COOK ⟂", "COOK ∥", "BURNT"];

export function SausageState({ state }: { state: GameState }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const runtimeRef = useRef<SausageSceneRuntime | null>(null);
  const latestRef = useRef(readSceneState(state));
  const [renderError, setRenderError] = useState("");
  const scene = useMemo(() => readSceneState(state), [state]);
  latestRef.current = scene;
  const level = asRecord(state.level);
  const overworld = asRecord(state.overworld);
  const mapMode = scene.mode === "overworld";
  const sausages = scene.entities.filter((entity) => entity.kind === "sausage");

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    let cancelled = false;
    import("./scene")
      .then(({ SausageScene }) => {
        if (cancelled) return;
        const runtime = new SausageScene(canvas);
        runtimeRef.current = runtime;
        runtime.update(latestRef.current);
        setRenderError("");
      })
      .catch((reason) => setRenderError(reason instanceof Error ? reason.message : String(reason)));
    return () => {
      cancelled = true;
      runtimeRef.current?.destroy();
      runtimeRef.current = null;
    };
  }, []);

  useEffect(() => {
    try {
      runtimeRef.current?.update(scene);
      setRenderError("");
    } catch (reason) {
      setRenderError(reason instanceof Error ? reason.message : String(reason));
    }
  }, [scene]);

  return (
    <div className="sausage-state">
      <header className="sausage-scene-header">
        <div>
          <span>{mapMode ? "OVERWORLD" : `PUZZLE ${asNumber(level?.ordinal) || "—"}`}</span>
          <strong>{mapMode ? `${asString(overworld?.title, "Land's End")} · ${scene.entrances.length} entrances` : asString(level?.title, "Campaign complete")}</strong>
        </div>
        <dl>
          <div><dt>MOVE</dt><dd>{asNumber(mapMode ? overworld?.actions : level?.actions)}</dd></div>
          <div><dt>HEIGHT</dt><dd>{scene.tiles.length ? Math.max(...scene.tiles.map((tile) => tile.pos.z)) - Math.min(...scene.tiles.map((tile) => tile.pos.z)) + 1 : 0}</dd></div>
          <div><dt>FORK</dt><dd>{scene.entities.some((entity) => entity.kind === "fork") ? "LOOSE" : "HELD"}</dd></div>
          {mapMode
            ? <div className="ready"><dt>OPEN</dt><dd>{scene.entrances.filter((entrance) => entrance.status === "available").length}</dd></div>
            : <div className={state.exit_ready ? "ready" : "locked"}><dt>EXIT</dt><dd>{state.exit_ready ? "READY" : "LOCKED"}</dd></div>}
        </dl>
      </header>
      <div className="sausage-stage" data-tile-set={scene.tileSet}>
        <canvas ref={canvasRef} aria-label={`${asString(level?.title, "Sausage Roll")} 的三维关卡状态`} />
        {renderError ? <div className="sausage-render-error" role="alert">3D renderer unavailable: {renderError}</div> : null}
        <span className="sausage-environment">{mapMode ? "LAND'S END / ALL ENTRANCES · ARROWS SHOW FACING" : `${TILE_SET_NAMES[scene.tileSet] ?? "GREEN"} / CLEAN GEOMETRY`}</span>
        <div className="sausage-view-controls" aria-label="三维视角控制">
          <button type="button" onClick={() => runtimeRef.current?.focusPlayer()}>FOLLOW PLAYER</button>
          <button type="button" onClick={() => runtimeRef.current?.showOverview()}>FULL MAP</button>
        </div>
        <span className="sausage-view-hint">DRAG ORBIT · SHIFT+DRAG PAN · WHEEL ZOOM</span>
      </div>
      <footer className="sausage-legend">
        <div className="cook-key" aria-label="烤制状态图例">
          {COOK_LABELS.map((label, index) => <span key={label}><i data-cook={index} />{label}</span>)}
        </div>
        <div className="sausage-face-list" aria-label="每根香肠的四面状态">
          {sausages.map((sausage) => (
            <span key={sausage.id}>
              <b>S{sausage.id}</b>
              {(sausage.cookedFaces ?? [0, 0, 0, 0]).map((face, index) => (
                <i data-cook={Math.max(0, Math.min(3, face))} key={index}>F{index + 1}</i>
              ))}
            </span>
          ))}
        </div>
      </footer>
    </div>
  );
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
} satisfies GameObserverModule;
