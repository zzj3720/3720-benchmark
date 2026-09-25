import type { GameId } from "./game-registry";
import type { Json, ObserverEvent } from "./game-observer";

export type ScorePoint = { timestamp_ms: number; elapsed_ms?: number; score: number };
export type ElapsedScorePoint = { elapsed_ms: number; score: number };

export type RunSummary = {
  id: string;
  job: string;
  trial: string;
  task_id: string;
  task: string;
  game: GameId;
  model: string;
  model_id: string;
  agent: string;
  effort: string;
  live: boolean;
  status: string;
  termination: {
    kind:
      | "live"
      | "orphaned"
      | "resumable"
      | "agent_stopped"
      | "completed"
      | "stopped"
      | "no_agent";
    resumable: boolean;
    reason?: string | null;
  };
  sidecar_only: boolean;
  score: number;
  total: number;
  objective: string;
  started_at?: number | null;
  finished_at?: number | null;
  last_activity_at?: number | null;
  last_score_at?: number | null;
  last_score_elapsed_ms?: number | null;
  consumed_ms?: number;
  observed_at?: number;
  latest_sequence: number;
  detail_revision?: string;
  execution?: { active: boolean; anchor_timestamp_ms: number | null; anchor_elapsed_ms: number | null };
  latest_action?: Record<string, Json> | null;
  latest_result?: Record<string, Json> | null;
  score_history?: ScorePoint[];
};

export type RunDetail = RunSummary & {
  state: Record<string, Json>;
  state_revision?: string;
  asset_refs?: Record<string, ObserverAssetReference>;
  replay_groups: ReplayGroupSummary[];
  replay_catalog_more?: boolean;
  replay_catalog_before?: number | null;
  recent_activity?: { timestamp_ms: number; text: string }[];
  live_replay?: {
    sequence: number;
    frames: ReplayFrame[];
    operations: ReplayOperation[];
  } | null;
  agent_experience: {
    updated_at?: number | null;
    source_count: number;
    counts: Record<"plan" | "verified" | "rejected" | "solved", number>;
    plan: string[];
    verified: string[];
    rejected: string[];
    solved: string[];
  };
};

export type ReplayFrame = {
  key: string;
  event: ObserverEvent;
  operation_sequence: number;
  operation_timestamp_ms?: number | null;
  operation_action: Record<string, Json>;
  instruction_index: number;
  instruction_count: number;
  operation_size: number;
  has_instruction_trace: boolean;
};

export type ReplayOperation = {
  sequence: number;
  timestamp_ms?: number | null;
  action: Record<string, Json>;
  first_frame_key: string;
  last_frame_key: string;
  frame_count: number;
  score_delta: number;
};

export type ReplayAttemptSummary = {
  id: number;
  successful: boolean;
  status?: "running" | "completed" | "failed";
  score?: number | null;
};

export type ReplayGroupSummary = {
  kind: "level" | "overworld" | "shift" | "world";
  reference: string;
  title?: string | null;
  score?: number | null;
  attempts: ReplayAttemptSummary[];
};

export type LoadedAttemptReplay = {
  attempt_id: number;
  next_after_sequence?: number | null;
  page_after_sequence?: number | null;
  kind: "level" | "overworld" | "shift" | "world";
  reference?: string | null;
  title?: string | null;
  score: number;
  successful: boolean;
  asset_refs?: Record<string, ObserverAssetReference>;
  frames: ReplayFrame[];
  operations: ReplayOperation[];
  activity: { timestamp_ms: number; text: string }[];
  skipped_unchanged: number;
  eliminated_history_frames: number;
};

export type ObserverAssetReference = {
  id: string;
  media_type: string;
  bytes: number;
};
