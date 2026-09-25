import { GIFEncoder, applyPalette, quantize } from "gifenc";

export type ReplayExportFormat = "gif" | "video";

type ReplayExportOptions = {
  frameCount: number;
  frameDelayMs: number;
  /** Extra time the last frame stays on screen, so a looping GIF shows the result. */
  lastFrameHoldMs?: number;
  fileName: string;
  format: ReplayExportFormat;
  captureFrame: (index: number) => HTMLCanvasElement | Promise<HTMLCanvasElement>;
  onProgress: (completed: number) => void;
  signal?: AbortSignal;
};

type MediaRuntime = typeof import("mediabunny");
type RecordedFrame = {
  packet: import("mediabunny").EncodedPacket;
  codec: import("mediabunny").VideoCodec;
  decoderConfig: VideoDecoderConfig;
};

function throwIfAborted(signal?: AbortSignal) {
  if (signal?.aborted) {
    throw new DOMException("Replay export cancelled.", "AbortError");
  }
}

export async function exportReplaySegment(options: ReplayExportOptions) {
  throwIfAborted(options.signal);
  const readback = document.createElement("canvas");
  const readContext = readback.getContext("2d", { willReadFrequently: true });
  if (!readContext) throw new Error("无法读取回放画布");
  let lastYield = performance.now();
  const capture = async (index: number) => {
    if (performance.now() - lastYield > 32) { await delay(0, options.signal); lastYield = performance.now(); }
    throwIfAborted(options.signal);
    const frame = await options.captureFrame(index);
    if (readback.width !== frame.width || readback.height !== frame.height) { readback.width = frame.width; readback.height = frame.height; }
    readContext.drawImage(frame, 0, 0);
    return readback;
  };

  if (options.format === "gif") {
    const gif = GIFEncoder();
    for (let index = 0; index < options.frameCount; index += 1) {
      const canvas = await capture(index);
      const context = canvas.getContext("2d", { willReadFrequently: true });
      if (!context) throw new Error("Could not read the replay frame.");
      const pixels = context.getImageData(0, 0, canvas.width, canvas.height).data;
      const palette = quantize(pixels, 128, { format: "rgb444" });
      const last = index === options.frameCount - 1;
      gif.writeFrame(applyPalette(pixels, palette, "rgb444"), canvas.width, canvas.height, {
        delay: options.frameDelayMs + (last ? options.lastFrameHoldMs ?? 0 : 0),
        palette,
        repeat: 0,
      });
      options.onProgress(index + 1);
    }
    gif.finish();
    throwIfAborted(options.signal);
    download(new Blob([Uint8Array.from(gif.bytes())], { type: "image/gif" }), `${options.fileName}.gif`);
    return;
  }

  const firstFrame = await capture(0);
  const media = await import("mediabunny");
  if (typeof VideoEncoder !== "undefined") {
    const { BufferTarget, Output, WebMOutputFormat, Mp4OutputFormat, CanvasSource, Quality, canEncodeVideo } = media;
    const quality = new Quality({ bitrate: 2_500_000 });
    const canvas = document.createElement("canvas");
    canvas.width = Math.ceil(firstFrame.width / 2) * 2;
    canvas.height = Math.ceil(firstFrame.height / 2) * 2;
    const context = canvas.getContext("2d")!;
    // Prefer a file that also opens in native players, including QuickTime.
    for (const codec of ["avc", "vp9", "vp8"] as const) {
      if (!await canEncodeVideo(codec, { width: canvas.width, height: canvas.height, quality, latencyMode: "quality" })) continue;
      const target = new BufferTarget();
      const output = new Output({ target, format: codec === "avc" ? new Mp4OutputFormat() : new WebMOutputFormat() });
      const encodedTimestamps = new Set<number>();
      const source = new CanvasSource(canvas, {
        codec, quality, latencyMode: "quality",
        onEncodedPacket: packet => { encodedTimestamps.add(Math.round(packet.timestamp * 1e6)); },
      });
      output.addVideoTrack(source, { frameRate: 1000 / options.frameDelayMs });
      try {
        await output.start();
        const duration = options.frameDelayMs / 1000;
        const hold = (options.lastFrameHoldMs ?? 0) / 1000;
        for (let index = 0; index < options.frameCount; index++) {
          throwIfAborted(options.signal);
          const frame = index === 0 ? firstFrame : await capture(index);
          context.fillStyle = "#08100e"; context.fillRect(0, 0, canvas.width, canvas.height);
          context.drawImage(frame, 0, 0);
          await source.add(index * duration, index === options.frameCount - 1 ? duration + hold : duration);
          options.onProgress(index + 1);
        }
        source.close();
        await output.finalize();
        throwIfAborted(options.signal);
        if (encodedTimestamps.size !== options.frameCount || Array.from({ length: options.frameCount }, (_, index) => Math.round(index * duration * 1e6)).some(timestamp => !encodedTimestamps.has(timestamp))) {
          throw new Error("视频帧不完整，请重试导出。");
        }
        if (!target.buffer) throw new Error("视频编码未生成文件");
        const extension = codec === "avc" ? "mp4" : "webm";
        download(new Blob([target.buffer], { type: `video/${extension}` }), `${options.fileName}.${extension}`);
      } catch (reason) { await output.cancel().catch(() => {}); throw reason; }
      finally { source.close(); }
      return;
    }
  }
  const canvas = document.createElement("canvas");
  canvas.width = Math.ceil(firstFrame.width / 2) * 2;
  canvas.height = Math.ceil(firstFrame.height / 2) * 2;
  const context = canvas.getContext("2d");
  if (!context || !canvas.captureStream || typeof MediaRecorder === "undefined") {
    throw new Error("This browser cannot export replay video.");
  }
  const format = videoTarget();
  const target = new media.BufferTarget();
  const output = new media.Output({ target, format: format.extension === "mp4" ? new media.Mp4OutputFormat() : new media.WebMOutputFormat() });
  let source: InstanceType<MediaRuntime["EncodedVideoPacketSource"]> | undefined;
  let decoderKey: string | undefined;
  const duration = options.frameDelayMs / 1000;
  const hold = (options.lastFrameHoldMs ?? 0) / 1000;
  try {
    for (let index = 0; index < options.frameCount; index++) {
      throwIfAborted(options.signal);
      const frame = index === 0 ? firstFrame : await capture(index);
      context.fillStyle = "#08100e";
      context.fillRect(0, 0, canvas.width, canvas.height);
      context.drawImage(frame, 0, 0);
      // Hold each still until its keyframe exists. Browser paint throttling can
      // delay encoding, but can no longer advance past an unrecorded game state.
      const encoded = await recordStill(canvas, context, format.mimeType, media, options.signal);
      const key = decoderConfigKey(encoded.decoderConfig);
      if (!source) {
        source = new media.EncodedVideoPacketSource(encoded.codec);
        decoderKey = key;
        output.addVideoTrack(source, { frameRate: 1000 / options.frameDelayMs });
        await output.start();
      } else if (key !== decoderKey) {
        throw new Error("视频编码格式发生变化，请重试导出。");
      }
      const frameDuration = index === options.frameCount - 1 ? duration + hold : duration;
      await source.add(encoded.packet.clone({ timestamp: index * duration, duration: frameDuration, sequenceNumber: index }), { decoderConfig: encoded.decoderConfig });
      options.onProgress(index + 1);
    }
    source?.close();
    await output.finalize();
    throwIfAborted(options.signal);
    if (!target.buffer) throw new Error("视频编码未生成文件");
    download(new Blob([target.buffer], { type: format.mimeType }), `${options.fileName}.${format.extension}`);
  } catch (reason) {
    await output.cancel().catch(() => {});
    throw reason;
  }
}

