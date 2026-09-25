import { expect, test } from "@playwright/test";
import { readFileSync } from "node:fs";
const states = JSON.parse(readFileSync(new URL("./fixtures/game-states.json", import.meta.url), "utf8")) as Record<string, unknown>;

// Authoritative samples cover flipped recursion, multi-height 3D geometry,
// dispatch, kitchen, both tile boards, and a populated robot roster.
for (const [game, state] of Object.entries(states)) {
  test(`${game} renders WebGL at narrow widths and restores its context`, async ({ page }) => {
    const errors: string[] = [];
    page.on("pageerror", error => errors.push(error.message));
    const summary = { id: `gpu-${game}`, game, model: "GPU fixture", task: game, agent: "fixture", effort: "default", objective: game, live: false, score: 0, total: 1, latest_sequence: 1, detail_revision: "fixture", termination: { kind: "stopped" }, score_history: [] };
    await page.route("**/api/live/v1/**", route => {
      const url = new URL(route.request().url()), path = url.pathname;
      if (url.searchParams.has("replay_attempt")) return route.fulfill({ json: {
        attempt_id: 1, kind: "level", reference: "fixture", score: 1, successful: true,
        frames: [0, 1].map(index => ({ key: `f${index}`, event: { state, sequence: index, type: "move", action: { command: "move" } }, operation_sequence: index, operation_action: { command: "move" }, instruction_index: 0, instruction_count: 1, operation_size: 1, has_instruction_trace: false })),
        operations: [], activity: [], skipped_unchanged: 0, eliminated_history_frames: 0,
      } });
      if (path.endsWith("/subscribe")) return route.fulfill({ contentType: "text/event-stream", body: `data: ${JSON.stringify({ schema: "benchmark-live-subscription-v2", revision: 1, reset: true, removed: [], runs: [summary], generated_at: Date.now() })}\n\n` });
      return route.fulfill({ json: path.endsWith(`/${summary.id}`) ? { run: { ...summary, state, replay_groups: [{ kind: "level", reference: "fixture", attempts: [{ id: 1, successful: true, score: 1 }] }], live_replay: null, recent_activity: [] } } : { runs: [summary] } });
    });
    await page.goto(`/?run=${summary.id}`);
    const canvas = page.locator("canvas[data-renderer=webgl]");
    await expect(canvas).toHaveAttribute("data-ready", "true");
    for (const width of [1280, 375, 320]) {
      await page.setViewportSize({ width, height: 900 });
      await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
      await expect(canvas).toHaveAttribute("data-ready", "true");
    }
    const hasPixels = () => canvas.evaluate((node: HTMLCanvasElement) => {
      const gl = node.getContext("webgl2")!;
      const pixels = new Uint8Array(node.width * node.height * 4);
      gl.readPixels(0, 0, node.width, node.height, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
      const colors = new Set<number>();
      for (let i = 0; i < pixels.length; i += 64) colors.add((pixels[i] << 16) | (pixels[i + 1] << 8) | pixels[i + 2]);
      return colors.size > 12;
    });
    await expect.poll(hasPixels).toBe(true);
    await canvas.evaluate((node: HTMLCanvasElement) => {
      const extension = node.getContext("webgl2")!.getExtension("WEBGL_lose_context")!;
      node.addEventListener("webglcontextlost", () => setTimeout(() => extension.restoreContext(), 300), { once: true });
      extension.loseContext();
    });
    await expect(canvas).toHaveAttribute("data-ready", "false");
    await expect(canvas).toHaveAttribute("data-ready", "true");
    await expect.poll(hasPixels).toBe(true);
    await expect(page.locator(".game-render-error, .sausage-render-error")).toHaveCount(0);
    await page.locator(".score-replay-list button.scored").click();
    await page.getByRole("button", { name: "更多回放操作" }).click();
    const downloaded = page.waitForEvent("download");
    await page.getByRole("menuitem", { name: "导出 GIF", exact: true }).click();
    const download = await downloaded;
    const file = readFileSync((await download.path())!);
    expect(file.subarray(0, 3).toString()).toBe("GIF");
    expect(file.length).toBeGreaterThan(1000);
    expect(file.readUInt16LE(6)).toBeGreaterThan(0);
    await expect(page.locator(".replay-status")).toContainText("GIF 已下载");
    expect(errors).toEqual([]);
  });
}
