import type { DrawCommand, GameScene, Rect, Sprite } from "./scene";

/** Where each sprite of `to` starts: its partner in `from`, or null when it has none. Same-key sprites that did not move pair first, the rest pair by nearest centre. */
export function pairSprites(from: Sprite[], to: Sprite[]): (Rect | null)[] {
  const starts: (Rect | null)[] = to.map(() => null);
  const keys = new Set(to.map(sprite => sprite.key));
  for (const key of keys) {
    const olds = from.filter(sprite => sprite.key === key).map(sprite => sprite.bounds);
    const news = to.flatMap((sprite, index) => sprite.key === key ? [index] : []);
    const open = new Set(news);
    for (const index of news) {
      const same = olds.findIndex(bounds => sameRect(bounds, to[index].bounds));
      if (same >= 0) { starts[index] = olds.splice(same, 1)[0]; open.delete(index); }
    }
    while (open.size && olds.length) {
      let best: [number, number, number] | null = null;
      for (const index of open) olds.forEach((bounds, old) => {
        const distance = Math.hypot(centre(bounds).x - centre(to[index].bounds).x, centre(bounds).y - centre(to[index].bounds).y);
        if (!best || distance < best[2]) best = [index, old, distance];
      });
      const [index, old] = best!;
      starts[index] = olds.splice(old, 1)[0]; open.delete(index);
    }
  }
  return starts;
}

export function sameRect(a: Rect, b: Rect) {
  return Math.abs(a.x - b.x) < .5 && Math.abs(a.y - b.y) < .5 && Math.abs(a.width - b.width) < .5 && Math.abs(a.height - b.height) < .5;
}

export function lerpRect(a: Rect, b: Rect, t: number): Rect {
  return { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t, width: a.width + (b.width - a.width) * t, height: a.height + (b.height - a.height) * t };
}

export const easeInOut = (t: number) => t < .5 ? 2 * t * t : 1 - (-2 * t + 2) ** 2 / 2;

/**
 * The scene with each sprite drawn at `placed[i]` and faded to `alpha[i]`.
 * Sprites are drawn after everything else so a moving piece passes over the
 * floor of the cells it crosses; at rest they overlap nothing, so the order
 * does not show.
 */
export function placeSprites(scene: GameScene, placed: Rect[], alpha: number[] = []): GameScene {
  const still: DrawCommand[] = [], moving: DrawCommand[] = [];
  for (const command of scene.commands) {
    if (command.sprite === undefined) { still.push(command); continue; }
    const from = scene.sprites[command.sprite].bounds, to = placed[command.sprite] ?? from;
    moving.push(move(command, from, to, alpha[command.sprite] ?? 1));
  }
  return { ...scene, commands: [...still, ...moving] };
}

function move(command: DrawCommand, from: Rect, to: Rect, fade: number): DrawCommand {
  const sx = to.width / Math.max(1e-6, from.width), sy = to.height / Math.max(1e-6, from.height), s = Math.min(sx, sy);
  const x = (value: number) => to.x + (value - from.x) * sx, y = (value: number) => to.y + (value - from.y) * sy;
  const points = (values: number[]) => values.map((value, index) => index % 2 ? y(value) : x(value));
  // A clip inside the sprite (a box's own interior) moves with it; the view's clip stays put.
  const clip = command.clip && inside(command.clip, from) ? { x: x(command.clip.x), y: y(command.clip.y), width: command.clip.width * sx, height: command.clip.height * sy } : command.clip;
  const alpha = (command.alpha ?? 1) * fade;
  switch (command.kind) {
    case "rect": return { ...command, clip, alpha, x: x(command.x), y: y(command.y), width: command.width * sx, height: command.height * sy, radius: (command.radius ?? 0) * s, strokeWidth: (command.strokeWidth ?? 1) * s };
    case "circle": return { ...command, clip, alpha, x: x(command.x), y: y(command.y), radius: command.radius * s, strokeWidth: (command.strokeWidth ?? 1) * s };
    case "line": return { ...command, clip, alpha, points: points(command.points), width: command.width * s };
    case "polygon": return { ...command, clip, alpha, points: points(command.points), strokeWidth: (command.strokeWidth ?? 1) * s };
    case "text": return { ...command, clip, alpha, x: x(command.x), y: y(command.y), size: command.size * s };
  }
}

function inside(inner: Rect, outer: Rect) {
  return inner.x >= outer.x - .5 && inner.y >= outer.y - .5 && inner.x + inner.width <= outer.x + outer.width + .5 && inner.y + inner.height <= outer.y + outer.height + .5;
}

function centre(rect: Rect) { return { x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 }; }