function decoderConfigKey(config: VideoDecoderConfig) {
  const description = config.description;
  const bytes = description ? ArrayBuffer.isView(description)
    ? new Uint8Array(description.buffer, description.byteOffset, description.byteLength)
    : new Uint8Array(description) : [];
  return JSON.stringify([config.codec, config.codedWidth, config.codedHeight, Array.from(bytes)]);
}

async function recordStill(canvas: HTMLCanvasElement, context: CanvasRenderingContext2D, mimeType: string, media: MediaRuntime, signal?: AbortSignal): Promise<RecordedFrame> {
  // Stop/flush each short recording before inspecting it: some browsers do
  // not publish a complete MP4 fragment until the recorder stops. A missing
  // frame retries this same still with more time; it never advances the replay.
  for (const holdMs of [100, 250, 500, 1000, 2000, 4000]) {
    throwIfAborted(signal);
    const blob = await recordStillBlob(canvas, context, mimeType, holdMs, signal);
    const input = new media.Input({ source: new media.BlobSource(blob), formats: media.ALL_FORMATS });
    try {
      const track = await input.getPrimaryVideoTrack();
      if (!track) continue;
      const packet = await new media.EncodedPacketSink(track).getFirstKeyPacket();
      const decoderConfig = await track.getDecoderConfig();
      const codec = await track.getCodec();
      if (packet && decoderConfig && codec) return { packet: packet.clone({ data: Uint8Array.from(packet.data) }), decoderConfig, codec };
    } catch {
      // A recording stopped before its first paint can contain only a header.
    } finally { input.dispose(); }
  }
  throw new Error("未能捕获完整视频帧，请保持页面可见后重试。");
}

