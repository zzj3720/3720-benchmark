import type { ComponentType } from "react";

export type Json = null | boolean | number | string | Json[] | { [key: string]: Json };
export type GameState = Record<string, Json>;
export type ObserverEvent = {
  sequence: number;
  timestamp_ms?: number | null;
  type?: string;
  action?: Record<string, Json> | null;
  state?: GameState | null;
  result?: Record<string, Json> | null;
  score?: number;
  score_delta?: number;
};
export type EventDescription = {
  label: string;
  title: string;
  detail: string;
  tone?: "neutral" | "success" | "warning";
};

export type GameObserverModule<Game extends string = string> = {
  id: Game;
  meta: {
    label: string;
    short: string;
    accent: string;
  };
  State: ComponentType<{ state: GameState; previousState?: GameState | null }>;
  describeEvent?: (event: ObserverEvent, previous?: ObserverEvent | null) => EventDescription;
};

export function asRecord(value: Json | undefined): GameState | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as GameState)
    : null;
}

export function asNumber(value: Json | undefined, fallback = 0) {
  return typeof value === "number" ? value : fallback;
}

export function asString(value: Json | undefined, fallback = "—") {
  return typeof value === "string" ? value : fallback;
}

export function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div className="metric">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

export function EmptyState({ title, body }: { title: string; body: string }) {
  return (
    <div className="empty-state">
      <strong>{title}</strong>
      <p>{body}</p>
    </div>
  );
}
