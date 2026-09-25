/** One in-flight request per resource. Revisions coalesce while work is pending;
 * disposal invalidates late results even when the transport ignores abort. */
export class LiveResource<T> {
  private desired: string | null = null;
  private loaded: string | null = null;
  private pending = false;
  private disposed = false;
  private failures = 0;
  private controller: AbortController | null = null;
  private retryTimer: ReturnType<typeof setTimeout> | null = null;

  private options: {
    load: (signal: AbortSignal) => Promise<T>;
    commit: (value: T) => void;
    status: (state: "loading" | "ready" | "error", error?: unknown) => void;
    retryDelayMs?: number;
    timeoutMs?: number;
  };
  constructor(options: LiveResource<T>["options"]) { this.options = options; }

  request(revision: string) {
    if (this.disposed) return;
    if (revision !== this.desired) {
      this.failures = 0;
      this.desired = revision;
      if (this.retryTimer !== null) { clearTimeout(this.retryTimer); this.retryTimer = null; }
    }
    void this.pump();
  }

  retry() {
    this.failures = 0;
    this.loaded = null;
    if (this.retryTimer !== null) { clearTimeout(this.retryTimer); this.retryTimer = null; }
    void this.pump();
  }

  dispose() {
    this.disposed = true;
    this.controller?.abort();
    if (this.retryTimer !== null) clearTimeout(this.retryTimer);
  }

  private async pump() {
    if (this.disposed || this.pending || this.retryTimer !== null || this.failures >= 6 || this.desired === null || this.loaded === this.desired) return;
    const revision = this.desired;
    const controller = new AbortController();
    this.controller = controller;
    this.pending = true;
    this.options.status("loading");
    const timeout = setTimeout(() => controller.abort(new Error("请求超时")), this.options.timeoutMs ?? 15_000);
    try {
      const value = await abortable(this.options.load(controller.signal), controller.signal);
      if (this.disposed || controller.signal.aborted) return;
      this.loaded = revision;
      this.failures = 0;
      this.options.commit(value);
      this.options.status("ready");
    } catch (error) {
      if (this.disposed) return;
      this.failures += 1;
      this.options.status("error", error);
      if (this.failures < 6) {
        const delay = Math.min(5_000, (this.options.retryDelayMs ?? 500) * 2 ** (this.failures - 1));
        this.retryTimer = setTimeout(() => { this.retryTimer = null; void this.pump(); }, delay);
      }
    } finally {
      clearTimeout(timeout);
      this.pending = false;
      if (!this.disposed && this.loaded !== this.desired && this.retryTimer === null && this.failures === 0) void this.pump();
    }
  }
}

function abortable<T>(work: Promise<T>, signal: AbortSignal): Promise<T> {
  return new Promise((resolve, reject) => {
    const abort = () => reject(signal.reason ?? new Error("请求已取消"));
    if (signal.aborted) { abort(); return; }
    signal.addEventListener("abort", abort, { once: true });
    work.then(resolve, reject).finally(() => signal.removeEventListener("abort", abort));
  });
}

export class RequestGate {
  private generation = 0;
  private controller: AbortController | null = null;
  cancel() { this.generation += 1; this.controller?.abort(); this.controller = null; }
  begin() {
    this.cancel();
    const generation = this.generation;
    const controller = new AbortController();
    this.controller = controller;
    return { signal: controller.signal, current: () => generation === this.generation && !controller.signal.aborted };
  }
}

type LiveRecord = { id: string; latest_sequence: number; score_history?: unknown[]; score_history_delta?: unknown[] };

/** Validate before touching the last good snapshot. V1 remains readable while
 * the gateway and browser are rolled out independently. */
export function applySubscription<T extends LiveRecord>(previous: T[], message: unknown): { runs: T[]; generatedAt: number } {
  if (!message || typeof message !== "object") throw new Error("直播消息格式无效");
  const payload = message as Record<string, unknown>;
  if (payload.error) throw new Error("直播数据暂时不可用");
  if (!Array.isArray(payload.runs) || typeof payload.generated_at !== "number") throw new Error("直播消息缺少运行快照");
  if (payload.schema !== "benchmark-live-subscription-v1" && payload.schema !== "benchmark-live-subscription-v2") throw new Error("不支持的直播协议版本");
  const delta = payload.schema === "benchmark-live-subscription-v2";
  if (delta && (typeof payload.reset !== "boolean" || !Array.isArray(payload.removed) || !payload.removed.every(id => typeof id === "string"))) throw new Error("直播增量格式无效");
  const records = new Map<string, T>((!delta || payload.reset ? [] : previous).map(run => [run.id, run]));
  for (const raw of payload.runs) {
    if (!raw || typeof raw !== "object" || typeof raw.id !== "string" || typeof raw.latest_sequence !== "number") throw new Error("直播运行记录格式无效");
    const before = records.get(raw.id);
    const history = raw.score_history ?? [...(before?.score_history ?? []), ...(raw.score_history_delta ?? [])];
    if (!Array.isArray(history)) throw new Error("分数历史格式无效");
    records.set(raw.id, { ...raw, score_history: history });
  }
  if (delta) for (const id of payload.removed as string[]) records.delete(id);
  return { runs: [...records.values()], generatedAt: payload.generated_at };
}

export function effectiveDuration(run: {
  consumed_ms?: number;
  live: boolean;
  execution?: { active: boolean; anchor_timestamp_ms: number | null; anchor_elapsed_ms: number | null };
}, now: number) {
  const execution = run.execution;
  if (run.live && execution?.active && execution.anchor_timestamp_ms !== null && execution.anchor_elapsed_ms !== null) {
    return Math.max(run.consumed_ms ?? 0, execution.anchor_elapsed_ms + Math.max(0, now - execution.anchor_timestamp_ms));
  }
  return run.consumed_ms ?? 0;
}
