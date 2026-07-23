#!/usr/bin/env node

import { createHash } from "node:crypto";
import { mkdir, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const output = resolve(root, "data/campaign/sokoban.json");
const sourceOutput = resolve(root, "data/campaign/source.json");
const packs = [
  {
    id: "novoban",
    title: "Novoban",
    difficulty: "Beginner",
    url: "https://sokoban-game.com/packs/novoban/download",
    expected: 50,
  },
  {
    id: "microban",
    title: "Microban",
    difficulty: "Beginner to intermediate",
    url: "https://sokoban-game.com/packs/microban/download",
    expected: 155,
  },
  {
    id: "sasquatch",
    title: "Sasquatch",
    difficulty: "Intermediate",
    url: "https://sokoban-game.com/packs/sasquatch/download",
    expected: 50,
  },
  {
    id: "sasquatch-iii",
    title: "Sasquatch III",
    difficulty: "Intermediate to very hard",
    url: "https://sokoban-game.com/packs/sasquatch-iii/download",
    expected: 50,
  },
];

const tiers = [];
const sources = [];
for (const pack of packs) {
  const response = await fetch(pack.url);
  if (!response.ok) {
    throw new Error(`${pack.url}: HTTP ${response.status}`);
  }
  const bytes = Buffer.from(await response.arrayBuffer());
  const xml = new TextDecoder("iso-8859-1").decode(bytes);
  const levels = parseLevels(xml, pack);
  if (levels.length !== pack.expected) {
    throw new Error(`${pack.id}: expected ${pack.expected} levels, found ${levels.length}`);
  }
  tiers.push({
    id: pack.id,
    title: pack.title,
    difficulty: pack.difficulty,
    source_url: pack.url,
    unlock_after: Math.ceil(levels.length / 2),
    levels,
  });
  sources.push({
    id: pack.id,
    title: pack.title,
    url: pack.url,
    format: "Sokoban Levels Collection (SLC)",
    sha256: createHash("sha256").update(bytes).digest("hex"),
    level_count: levels.length,
  });
}

await mkdir(dirname(output), { recursive: true });
await writeFile(
  output,
  `${JSON.stringify(
    {
      schema: "sokoban-campaign-v1",
      id: "sokoban-classics-v1",
      title: "Sokoban Classics",
      tiers,
    },
    null,
    2,
  )}\n`,
);
await writeFile(
  sourceOutput,
  `${JSON.stringify(
    {
      schema: "sokoban-source-v1",
      imported_on: "2026-07-23",
      source_index: "https://sokoban-game.com/packs",
      sources,
      ordering:
        "Pack order defines benchmark difficulty; level order within each pack is preserved exactly.",
      notes:
        "The benchmark uses the original puzzle layouts and replaces presentation, controls, scoring, and progression with its own implementation.",
    },
    null,
    2,
  )}\n`,
);
console.log(`wrote ${tiers.reduce((sum, tier) => sum + tier.levels.length, 0)} levels to ${output}`);

function parseLevels(xml, pack) {
  const levels = [];
  const pattern = /<Level\b([^>]*)>([\s\S]*?)<\/Level>/gi;
  for (const match of xml.matchAll(pattern)) {
    const attributes = Object.fromEntries(
      [...match[1].matchAll(/([A-Za-z]+)="([^"]*)"/g)].map((attribute) => [
        attribute[1],
        decodeXml(attribute[2]),
      ]),
    );
    const rows = [...match[2].matchAll(/<L>([\s\S]*?)<\/L>/gi)].map((row) =>
      decodeXml(row[1]),
    );
    const width = Number(attributes.Width);
    const height = Number(attributes.Height);
    if (!Number.isInteger(width) || !Number.isInteger(height) || rows.length !== height) {
      throw new Error(`${pack.id}/${attributes.Id}: invalid dimensions`);
    }
    const padded = rows.map((row) => row.padEnd(width, " "));
    if (padded.some((row) => [...row].length !== width)) {
      throw new Error(`${pack.id}/${attributes.Id}: row exceeds declared width`);
    }
    levels.push({
      id: `${pack.id}-${String(levels.length + 1).padStart(3, "0")}`,
      source_id: attributes.Id || String(levels.length + 1),
      title: `${pack.title} ${levels.length + 1}`,
      width,
      height,
      rows: padded,
    });
  }
  return levels;
}

function decodeXml(value) {
  return value
    .replaceAll("&lt;", "<")
    .replaceAll("&gt;", ">")
    .replaceAll("&quot;", '"')
    .replaceAll("&apos;", "'")
    .replaceAll("&amp;", "&");
}
