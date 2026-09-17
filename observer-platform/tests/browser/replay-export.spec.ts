import { expect, test, type Page } from "@playwright/test";
import { readFileSync } from "node:fs";

const puzzle = JSON.parse(readFileSync(new URL("./fixtures/game-states.json", import.meta.url), "utf8")).sausage;
const overworld = {
  ...puzzle,
  mode: "overworld",
  level: null,
  overworld: { title: "Land's End", entrances: [] },
  tiles: puzzle.tiles.map((tile: { pos: { x: number; y: number; z: number } }) => ({
    ...tile, pos: { ...tile.pos, x: tile.pos.x * 4, y: tile.pos.y * 4 },
  })),
};

async function openReplay(page: Page) {
  const summary = { id: "sausage-export-transition", game: "sausage", model: "fixture", task: "sausage", agent: "fixture", effort: "default", objective: "sausage", live: false, score: 1, total: 86, latest_sequence: 2, detail_revision: "fixture", termination: { kind: "stopped" }, score_history: [] };
  await page.route("**/api/live/v1/**", route => {
    const url = new URL(route.request().url());
    if (url.searchParams.has("replay_attempt")) return route.fulfill({ json: {
      attempt_id: 1, kind: "level", reference: "fixture", score: 1, successful: true,
      frames: [puzzle, overworld].map((state, index) => ({ key: `f${index}`, event: { state, sequence: index, type: "move", action: { command: "move" } }, operation_sequence: index, operation_action: { command: "move" }, instruction_index: 0, instruction_count: 1, operation_size: 1, has_instruction_trace: false })),
      operations: [], activity: [], skipped_unchanged: 0, eliminated_history_frames: 0,
    } });
    if (url.pathname.endsWith("/subscribe")) return route.fulfill({ contentType: "text/event-stream", body: `data: ${JSON.stringify({ schema: "benchmark-live-subscription-v2", revision: 1, reset: true, removed: [], runs: [summary], generated_at: Date.now() })}\n\n` });
    return route.fulfill({ json: { run: { ...summary, state: puzzle, replay_groups: [{ kind: "level", reference: "fixture", attempts: [{ id: 1, successful: true, score: 1 }] }], live_replay: null, recent_activity: [] } } });
  });
  await page.setViewportSize({ width: 960, height: 900 });
  await page.goto(`/?run=${summary.id}`);
}

test("sausage GIF keeps the opening puzzle view when exported from the final overworld frame", async ({ page }, testInfo) => {
  await openReplay(page);
  const canvas = page.locator("canvas[data-renderer=webgl]");
  await expect(canvas).toHaveAttribute("data-ready", "true");
  const expected = await canvas.evaluate((node: HTMLCanvasElement) => {
    const sample = document.createElement("canvas"); sample.width = 120; sample.height = 120;
    const context = sample.getContext("2d")!; context.drawImage(node, 0, 0, 120, 120);
    return Array.from(context.getImageData(0, 0, 120, 120).data);
  });
  await canvas.screenshot({ path: testInfo.outputPath("opening-puzzle.png") });
  await page.locator(".score-replay-list button.scored").click();
  await expect(page.getByRole("button", { name: "下一条有效指令", exact: true })).toBeDisabled();
  await page.getByRole("button", { name: "更多回放操作" }).click();
  const downloaded = page.waitForEvent("download");
  await page.getByRole("menuitem", { name: "导出 GIF", exact: true }).click();
  const download = await downloaded;
  await download.saveAs(testInfo.outputPath("export.gif"));
  const gif = readFileSync((await download.path())!);
  await testInfo.attach("export.gif", { body: gif, contentType: "image/gif" });
  const actual = await page.evaluate(async bytes => {
    const Decoder = (window as unknown as { ImageDecoder: new (options: { data: Uint8Array; type: string }) => { decode(options: { frameIndex: number }): Promise<{ image: VideoFrame }>; close(): void } }).ImageDecoder;
    const decoder = new Decoder({ data: new Uint8Array(bytes), type: "image/gif" });
    const { image } = await decoder.decode({ frameIndex: 0 });
    try {
      const sample = document.createElement("canvas"); sample.width = 120; sample.height = 120;
      const context = sample.getContext("2d")!; context.drawImage(image, 0, 0, 120, 120);
      return Array.from(context.getImageData(0, 0, 120, 120).data);
    } finally { image.close(); decoder.close(); }
  }, Array.from(gif));
  const meanDifference = actual.reduce((sum, value, index) => sum + Math.abs(value - expected[index]), 0) / actual.length;
  // Permit GIF palette quantization; a camera from another scene changes most pixels.
  expect(meanDifference).toBeLessThan(12);
});

for (const encoder of ["WebCodecs", "MediaRecorder"] as const) {
  test(`sausage video exported with ${encoder} decodes both replay frames`, async ({ page }, testInfo) => {
    if (encoder === "MediaRecorder") {
      await page.addInitScript(() => Object.defineProperty(window, "VideoEncoder", { value: undefined }));
    }
    await openReplay(page);
    await expect(page.locator("canvas[data-renderer=webgl]")).toHaveAttribute("data-ready", "true");
    await page.locator(".score-replay-list button.scored").click();
    await expect(page.getByRole("button", { name: "下一条有效指令", exact: true })).toBeDisabled();
    await page.getByRole("button", { name: "更多回放操作" }).click();
    const downloaded = page.waitForEvent("download");
    await page.getByRole("menuitem", { name: "导出视频", exact: true }).click();
    const download = await downloaded;
    expect(download.suggestedFilename()).toMatch(/\.mp4$/);
    await download.saveAs(testInfo.outputPath(download.suggestedFilename()));
    const bytes = Array.from(readFileSync((await download.path())!));
    const result = await page.evaluate(async data => {
      const video = document.createElement("video");
      video.muted = true;
      const url = URL.createObjectURL(new Blob([new Uint8Array(data)]));
      const event = (name: string) => new Promise<void>((resolve, reject) => {
        const timeout = setTimeout(() => reject(new Error(`Video ${name} timed out`)), 10_000);
        video.addEventListener(name, () => { clearTimeout(timeout); resolve(); }, { once: true });
        video.addEventListener("error", () => { clearTimeout(timeout); reject(new Error(video.error?.message ?? "Video decode failed")); }, { once: true });
      });
      try {
        const loaded = event("loadeddata"); video.src = url; await loaded;
        const width = video.videoWidth, height = video.videoHeight, duration = video.duration;
        const canvas = document.createElement("canvas"); canvas.width = 120; canvas.height = 120;
        const context = canvas.getContext("2d")!;
        const sample = async (time: number) => {
          const seeked = event("seeked"); video.currentTime = time; await seeked;
          context.drawImage(video, 0, 0, 120, 120);
          return Array.from(context.getImageData(0, 0, 120, 120).data);
        };
        const first = await sample(.1), last = await sample(.65);
        return { width, height, duration, difference: first.reduce((sum, value, index) => sum + Math.abs(value - last[index]), 0) / first.length };
      } finally { video.removeAttribute("src"); video.load(); URL.revokeObjectURL(url); }
    }, bytes);
    expect(result.width).toBeGreaterThan(100);
    expect(result.height).toBeGreaterThan(100);
    expect(result.duration).toBeGreaterThan(.7);
    expect(result.duration).toBeLessThan(1.5);
    expect(result.difference).toBeGreaterThan(12);
    await expect(page.locator(".replay-status")).toContainText("视频已下载");
  });
}
