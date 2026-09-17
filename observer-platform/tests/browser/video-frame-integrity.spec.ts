import { expect, test } from "@playwright/test";
import { readFileSync, readdirSync } from "node:fs";
import { ALL_FORMATS, BufferSource, EncodedPacketSink, Input } from "mediabunny";

const exporter = readdirSync(new URL("../../dist/client/assets/", import.meta.url)).find(name => /^replay-export-.*\.js$/.test(name))!;
const frameCount = 64;
const frameDelayMs = 112.5; // The UI's 4x replay speed.

for (const encoder of ["WebCodecs", "MediaRecorder", "MediaRecorder throttled"] as const) {
  test(`${encoder} preserves every numbered frame under rendering load`, async ({ page }, testInfo) => {
    test.setTimeout(60_000);
    if (encoder !== "WebCodecs") await page.addInitScript(() => Object.defineProperty(window, "VideoEncoder", { value: undefined }));
    if (encoder === "MediaRecorder throttled") await page.addInitScript(() => {
      const captureStream = HTMLCanvasElement.prototype.captureStream;
      HTMLCanvasElement.prototype.captureStream = function () { return captureStream.call(this, 4); };
    });
    await page.route("**/api/live/v1/**", route => route.fulfill({ contentType: "text/event-stream", body: `data: ${JSON.stringify({ schema: "benchmark-live-subscription-v2", reset: true, runs: [], removed: [], revision: 1 })}\n\n` }));
    await page.goto("/");
    const downloaded = page.waitForEvent("download");
    await page.evaluate(async ({ exporter, frameCount, frameDelayMs }) => {
      const { exportReplaySegment } = await import(`/assets/${exporter}`);
      const canvas = document.createElement("canvas"); canvas.width = 192; canvas.height = 128;
      const context = canvas.getContext("2d")!;
      await exportReplaySegment({
        format: "video", fileName: "numbered-frames", frameCount, frameDelayMs, onProgress() {},
        async captureFrame(index: number) {
          // Simulate the asynchronous work required to render a large game scene.
          if (index % 4 === 0) await new Promise(resolve => setTimeout(resolve, 80));
          for (let bit = 0; bit < 6; bit++) {
            context.fillStyle = index & (1 << bit) ? "white" : "black";
            context.fillRect(bit * 32, 0, 32, 128);
          }
          return canvas;
        },
      });
    }, { exporter, frameCount, frameDelayMs });
    const download = await downloaded;
    await download.saveAs(testInfo.outputPath(download.suggestedFilename()));
    const input = new Input({ source: new BufferSource(readFileSync((await download.path())!)), formats: ALL_FORMATS });
    try {
      const track = (await input.getPrimaryVideoTrack())!;
      const config = (await track.getDecoderConfig())!;
      const packets = [];
      for await (const packet of new EncodedPacketSink(track).packets()) {
        packets.push({ type: packet.type, timestamp: Math.round(packet.timestamp * 1e6), duration: Math.round(packet.duration * 1e6), data: Array.from(packet.data) });
      }
      const description = config.description ? Array.from(new Uint8Array(config.description as ArrayBuffer)) : null;
      const decoded = await page.evaluate(async ({ config, description, packets }) => {
        const frames: { number: number; timestamp: number; duration: number | null }[] = [];
        const canvas = document.createElement("canvas"); canvas.width = 192; canvas.height = 128;
        const context = canvas.getContext("2d")!;
        let error: DOMException | undefined;
        const decoder = new VideoDecoder({
          output(frame) {
            try {
              context.drawImage(frame, 0, 0);
              let number = 0;
              for (let bit = 0; bit < 6; bit++) if (context.getImageData(bit * 32 + 16, 64, 1, 1).data[0] > 128) number |= 1 << bit;
              frames.push({ number, timestamp: frame.timestamp, duration: frame.duration });
            } finally { frame.close(); }
          },
          error(reason) { error = reason; },
        });
        try {
          decoder.configure({ ...config, ...(description ? { description: new Uint8Array(description) } : {}) });
          for (const packet of packets) decoder.decode(new EncodedVideoChunk({ ...packet, data: new Uint8Array(packet.data) }));
          await decoder.flush();
          if (error) throw error;
          return frames;
        } finally { decoder.close(); }
      }, { config: { codec: config.codec, codedWidth: config.codedWidth, codedHeight: config.codedHeight }, description, packets });
      await testInfo.attach("decoded-frames.json", { body: JSON.stringify(decoded, null, 2), contentType: "application/json" });
      const unique = decoded.filter((frame, index) => index === 0 || frame.number !== decoded[index - 1].number);
      expect(unique.map(frame => frame.number)).toEqual(Array.from({ length: frameCount }, (_, index) => index));
      expect(await track.computeDuration()).toBeCloseTo(frameCount * frameDelayMs / 1000, 6);
      expect(decoded).toHaveLength(frameCount);
      decoded.forEach((frame, index) => {
        expect(Math.abs(frame.timestamp - index * frameDelayMs * 1000)).toBeLessThanOrEqual(1);
        expect(Math.abs((frame.duration ?? 0) - frameDelayMs * 1000)).toBeLessThanOrEqual(1);
      });
    } finally { input.dispose(); }
  });
}
