import { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";

import { ParaboxState } from "../games/parabox-intro/observer";
import type { GameState } from "./app/game-observer";
import "./app/globals.css";
import "./qa-gallery.css";

type Sample = {
  reference: string;
  title: string;
  area: string;
  step: number;
  total_steps: number;
  path: string;
  features: {
    depth: number;
    rectangular: boolean;
    rectangular_spaces: number;
    flipped: boolean;
    visible_spaces: number;
    nested_boxes: number;
    boundary_openings: number;
  };
  state: GameState;
};

function Gallery() {
  const [samples, setSamples] = useState<Sample[]>([]);
  const [error, setError] = useState("");
  useEffect(() => {
    fetch("/parabox-render-qa-64.json", { cache: "no-store" })
      .then((response) => {
        if (!response.ok) throw new Error(`HTTP ${response.status}`);
        return response.json();
      })
      .then((payload) => setSamples(payload.samples))
      .catch((reason) => setError(String(reason)));
  }, []);
  return (
    <main className="qa-page">
      <header className="qa-header">
        <div>
          <span>PARABOX RENDER QA</span>
          <h1>64 个真实递归状态</h1>
        </div>
        <p>完整 walkthrough 中分层抽样 · 每格标注关卡、步数、容器路径和状态特征</p>
      </header>
      {error ? (
        <p className="qa-error">
          加载失败：{error}。请先在 observer-platform 运行 `vp run qa:parabox:data`。
        </p>
      ) : null}
      <section className="qa-gallery">
        {samples.map((sample, index) => (
          <article className="qa-card" key={`${sample.reference}:${sample.step}`}>
            <header>
              <strong>{String(index + 1).padStart(2, "0")} · {sample.reference}</strong>
              <span>{sample.title}</span>
              <small>STEP {sample.step}/{sample.total_steps} · DEPTH {sample.features.depth}</small>
              <small>{sample.path}</small>
            </header>
            <div className="qa-render">
              <ParaboxState state={sample.state} />
            </div>
            <footer>
              <span>
                {sample.features.rectangular
                  ? `RECT×${sample.features.rectangular_spaces}`
                  : "SQUARE"}
              </span>
              <span>{sample.features.flipped ? "FLIPPED" : "NORMAL"}</span>
              <span>{sample.path.includes("cycle") ? "CYCLE" : `${sample.features.depth} DEEP`}</span>
              <span>{sample.features.visible_spaces} SPACES</span>
              <span>{sample.features.boundary_openings} OPEN</span>
            </footer>
          </article>
        ))}
      </section>
    </main>
  );
}

createRoot(document.getElementById("root")!).render(<Gallery />);
