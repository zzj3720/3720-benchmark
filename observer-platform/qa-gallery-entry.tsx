import { useEffect, useMemo, useState } from "react";
import { createRoot, type Root } from "react-dom/client";

import { ParaboxState } from "../games/parabox-intro/observer";
import { SausageState } from "../games/sausage-roll/observer";
import { OperatorState } from "../games/emergency-operator/observer";
import KitchenState from "../games/kitchen-terminal/observer";
import type { GameState } from "./app/game-observer";
import "./app/globals.css";
import "./qa-gallery.css";

type ParaboxFeatures = {
  depth: number;
  rectangular: boolean;
  rectangular_spaces: number;
  flipped: boolean;
  visible_spaces: number;
  nested_boxes: number;
  boundary_openings: number;
};
type SausageFeatures = {
  tile_set: number;
  tiles: number;
  height_span: number;
  sausages: number;
  cooked_faces: number;
  grills: number;
  ladders: number;
  detached_fork: boolean;
  exit_ready: boolean;
};
type Sample = {
  reference: string;
  title: string;
  area: string;
  step: number;
  total_steps: number;
  path?: string;
  features: ParaboxFeatures | SausageFeatures | Record<string, never>;
  state: GameState;
};
type Game = "parabox" | "sausage" | "operator" | "kitchen";

const PAGE_SIZE = 8;

function selectedGame(): Game {
  const game = new URLSearchParams(window.location.search).get("game");
  return game === "sausage" || game === "operator" || game === "kitchen" ? game : "parabox";
}

function Gallery() {
  const [game, setGame] = useState<Game>(selectedGame);
  const [samples, setSamples] = useState<Sample[]>([]);
  const [error, setError] = useState("");
  const [page, setPage] = useState(0);
  useEffect(() => {
    const file = game === "sausage"
      ? "/sausage-render-qa-64.json"
      : game === "operator"
        ? "/operator-render-qa.json"
        : game === "kitchen"
          ? "/kitchen-render-qa.json"
        : "/parabox-render-qa-64.json";
    fetch(file, { cache: "no-store" })
      .then((response) => {
        if (!response.ok) throw new Error(`HTTP ${response.status}`);
        return response.json();
      })
      .then((payload) => setSamples((payload as { samples: Sample[] }).samples))
      .catch((reason) => setError(String(reason)));
  }, [game]);
  const visible = useMemo(
    () => samples.slice(page * PAGE_SIZE, (page + 1) * PAGE_SIZE),
    [page, samples],
  );
  const pageCount = Math.max(1, Math.ceil(samples.length / PAGE_SIZE));

  function choose(next: Game) {
    window.history.replaceState(null, "", `?game=${next}`);
    setSamples([]);
    setError("");
    setPage(0);
    setGame(next);
  }

  return (
    <main className={`qa-page ${game}-qa`}>
      <header className="qa-header">
        <div>
          <span>{game === "sausage" ? "SAUSAGE 3D RENDER QA" : game === "operator" ? "OPERATOR LIVE CONSOLE QA" : game === "kitchen" ? "KITCHEN FLOOR QA" : "PARABOX RENDER QA"}</span>
          <h1>{game === "sausage" ? "64 个真实三维状态" : game === "operator" ? "真实时间调度状态" : game === "kitchen" ? "8 个原版代表厨房" : "64 个真实递归状态"}</h1>
        </div>
        <div className="qa-actions">
          <nav aria-label="选择渲染检查集">
            <button className={game === "parabox" ? "active" : ""} onClick={() => choose("parabox")}>PARABOX</button>
            <button className={game === "sausage" ? "active" : ""} onClick={() => choose("sausage")}>SAUSAGE 3D</button>
            <button className={game === "operator" ? "active" : ""} onClick={() => choose("operator")}>OPERATOR</button>
            <button className={game === "kitchen" ? "active" : ""} onClick={() => choose("kitchen")}>KITCHEN</button>
          </nav>
          <p>{game === "sausage" ? "86 关完整 walkthrough 分层抽样 · 每页 8 个 WebGL 场景" : game === "operator" ? "真实 campaign 入口生成 · 覆盖来电、分支、并发报告、场景实体和派遣" : game === "kitchen" ? "原版 campaign 数据生成 · 覆盖基础、烹饪、传送带、移动台、开关、灾害与最终关" : "完整 walkthrough 中分层抽样 · 每格标注关卡、步数、容器路径和状态特征"}</p>
        </div>
      </header>
      {error ? (
        <p className="qa-error">
          加载失败：{error}。请在 observer-platform 运行 `vp run {game === "sausage" ? "qa:sausage:data" : game === "operator" ? "qa:operator:data" : game === "kitchen" ? "qa:kitchen:data" : "qa:parabox:data"}`。
        </p>
      ) : null}
      {samples.length > PAGE_SIZE ? (
        <nav className="qa-pagination" aria-label="渲染 QA 分页">
          <button disabled={page === 0} onClick={() => setPage((value) => Math.max(0, value - 1))}>← PREV</button>
          <span>PAGE {page + 1} / {pageCount} · SAMPLES {page * PAGE_SIZE + 1}–{Math.min((page + 1) * PAGE_SIZE, samples.length || PAGE_SIZE)}</span>
          <button disabled={page + 1 >= pageCount} onClick={() => setPage((value) => Math.min(pageCount - 1, value + 1))}>NEXT →</button>
        </nav>
      ) : null}
      <section className="qa-gallery">
        {visible.map((sample, visibleIndex) => {
          const index = page * PAGE_SIZE + visibleIndex;
          return (
            <article className="qa-card" key={`${sample.reference}:${sample.step}`}>
              <header>
                <strong>{String(index + 1).padStart(2, "0")} · {sample.reference}</strong>
                <span>{sample.title}</span>
                <small>{game === "operator" || game === "kitchen" ? `TIME ${clockLabel(sample.step)} · ${sample.area.toUpperCase()}` : `STEP ${sample.step}/${sample.total_steps} · ${game === "sausage" ? sample.area.toUpperCase() : `DEPTH ${(sample.features as ParaboxFeatures).depth}`}`}</small>
                {sample.path ? <small>{sample.path}</small> : null}
              </header>
              <div className="qa-render">
                {game === "sausage" ? <SausageState state={sample.state} /> : game === "operator" ? <OperatorState state={sample.state} /> : game === "kitchen" ? <KitchenState.State state={sample.state} /> : <ParaboxState state={sample.state} />}
              </div>
              {game === "sausage"
                ? <SausageFooter features={sample.features as SausageFeatures} />
                : game === "operator"
                  ? <OperatorFooter state={sample.state} />
                  : game === "kitchen"
                    ? <KitchenFooter state={sample.state} />
                  : <ParaboxFooter sample={sample} />}
            </article>
          );
        })}
      </section>
    </main>
  );
}

