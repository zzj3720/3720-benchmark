import { asNumber, asRecord, asString, type Json, type GameState } from "../game-observer";
export { asNumber as num, asRecord as rec, asString as str };
export function records(value: Json | undefined): GameState[] { return Array.isArray(value) ? value.map(asRecord).filter((item): item is GameState => item !== null) : []; }
export function strings(value: Json | undefined): string[] { return Array.isArray(value) ? value.filter((item): item is string => typeof item === "string") : []; }
export function clock(milliseconds: number) { const seconds = Math.max(0, Math.ceil(milliseconds / 1000)); return `${String(Math.floor(seconds / 60)).padStart(2, "0")}:${String(seconds % 60).padStart(2, "0")}`; }
export function friendly(value: string) { return value.replace(/([a-z0-9])([A-Z])/g, "$1 $2").replaceAll("_", " ").replace(/\s+/g, " ").trim(); }
export function point(value: Json | undefined) { const row = asRecord(value); return row ? { x: asNumber(row.x), y: asNumber(row.y), z: asNumber(row.z) } : null; }
