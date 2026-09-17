#!/usr/bin/env node

import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const gameRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const campaign = JSON.parse(
  await readFile(resolve(gameRoot, "data/campaign/sokoban.json"), "utf8"),
);
const output = resolve(gameRoot, "data/oracle/ksokoban.tsv");
const bundleBase = "https://ksokoban.online/solutions";
const solutionStrings = new Map();

for (let index = 0; index <= 38; index += 1) {
  const url = `${bundleBase}/bunch10_${index}.js`;
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(`${url}: HTTP ${response.status}`);
  }
  const source = await response.text();
  for (const match of source.matchAll(
    /window\.SokobanSolutions\["([^"]+)"\]\s*=\s*"([^"]+)";/g,
  )) {
    solutionStrings.set(match[1], match[2]);
  }
}

const lines = [];
for (const tier of campaign.tiers.filter((tier) => tier.id !== "novoban")) {
  await verifyLevelSet(tier);
  for (const level of tier.levels) {
    const key = `${tier.title}#${level.number ?? Number(level.id.slice(-3))}`;
    const encoded = solutionStrings.get(key);
    if (!encoded) {
      throw new Error(`missing public solution: ${key}`);
    }
    lines.push(`${level.id}\t${decodeSolution(level, encoded)}`);
  }
}

await mkdir(dirname(output), { recursive: true });
await writeFile(output, `${lines.join("\n")}\n`);
console.log(`wrote ${lines.length} decoded solutions to ${output}`);

async function verifyLevelSet(tier) {
  const url = `https://ksokoban.online/levels/${encodeURIComponent(tier.title)}.js`;
  const response = await fetch(url);
  if (!response.ok) throw new Error(`${url}: HTTP ${response.status}`);
  const source = await response.text();
  const assignment = source.match(
    new RegExp(
      `window\\.SokobanLevels\\[${JSON.stringify(tier.title)}\\]\\s*=\\s*(\\[[\\s\\S]*\\]);?`,
    ),
  );
  if (!assignment) throw new Error(`${tier.id}: missing public level array`);
  const publicLevels = JSON.parse(assignment[1]);
  for (let index = 0; index < tier.levels.length; index += 1) {
    const expected = tier.levels[index].rows.map((row) => row.trimEnd());
    const actual = publicLevels[index]?.split("|");
    if (JSON.stringify(actual) !== JSON.stringify(expected)) {
      throw new Error(`${tier.id}-${index + 1}: public level layout differs from campaign`);
    }
  }
}

function decodeSolution(level, encoded) {
  const board = level.rows.map((row) => [...row]);
  let player;
  const boxes = new Set();
  const goals = new Set();
  for (let y = 0; y < board.length; y += 1) {
    for (let x = 0; x < board[y].length; x += 1) {
      const tile = board[y][x];
      if (tile === "@" || tile === "+") player = { x, y };
      if (tile === "$" || tile === "*") boxes.add(position(x, y));
      if (tile === "." || tile === "*" || tile === "+") goals.add(position(x, y));
    }
  }
  if (!player) throw new Error(`${level.id}: missing player`);

  let moves = "";
  const terms = encoded.replace(/(\d)([lrud])/g, "$1,$2").split(",");
  for (const term of terms) {
    const destination = term.match(/^(\d+)-(\d+)$/);
    if (destination) {
      const target = { x: Number(destination[1]), y: Number(destination[2]) };
      const path = findWalk(board, boxes, player, target, level.id);
      for (const direction of path) player = movePlayer(player, direction);
      moves += path.join("");
      continue;
    }
    const run = term.match(/^([lrud])(\d+)$/);
    if (!run) throw new Error(`${level.id}: invalid solution term ${term}`);
    for (let count = Number(run[2]); count > 0; count -= 1) {
      const direction = run[1];
      const next = movePlayer(player, direction);
      const nextKey = position(next.x, next.y);
      if (boxes.has(nextKey)) {
        const beyond = movePlayer(next, direction);
        const beyondKey = position(beyond.x, beyond.y);
        if (isWall(board, beyond.x, beyond.y) || boxes.has(beyondKey)) {
          throw new Error(`${level.id}: illegal push at ${player.x}-${player.y}`);
        }
        boxes.delete(nextKey);
        boxes.add(beyondKey);
      } else if (isWall(board, next.x, next.y)) {
        throw new Error(`${level.id}: illegal walk at ${player.x}-${player.y}`);
      }
      player = next;
      moves += direction;
    }
  }
  if (boxes.size !== goals.size || [...boxes].some((box) => !goals.has(box))) {
    throw new Error(`${level.id}: decoded solution does not solve the level`);
  }
  return moves;
}

function findWalk(board, boxes, start, target, levelId) {
  const startKey = position(start.x, start.y);
  const targetKey = position(target.x, target.y);
  const queue = [start];
  const previous = new Map([[startKey, null]]);
  const previousDirection = new Map();
  for (let index = 0; index < queue.length; index += 1) {
    const current = queue[index];
    if (position(current.x, current.y) === targetKey) break;
    for (const direction of ["u", "l", "d", "r"]) {
      const next = movePlayer(current, direction);
      const key = position(next.x, next.y);
      if (
        previous.has(key) ||
        boxes.has(key) ||
        isWall(board, next.x, next.y)
      ) {
        continue;
      }
      previous.set(key, position(current.x, current.y));
      previousDirection.set(key, direction);
      queue.push(next);
    }
  }
  if (!previous.has(targetKey)) {
    throw new Error(`${levelId}: cannot walk to ${target.x}-${target.y}`);
  }
  const path = [];
  for (let key = targetKey; key !== startKey; key = previous.get(key)) {
    path.push(previousDirection.get(key));
  }
  return path.reverse();
}

function movePlayer(player, direction) {
  const delta = {
    u: [0, -1],
    d: [0, 1],
    l: [-1, 0],
    r: [1, 0],
  }[direction];
  return { x: player.x + delta[0], y: player.y + delta[1] };
}

function isWall(board, x, y) {
  return y < 0 || y >= board.length || x < 0 || x >= board[y].length || board[y][x] === "#";
}

function position(x, y) {
  return `${x},${y}`;
}
