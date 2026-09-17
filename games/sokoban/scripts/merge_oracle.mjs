#!/usr/bin/env node

import { readFile, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const gameRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const festivalPath = process.argv[2];
if (!festivalPath) {
  throw new Error("usage: merge_oracle.mjs FESTIVAL_NOVOBAN_SOLUTIONS.sok");
}

const festival = await readFile(festivalPath, "utf8");
const ksokoban = await readFile(resolve(gameRoot, "data/oracle/ksokoban.tsv"), "utf8");
const novoban = [];
const pattern = /^Level (\d+)\n[\s\S]*?^Solution\n([UDLRudlr]+)$/gm;
for (const match of festival.matchAll(pattern)) {
  novoban.push({
    number: Number(match[1]),
    moves: match[2].toLowerCase(),
  });
}
novoban.sort((left, right) => left.number - right.number);
if (
  novoban.length !== 50 ||
  novoban.some((solution, index) => solution.number !== index + 1)
) {
  throw new Error(`expected Festival solutions for Novoban 1..50, found ${novoban.length}`);
}

const lines = [
  ...novoban.map(
    ({ number, moves }) => `novoban-${String(number).padStart(3, "0")}\t${moves}`,
  ),
  ...ksokoban.trim().split("\n"),
];
if (lines.length !== 305) throw new Error(`expected 305 solutions, found ${lines.length}`);

await writeFile(resolve(gameRoot, "data/oracle/solutions.tsv"), `${lines.join("\n")}\n`);
await writeFile(
  resolve(gameRoot, "data/oracle/source.json"),
  `${JSON.stringify(
    {
      schema: "sokoban-oracle-source-v1",
      generated_on: "2026-07-23",
      level_count: lines.length,
      sources: [
        {
          campaign_scope: ["novoban"],
          generator: "Festival 3.1",
          source: "https://sourceforge.net/projects/festival3os/",
          method:
            "Solutions generated locally with a 600 second per-level limit and eight solver cores.",
        },
        {
          campaign_scope: ["microban", "sasquatch", "sasquatch-iii"],
          source: "https://ksokoban.online/",
          solution_bundles:
            "https://ksokoban.online/solutions/bunch10_0.js through bunch10_38.js",
          method:
            "Public coordinate-and-push records expanded into ordinary LURD moves after exact layout comparison.",
        },
      ],
      verification:
        "Every expanded LURD sequence is replayed by the benchmark's Rust rules engine before packaging.",
    },
    null,
    2,
  )}\n`,
);
console.log(`wrote ${lines.length} merged solutions`);
