import { Container, Graphics, Text, WebGLRenderer } from "pixi.js";
import type { GameScene, Rect } from "./scene";

/** One WebGL surface per view/export, with no ticker or retained frame history. */
export class WebGLSurface {
  readonly renderer = new WebGLRenderer<HTMLCanvasElement>();
  private stage = new Container();
  private destroyed = false;
  private resolutionLimit = 1;
  private constructor(readonly canvas: HTMLCanvasElement) {}
  static async create(canvas: HTMLCanvasElement, width: number, height: number, resolution = 1, context?: WebGL2RenderingContext) {
    const surface = new WebGLSurface(canvas);
    surface.resolutionLimit = resolution;
    await surface.renderer.init({ canvas, width, height, resolution, context, backgroundAlpha: context ? 0 : 1, antialias: true, preserveDrawingBuffer: true, clearBeforeRender: !context, powerPreference: "high-performance" });
    surface.stage.eventMode = "none";
    canvas.dataset.renderer = "webgl";
    return surface;
  }
  private clear() {
    const destroy = (node: Container) => {
      for (const child of node.removeChildren()) destroy(child);
      if (node instanceof Text) node.destroy({ texture: true, textureSource: true, style: true });
      else if (node instanceof Graphics) node.destroy({ context: true });
      else node.destroy();
    };
    for (const child of this.stage.removeChildren()) destroy(child);
  }
  render(scene: GameScene, options: { width?: number; height?: number; clear?: boolean } = {}) {
    if (this.destroyed || this.renderer.gl.isContextLost()) return;
    this.clear();
    const width = options.width ?? scene.width, height = options.height ?? scene.height;
    const resolution = Math.min(this.resolutionLimit, Math.sqrt(4_000_000 / Math.max(1, width * height)));
    if (this.renderer.screen.width !== width || this.renderer.screen.height !== height || this.renderer.resolution !== resolution) this.renderer.resize(width, height, resolution);
    this.renderer.background.color = scene.background;
    const scale = Math.min(width / scene.width, height / scene.height);
    this.stage.scale.set(scale);
    this.stage.position.set((width - scene.width * scale) / 2, 0);
    let layer = this.stage, graphics: Graphics | null = null, clipKey = "";
    const group = (clip?: Rect) => {
      const key = clip ? `${clip.x},${clip.y},${clip.width},${clip.height}` : "";
      if (key === clipKey) return;
      clipKey = key; graphics = null;
      layer = new Container(); layer.eventMode = "none"; this.stage.addChild(layer);
      if (clip) {
        const mask = new Graphics().rect(clip.x, clip.y, clip.width, clip.height).fill(0xffffff);
        layer.addChild(mask); layer.mask = mask;
      }
    };
    for (const command of scene.commands) {
      group(command.clip);
      if (command.kind === "text") {
        const text = new Text({ text: command.text, style: { fontFamily: "Menlo, Consolas, monospace", fontSize: command.size, fill: command.color, fontWeight: command.bold ? "700" : "400" }, resolution: Math.min(2, Math.max(1, this.renderer.resolution)) });
        text.position.set(command.x, command.y);
        text.anchor.set(command.align === "center" ? .5 : command.align === "right" ? 1 : 0, 0);
        text.alpha = command.alpha ?? 1;
        layer.addChild(text); graphics = null;
        continue;
      }
      if (!graphics) { graphics = new Graphics(); layer.addChild(graphics); }
      if (command.kind === "rect") graphics.roundRect(command.x, command.y, command.width, command.height, Math.min(command.radius ?? 0, command.width / 2, command.height / 2));
      else if (command.kind === "circle") graphics.circle(command.x, command.y, command.radius);
      else if (command.kind === "polygon") graphics.poly(command.points);
      else {
        graphics.moveTo(command.points[0], command.points[1]);
        for (let i = 2; i < command.points.length; i += 2) graphics.lineTo(command.points[i], command.points[i + 1]);
        graphics.stroke({ color: command.color, width: command.width, alpha: command.alpha ?? 1 });
        continue;
      }
      if (command.color) graphics.fill({ color: command.color, alpha: command.alpha ?? 1 });
      if (command.stroke) graphics.stroke({ color: command.stroke, width: command.strokeWidth ?? 1, alpha: command.alpha ?? 1 });
    }
    this.renderer.resetState();
    this.renderer.render({ container: this.stage, clear: options.clear ?? true });
    this.canvas.dataset.ready = "true";
  }
  destroy() {
    if (this.destroyed) return;
    this.destroyed = true; this.clear(); this.stage.destroy();
    this.renderer.destroy({ removeView: false });
  }
}
