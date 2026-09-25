#!/usr/bin/env node
// Bake level covers: render the first frame of each attempt a card shows with
// the console's own renderer, in headless Chrome, and store it in R2 as
// pub/covers/<run>/<attempt>.webp. Cards load these instead of rendering.
//
//   node scripts/bake-covers.mjs --site https://live.benchmark.3720.org \
//     --token-file ~/.config/3720-benchmark/live-ingest-token [--ingest ORIGIN] [--state FILE] [--limit N]
//
// The attempts are every level card's sample and the latest attempt of each
// level in each run (the run page's shelf). An attempt's first frame never
// changes, so a baked cover is kept forever; --state remembers what is done.
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join } from "node:path";
import { chromium } from "@playwright/test";

const args = Object.fromEntries(process.argv.slice(2).reduce((pairs, value, index, all) => (value.startsWith("--") ? [...pairs, [value.slice(2), all[index + 1]]] : pairs), []));
const site = (args.site ?? "https://live.benchmark.3720.org").replace(/\/$/, "");
// Where covers are uploaded; the site by default (a preview renders, production stores).
const ingest = (args.ingest ?? site).replace(/\/$/, "");
const token = readFileSync((args["token-file"] ?? join(homedir(), ".config/3720-benchmark/live-ingest-token")).replace(/^~/, homedir()), "utf8").trim();
const statePath = (args.state ?? join(homedir(), "3720-benchmark/.harbor/live-publish/covers.json")).replace(/^~/, homedir());
const limit = Number(args.limit ?? Infinity);
const BATCH_COUNT = 40, BATCH_BYTES = 4 * 1024 * 1024;

const api = async (path) => {
  const response = await fetch(`${site}/api/live${path}`, { headers: { "user-agent": "3720-cover-baker" } });
  if (!response.ok) throw new Error(`${path}: ${response.status}`);
  return response.json();
};
const baked = new Set((() => { try { return JSON.parse(readFileSync(statePath, "utf8")); } catch { return []; } })());
const save = () => { mkdirSync(dirname(statePath), { recursive: true }); writeFileSync(statePath, JSON.stringify([...baked].sort())); };

// What the pages show.
const wanted = new Map();
const want = (game, run, attempt) => { if (run && attempt != null) wanted.set(`${run}/${attempt}`, { game, run, attempt }); };
const { runs } = await api("/v1/runs");
for (const game of new Set(runs.map(run => run.game))) {
  const { levels } = await api(`/v1/games/${game}/levels`).catch(() => ({ levels: [] }));
  for (const level of levels) want(game, level.sample?.run, level.sample?.attempt);
}
for (const summary of runs) {
  const detail = await api(`/v1/runs/${encodeURIComponent(summary.id)}`);
  for (const group of (detail.run ?? detail).replay_groups ?? []) want(summary.game, summary.id, group.attempts.at(-1)?.id);
}
// A cover may exist though the state file was lost; ask before rendering.
const missing = [];
for (const [key, job] of wanted) {
  if (baked.has(key)) continue;
  const head = await fetch(`${site}/api/live/v1/covers/${key}`, { method: "HEAD" }).catch(() => null);
  if (head?.ok) baked.add(key); else missing.push(job);
}
save();
const todo = missing.sort((a, b) => a.game.localeCompare(b.game)).slice(0, limit);
console.log(`covers: ${wanted.size} shown, ${wanted.size - missing.length} baked, ${todo.length} to bake`);
if (!todo.length) process.exit(0);

const browser = await chromium.launch({ channel: "chrome", args: ["--use-angle=metal", "--enable-gpu", "--ignore-gpu-blocklist"] });
const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
await page.goto(`${site}/?bake=covers`, { waitUntil: "networkidle" });
await page.waitForFunction(() => typeof window.bakeCover === "function", null, { timeout: 30_000 });

let batch = [], bytes = 0, done = 0, failed = 0;
async function flush() {
  if (!batch.length) return;
  const manifest = Buffer.from(JSON.stringify(batch.map(({ key, body }) => ({ key, bytes: body.length, content_type: "image/webp", encoding: null, cache_control: "public, max-age=31536000, immutable" }))));
  const length = Buffer.alloc(4); length.writeUInt32BE(manifest.length);
  const response = await fetch(`${ingest}/api/ingest/objects`, { method: "POST", headers: { authorization: `Bearer ${token}`, "content-type": "application/octet-stream" }, body: Buffer.concat([length, manifest, ...batch.map(item => item.body)]) });
  if (!response.ok) throw new Error(`ingest: ${response.status} ${await response.text()}`);
  for (const item of batch) baked.add(item.id);
  save();
  batch = []; bytes = 0;
}
for (const job of todo) {
  const url = await page.evaluate(({ game, run, attempt }) => window.bakeCover(game, run, attempt), job).catch(() => null);
  if (!url?.startsWith("data:image/webp;base64,")) { failed += 1; continue; }
  const body = Buffer.from(url.slice(url.indexOf(",") + 1), "base64");
  batch.push({ id: `${job.run}/${job.attempt}`, key: `pub/covers/${job.run}/${job.attempt}.webp`, body });
  bytes += body.length; done += 1;
  if (batch.length >= BATCH_COUNT || bytes >= BATCH_BYTES) await flush();
}
await flush();
await browser.close();
console.log(`covers: baked ${done}, failed ${failed}`);
