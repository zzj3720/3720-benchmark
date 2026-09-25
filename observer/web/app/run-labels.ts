// How runs are named, described and ordered for viewers. Raw model ids,
// agent class paths and termination kinds are operator vocabulary; the
// console shows these instead.

type LabelledRun = {
  id: string;
  model: string;
  agent?: string;
  effort?: string;
  live: boolean;
  score: number;
  started_at?: number | null;
  consumed_ms?: number;
  termination?: { kind: string; reason?: string | null } | null;
};

const TOKEN_CASE: Record<string, string> = {
  gpt: "GPT",
  deepseek: "DeepSeek",
  kimi: "Kimi",
  glm: "GLM",
  qwen: "Qwen",
  gemini: "Gemini",
  claude: "Claude",
};

/** `openai/gpt-5.6-luna` → `GPT-5.6 Luna`; `deepseek/deepseek-v4.1-flash-expires-on-0910` → `DeepSeek V4.1 Flash`. */
export function modelName(model: string) {
  const bare = model.split("/").pop()!.replace(/-expires-on-\d+$/i, "").replace(/\[.*\]$/, "");
  const tokens = bare.split(/[-_\s]+/).filter(Boolean);
  const words: string[] = [];
  for (const token of tokens) {
    const lower = token.toLowerCase();
    if (TOKEN_CASE[lower]) words.push(TOKEN_CASE[lower]);
    else if (/^v\d/.test(lower)) words.push(lower.toUpperCase());
    else if (/^\d/.test(token)) {
      // A version directly after a family name joins it: GPT-5.6, Qwen3.8.
      const previous = words.pop();
      words.push(previous === "GPT" ? `GPT-${token}` : previous ? `${previous} ${token}` : token);
    } else words.push(token[0].toUpperCase() + token.slice(1));
  }
  return words.join(" ") || model;
}

