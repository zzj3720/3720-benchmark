"use client";
// Helpers shared by the live views and the replay views.
import type { ButtonHTMLAttributes } from "react";
import type { LucideIcon } from "lucide-react";
import { asString, type Json } from "./game-observer";
import type { RunDetail } from "./live-contract";
import type { ReplayExportFormat } from "./replay-export";

export function clockTime(timestamp?: number | null) {
  if (!timestamp) return "—";
  return new Intl.DateTimeFormat("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  }).format(timestamp);
}

export function fileSlug(value: string) {
  return value
    .normalize("NFKD")
    .replace(/[^\w.-]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .toLowerCase();
}

export function durationLabel(durationMs: number, axisMaxMs = durationMs) {
  if (axisMaxMs < 60 * 60 * 1000) return `${Math.round(durationMs / 60_000)}m`;
  return `${(durationMs / (60 * 60 * 1000)).toFixed(1)}h`;
}

export function actionLabel(action?: Record<string, Json> | null) {
  const command = asString(action?.command, "STATE").toUpperCase();
  const direction = asString(action?.direction, "");
  return direction ? `${command} ${direction.toUpperCase()}` : command;
}

export type ExportChoice = { format: ReplayExportFormat; speed: number; caption: boolean };

export const FRAME_MS = 450;
export const EXPORT_HOLD_MS = 1500;

/** Draw a title band above an exported frame: what, who, which attempt, where from. */
export function captionFrame(
  frame: HTMLCanvasElement,
  target: HTMLCanvasElement,
  lines: { title: string; segment: string; step: string },
) {
  const band = Math.max(48, Math.round(frame.width * 0.075));
  if (target.width !== frame.width || target.height !== frame.height + band) {
    target.width = frame.width;
    target.height = frame.height + band;
  }
  const context = target.getContext("2d")!;
  context.fillStyle = "#0f1413";
  context.fillRect(0, 0, target.width, band);
  context.drawImage(frame, 0, band);
  const pad = Math.round(band * 0.3);
  const font = "system-ui, -apple-system, 'PingFang SC', sans-serif";
  context.textBaseline = "alphabetic";
  context.fillStyle = "#edf3eb";
  context.font = `600 ${Math.round(band * 0.3)}px ${font}`;
  context.fillText(lines.title, pad, band * 0.44);
  context.fillStyle = "#8c9892";
  context.font = `${Math.round(band * 0.25)}px ${font}`;
  context.fillText(lines.segment, pad, band * 0.8);
  context.textAlign = "right";
  context.fillText(lines.step, target.width - pad, band * 0.44);
  context.fillStyle = "rgba(237, 243, 235, 0.55)";
  context.font = `${Math.round(band * 0.22)}px ${font}`;
  context.fillText("live.benchmark.3720.org", target.width - pad, target.height - pad * 0.6);
  context.textAlign = "left";
  return target;
}

/** Once the local publisher is gone, time stops where its last heartbeat was. */

export function gatewayUrl(path: string) {
  const origin = import.meta.env.VITE_LIVE_GATEWAY_ORIGIN;
  return new URL(origin ? `${origin}${path}` : `/api/live${path}`, window.location.origin);
}

// Assets are content-addressed and immutable: parse each id once per session
// instead of re-fetching and re-parsing megabytes of JSON on every live tick.
const assetCache = new Map<string, { promise: Promise<Json>; bytes: number }>();
const ASSET_CACHE_BYTES = 16 * 1024 * 1024;

export function fetchAsset(id: string, decodedBytes = 0) {
  const cached = assetCache.get(id);
  if (cached) { assetCache.delete(id); assetCache.set(id, cached); return cached.promise; }
  const promise = fetch(gatewayUrl(`/v1/assets/${id}`), {cache: "force-cache", signal: AbortSignal.timeout(15_000)}).then(async response => {
    if (!response.ok) throw new Error(`asset ${id}: HTTP ${response.status}`);
    return await response.json() as Json;
  });
  const bytes = Math.max(decodedBytes, 64 * 1024) * 4;
  if (bytes <= ASSET_CACHE_BYTES) {
    while ([...assetCache.values()].reduce((total, entry) => total + entry.bytes, 0) + bytes > ASSET_CACHE_BYTES) {
      const oldest = assetCache.keys().next().value;
      if (oldest === undefined) break;
      assetCache.delete(oldest);
    }
    assetCache.set(id, {promise, bytes});
    promise.catch(() => { if (assetCache.get(id)?.promise === promise) assetCache.delete(id); });
  }
  return promise;
}

export async function hydrateRunAssets(detail: RunDetail) {
  const entries = Object.entries(detail.asset_refs ?? {});
  if (!entries.length) return detail;
  const assets = await Promise.all(
    entries.map(async ([name, reference]) => [name, await fetchAsset(reference.id, reference.bytes)] as const),
  );
  return { ...detail, state: { ...detail.state, ...Object.fromEntries(assets) } };
}

export function ReplayIconButton({
  label,
  icon,
  className = "",
  ...props
}: {
  label: string;
  icon: LucideIcon;
  className?: string;
} & ButtonHTMLAttributes<HTMLButtonElement>) {
  const Icon = icon;
  return (
    <button className={`replay-icon ${className}`} aria-label={label} title={label} {...props}>
      <Icon aria-hidden="true" size={17} strokeWidth={2} />
    </button>
  );
}

/** Run details are fetched once per page view and shared by every replay view. */
const detailCache = new Map<string, Promise<RunDetail>>();

export function fetchRunDetail(id: string) {
  let pending = detailCache.get(id);
  if (!pending) {
    pending = fetch(gatewayUrl(`/v1/runs/${encodeURIComponent(id)}`), { signal: AbortSignal.timeout(20_000) })
      .then(async response => {
        if (!response.ok) throw new Error(`HTTP ${response.status}`);
        const payload = await response.json() as { run: RunDetail };
        return hydrateRunAssets(payload.run);
      });
    pending.catch(() => detailCache.delete(id));
    detailCache.set(id, pending);
  }
  return pending;
}