function KitchenFooter({ state }: { state: GameState }) {
  const map = state.map && typeof state.map === "object" && !Array.isArray(state.map)
    ? state.map
    : {};
  const cells = Array.isArray(map.walkable) ? map.walkable.length : 0;
  const objects = Array.isArray(map.objects) ? map.objects.length : 0;
  const systems = Array.isArray(map.systems) ? map.systems.length : 0;
  return (
    <footer>
      <span>{cells} FLOOR CELLS</span>
      <span>{objects} OBJECTS</span>
      <span>{systems} SYSTEMS</span>
      <span>2 CHEFS</span>
      <span>REAL CLOCK</span>
    </footer>
  );
}

function OperatorFooter({ state }: { state: GameState }) {
  const calls = Array.isArray(state.calls) ? state.calls.length : 0;
  const incidents = Array.isArray(state.incidents) ? state.incidents.length : 0;
  const units = Array.isArray(state.units) ? state.units.length : 0;
  return (
    <footer>
      <span>{calls} CALL EVENTS</span>
      <span>{incidents} SCENES</span>
      <span>{units} UNITS</span>
      <span>REAL CLOCK</span>
      <span>NO AUTO ACTIONS</span>
    </footer>
  );
}

function SausageFooter({ features }: { features: SausageFeatures }) {
  return (
    <footer>
      <span>Z×{features.height_span}</span>
      <span>{features.tiles} TILES</span>
      <span>{features.sausages} SAUSAGES</span>
      <span>{features.cooked_faces} COOKED FACES</span>
      <span>{features.grills} GRILLS</span>
      <span>{features.ladders} LADDERS</span>
      <span>{features.detached_fork ? "FORK LOOSE" : "FORK HELD"}</span>
      <span>{features.exit_ready ? "EXIT READY" : "IN PROGRESS"}</span>
    </footer>
  );
}

function ParaboxFooter({ sample }: { sample: Sample }) {
  const features = sample.features as ParaboxFeatures;
  return (
    <footer>
      <span>{features.rectangular ? `RECT×${features.rectangular_spaces}` : "SQUARE"}</span>
      <span>{features.flipped ? "FLIPPED" : "NORMAL"}</span>
      <span>{sample.path?.includes("cycle") ? "CYCLE" : `${features.depth} DEEP`}</span>
      <span>{features.visible_spaces} SPACES</span>
      <span>{features.boundary_openings} OPEN</span>
    </footer>
  );
}

function clockLabel(milliseconds: number) {
  const seconds = Math.max(0, Math.floor(milliseconds / 1000));
  return `${String(Math.floor(seconds / 60)).padStart(2, "0")}:${String(seconds % 60).padStart(2, "0")}`;
}

const globalRoot = globalThis as typeof globalThis & { __qaGalleryRoot?: Root };
globalRoot.__qaGalleryRoot ??= createRoot(document.getElementById("root")!);
globalRoot.__qaGalleryRoot.render(<Gallery />);