const FAMILIES: [RegExp, string][] = [
  [/(^|[/-])(gpt|o\d|codex)([-.\d]|$)|^openai\//i, "OpenAI"],
  [/claude|^anthropic\//i, "Anthropic"],
  [/deepseek/i, "DeepSeek"],
  [/gemini|^google\//i, "Google"],
  [/kimi|moonshot/i, "Moonshot"],
  [/glm|zhipu|z-ai/i, "Zhipu"],
  [/qwen|alibaba/i, "Qwen"],
  [/grok|^x-ai\//i, "xAI"],
];

/** The lab behind a model id: `openai/gpt-5.6-sol` → `OpenAI`. Unlisted or stealth models are `其他`. */
export function modelFamily(model: string) {
  const bare = model.toLowerCase();
  return FAMILIES.find(([pattern]) => pattern.test(bare))?.[1] ?? "其他";
}

/**
 * Release dates (YYYY-MM-DD) by model name as `modelName` prints it. Within a
 * family, newer models are listed first; a model missing here is placed by
 * its version number, then by when it was first benchmarked. First public
 * availability, previews included.
 */
export const MODEL_RELEASES: Record<string, string> = {
  "GPT-6 Astra": "2026-09-03",
  "GPT-5.6 Sol": "2026-06-26",
  "GPT-5.6 Terra": "2026-06-26",
  "GPT-5.6 Luna": "2026-06-26",
  "DeepSeek V4.1 Flash": "2026-09-10",
  "DeepSeek V4 Flash": "2026-04-24",
};

/**
 * Relative size of models released together, larger first: OpenAI lists
 * GPT-5.6 as Luna < Terra < Sol, which their prices follow too.
 */
export const MODEL_SIZES: Record<string, number> = {
  "GPT-5.6 Sol": 3,
  "GPT-5.6 Terra": 2,
  "GPT-5.6 Luna": 1,
};

function version(name: string) {
  return name.match(/\d+(?:\.\d+)*/)?.[0].split(".").map(Number) ?? [];
}

/** Negative when model `a` is newer than `b`. `firstSeen` is when each was first benchmarked. */
export function compareModels(a: string, b: string, firstSeen: Map<string, number> = new Map()) {
  const [da, db] = [MODEL_RELEASES[a], MODEL_RELEASES[b]];
  if (da && db && da !== db) return db.localeCompare(da);
  if (da && db && (MODEL_SIZES[a] ?? 0) !== (MODEL_SIZES[b] ?? 0)) return (MODEL_SIZES[b] ?? 0) - (MODEL_SIZES[a] ?? 0);
  const [va, vb] = [version(a), version(b)];
  for (let i = 0; i < Math.max(va.length, vb.length); i++) {
    if ((va[i] ?? 0) !== (vb[i] ?? 0)) return (vb[i] ?? 0) - (va[i] ?? 0);
  }
  return (firstSeen.get(b) ?? 0) - (firstSeen.get(a) ?? 0) || a.localeCompare(b);
}

/** When each model was first benchmarked, from its runs' start times. */
export function firstSeen(runs: { model: string; started_at?: number | null }[]) {
  const seen = new Map<string, number>();
  for (const run of runs) {
    const name = modelName(run.model);
    seen.set(name, Math.min(seen.get(name) ?? Infinity, run.started_at ?? Infinity));
  }
  return seen;
}

/**
 * Runs grouped by model family. Families lead with the best score among
 * their runs ("其他" last); inside a family the newest model comes first,
 * and runs of one model keep their order in `runs`.
 */
export function groupByFamily<T extends LabelledRun>(runs: T[]) {
  const seen = firstSeen(runs);
  const position = new Map(runs.map((run, index) => [run.id, index]));
  const families = new Map<string, T[]>();
  for (const run of runs) families.set(modelFamily(run.model), [...(families.get(modelFamily(run.model)) ?? []), run]);
  return [...families]
    .map(([family, members]) => ({
      family,
      runs: members.sort((a, b) => compareModels(modelName(a.model), modelName(b.model), seen) || position.get(a.id)! - position.get(b.id)!),
    }))
    .sort((a, b) => Number(a.family === "其他") - Number(b.family === "其他") || Math.max(...b.runs.map(run => run.score)) - Math.max(...a.runs.map(run => run.score)));
}

/** Consecutive runs of one model (as `groupByFamily` orders them), e.g. its thinking depths. */
export function groupByModel<T extends LabelledRun>(runs: T[]) {
  const groups: { model: string; runs: T[] }[] = [];
  for (const run of runs) {
    const model = modelName(run.model), last = groups.at(-1);
    if (last?.model === model) last.runs.push(run);
    else groups.push({ model, runs: [run] });
  }
  return groups;
}

/** What tells a run apart inside its model's group: `max`, `xhigh · Codex`, or `默认`. */
export function subLabel(run: LabelledRun, label: string | undefined) {
  const model = modelName(run.model);
  const rest = label?.startsWith(`${model} · `) ? label.slice(model.length + 3) : "";
  return rest || "默认";
}

/** The harness driving the model, from the agent import path. */
export function harnessName(agent = "") {
  const module = agent.split(":")[0].split(".").pop() ?? "";
  if (module.includes("codex")) return "Codex";
  if (module.includes("claude_code")) return "Claude Code";
  if (module.startsWith("pi") || module.endsWith("_pi") || module === "deepseek41_pi") return "Pi";
  if (module.includes("qoder")) return "Qoder";
  if (module.includes("opencode")) return "OpenCode";
  if (agent === "terminus-2") return "Terminus";
  return module ? module.replace(/_/g, " ") : "";
}

const EFFORT_NAMES: Record<string, string> = { default: "", low: "low", medium: "medium", high: "high", xhigh: "xhigh", max: "max" };

/** `GPT-5.6 Luna · max`, extended with the harness or start date only when needed to tell runs apart. */
export function runLabels(runs: LabelledRun[]) {
  const base = new Map(runs.map(run => {
    const effort = EFFORT_NAMES[run.effort ?? "default"] ?? run.effort ?? "";
    return [run.id, [modelName(run.model), effort].filter(Boolean).join(" · ")];
  }));
  const labels = new Map<string, string>();
  const groups = new Map<string, LabelledRun[]>();
  for (const run of runs) groups.set(base.get(run.id)!, [...(groups.get(base.get(run.id)!) ?? []), run]);
  for (const [label, group] of groups) {
    const harnesses = new Set(group.map(run => harnessName(run.agent)));
    for (const run of group) {
      if (group.length === 1) labels.set(run.id, label);
      else if (harnesses.size === group.length) labels.set(run.id, `${label} · ${harnessName(run.agent)}`);
      else labels.set(run.id, `${label} · ${startDate(run.started_at)}`);
    }
  }
  return labels;
}

function startDate(timestamp?: number | null) {
  if (!timestamp) return "—";
  return new Intl.DateTimeFormat("zh-CN", { month: "numeric", day: "numeric" }).format(timestamp);
}

export type ViewerStatus = { label: string; tone: "live" | "done" | "ended" | "waiting"; detail: string };

/** Three states a viewer needs, with the operator reason as detail. */
export function viewerStatus(run: LabelledRun): ViewerStatus {
  const kind = run.termination?.kind ?? (run.live ? "live" : "stopped");
  const reason = run.termination?.reason ?? "";
  switch (kind) {
    case "live":
      return { label: "直播中", tone: "live", detail: "正在运行" };
    case "completed":
      return { label: "已完赛", tone: "done", detail: "游戏打到了终局" };
    case "no_agent":
      return { label: "等待 Agent", tone: "waiting", detail: "游戏已就绪，还没有 Agent 接入" };
    case "agent_stopped":
      return { label: "已结束", tone: "ended", detail: "模型主动结束" };
    case "stopped":
      return { label: "已结束", tone: "ended", detail: reason === "cancelled" ? "人工停止" : "已停止" };
    case "orphaned":
      return { label: "已结束", tone: "ended", detail: "记录意外中断" };
    default:
      return { label: "已结束", tone: "ended", detail: reason ? `因${translateReason(reason)}中止` : "运行中止" };
  }
}

function translateReason(reason: string) {
  if (/infrastructure/i.test(reason)) return "基础设施故障";
  if (/cancel/i.test(reason)) return "人工停止";
  return reason;
}

/** Live first, then highest score, then the faster run. */
export function rankRuns<T extends LabelledRun>(runs: T[]) {
  return runs.slice().sort((a, b) =>
    Number(b.live) - Number(a.live) || b.score - a.score || (a.consumed_ms ?? 0) - (b.consumed_ms ?? 0) || a.id.localeCompare(b.id));
}

/** Eight hues that stay distinguishable on the dark console, assigned by rank. */
export const SERIES_COLORS = ["#6cb6ff", "#ffa24c", "#c49bff", "#7ee07a", "#ff7eb6", "#f2d45c", "#4fd6e0", "#ff6b5b"];

export function seriesColors(ranked: { id: string }[]) {
  return new Map(ranked.map((run, index) => [run.id, SERIES_COLORS[index % SERIES_COLORS.length]]));
}