function recordStillBlob(canvas: HTMLCanvasElement, context: CanvasRenderingContext2D, mimeType: string, holdMs: number, signal?: AbortSignal): Promise<Blob> {
  throwIfAborted(signal);
  const stream = canvas.captureStream(30);
  let recorder: MediaRecorder;
  const recorderOptions: MediaRecorderOptions & { videoKeyFrameIntervalCount: number } = { mimeType, videoBitsPerSecond: 2_500_000, videoKeyFrameIntervalCount: 1 };
  try { recorder = new MediaRecorder(stream, recorderOptions); }
  catch (error) { stream.getTracks().forEach(track => track.stop()); throw error; }
  return new Promise((resolve, reject) => {
    const chunks: Blob[] = [];
    let settled = false;
    const finish = (error?: unknown) => {
      if (settled) return;
      settled = true;
      clearInterval(repaint); clearTimeout(timeout);
      signal?.removeEventListener("abort", abort);
      stream.getTracks().forEach(track => track.stop());
      if (error) reject(error); else resolve(new Blob(chunks, { type: mimeType }));
    };
    const abort = () => {
      if (recorder.state !== "inactive") recorder.stop();
      finish(new DOMException("Replay export cancelled.", "AbortError"));
    };
    const timeout = setTimeout(() => { if (recorder.state !== "inactive") recorder.stop(); }, holdMs);
    const repaint = setInterval(() => context.drawImage(canvas, 0, 0), 1000 / 30);
    signal?.addEventListener("abort", abort, { once: true });
    recorder.ondataavailable = event => { if (event.data.size) chunks.push(event.data); };
    recorder.onerror = () => finish(new Error("视频编码失败"));
    recorder.onstop = () => finish();
    try { recorder.start(); context.drawImage(canvas, 0, 0); }
    catch (error) { finish(error); }
  });
}

function videoTarget() {
  const targets = [
    { mimeType: "video/mp4;codecs=avc1.42001f", extension: "mp4" },
    { mimeType: "video/mp4", extension: "mp4" },
    { mimeType: "video/webm;codecs=vp9", extension: "webm" },
    { mimeType: "video/webm;codecs=vp8", extension: "webm" },
    { mimeType: "video/webm", extension: "webm" },
  ];
  const target = targets.find(({ mimeType }) => MediaRecorder.isTypeSupported(mimeType));
  if (!target) throw new Error("This browser has no supported video encoder.");
  return target;
}

function download(blob: Blob, fileName: string) {
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = fileName;
  link.click();
  window.setTimeout(() => URL.revokeObjectURL(url), 30_000);
}

function delay(milliseconds: number, signal?: AbortSignal) {
  return new Promise<void>((resolve, reject) => {
    if (signal?.aborted) { reject(new DOMException("Replay export cancelled.", "AbortError")); return; }
    const abort = () => { window.clearTimeout(timer); reject(new DOMException("Replay export cancelled.", "AbortError")); };
    const timer = window.setTimeout(() => { signal?.removeEventListener("abort", abort); resolve(); }, milliseconds);
    signal?.addEventListener("abort", abort, { once: true });
  });
}
