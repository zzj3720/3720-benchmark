// Pure state for the live hub: run summaries, their order, and whether the
// local publisher is still reachable. The Durable Object persists and
// broadcasts what this computes; keeping it free of Cloudflare APIs lets the
// subscription protocol be tested under plain Node.

export type Json = null | boolean | number | string | Json[] | { [key: string]: Json };
export type Run = { id: string; score_history?: Json[]; detail_revision?: Json; [key: string]: Json | undefined };

export type Feed = {
  connected: boolean;
  last_seen_ms: number | null;
  publisher: string | null;
};

export type IngestRuns = {
  order: string[];
  runs: Run[];
  removed: string[];
};

/** A publisher that has not heartbeated for this long is treated as gone. */
export const FEED_TIMEOUT_MS = 45_000;

export class HubState {
  runs = new Map<string, Run>();
  order: string[] = [];
  revision = 0;
  feed: Feed = { connected: false, last_seen_ms: null, publisher: null };

  /** Apply a publisher batch; returns the protocol-v2 update, or null if nothing changed. */
  applyRuns(message: IngestRuns, now: number) {
    const updates: Run[] = [];
    for (const run of message.runs) {
      const previous = this.runs.get(run.id);
      const update: Run = { ...run };
      const before = previous?.score_history;
      const after = run.score_history;
      if (Array.isArray(before) && Array.isArray(after) && after.length >= before.length
        && JSON.stringify(after.slice(0, before.length)) === JSON.stringify(before)) {
        delete update.score_history;
        update.score_history_delta = after.slice(before.length);
      }
      this.runs.set(run.id, run);
      updates.push(update);
    }
    // `order` is the publisher's complete run list: a run deleted on the
    // benchmark host disappears here even if this publisher never sent it.
    const listed = new Set(message.order);
    const removed = [...new Set([...message.removed, ...this.runs.keys()])]
      .filter(id => !listed.has(id) && this.runs.delete(id));
    this.order = message.order.filter(id => this.runs.has(id));
    this.seen(now);
    if (!updates.length && !removed.length) return null;
    this.revision += 1;
    return { runs: updates, removed };
  }

  /** Record a heartbeat; returns true when this reconnects a lost feed. */
  heartbeat(publisher: string, now: number) {
    const reconnected = !this.feed.connected;
    this.feed = { connected: true, last_seen_ms: now, publisher };
    if (reconnected) this.revision += 1;
    return reconnected;
  }

  /** Returns true when the feed has just gone stale. */
  expire(now: number) {
    if (!this.feed.connected || this.feed.last_seen_ms === null) return false;
    if (now - this.feed.last_seen_ms <= FEED_TIMEOUT_MS) return false;
    this.feed = { ...this.feed, connected: false };
    this.revision += 1;
    return true;
  }

  ordered() {
    return this.order.map(id => this.runs.get(id)).filter((run): run is Run => run !== undefined);
  }

  /** The first message a new subscriber receives. */
  snapshot(selected: string | null, now: number) {
    return this.message({ runs: this.ordered(), removed: [] }, selected, now, true);
  }

  message(update: { runs: Run[]; removed: string[] }, selected: string | null, now: number, reset = false) {
    const run = selected ? this.runs.get(selected) : undefined;
    return {
      schema: "benchmark-live-subscription-v2",
      revision: this.revision,
      generated_at: now,
      reset,
      runs: update.runs,
      removed: update.removed,
      selected: selected ? { id: selected, revision: run?.detail_revision ?? null } : null,
      feed: this.feed,
    };
  }

  runsResponse(now: number) {
    return { schema: "benchmark-live-runs-v1", generated_at: now, runs: this.ordered(), feed: this.feed };
  }

  private seen(now: number) {
    if (this.feed.connected) this.feed = { ...this.feed, last_seen_ms: now };
  }
}
