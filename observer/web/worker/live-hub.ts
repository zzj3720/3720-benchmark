import { HubState, type IngestRuns, type Run } from "./live-hub-state";

// One instance holds every run summary and fans updates out to all open
// Server-Sent Events streams. It keeps serving the last state it received
// when the local publisher disappears, and tells viewers the feed is offline.
// A plain class (no `cloudflare:workers` import) keeps the Worker bundle
// loadable under Node for server-rendering tests.

const KEEPALIVE_MS = 15_000;
/** Messages a viewer may lag behind before its stream is closed. */
const MAX_BACKLOG = 64;

type Subscriber = { writer: WritableStreamDefaultWriter<Uint8Array>; selected: string | null };

export class LiveHub {
  private state = new HubState();
  private subscribers = new Set<Subscriber>();
  private encoder = new TextEncoder();
  private ctx: DurableObjectState;

  constructor(ctx: DurableObjectState) {
    this.ctx = ctx;
    ctx.blockConcurrencyWhile(async () => {
      const sql = ctx.storage.sql;
      sql.exec("CREATE TABLE IF NOT EXISTS runs (id TEXT PRIMARY KEY, body TEXT NOT NULL)");
      sql.exec("CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL)");
      for (const row of sql.exec<{ id: string; body: string }>("SELECT id, body FROM runs")) {
        this.state.runs.set(row.id, JSON.parse(row.body) as Run);
      }
      const meta = Object.fromEntries(
        [...sql.exec<{ key: string; value: string }>("SELECT key, value FROM meta")].map(row => [row.key, JSON.parse(row.value)]),
      );
      this.state.order = Array.isArray(meta.order) ? meta.order : [...this.state.runs.keys()];
      this.state.revision = typeof meta.revision === "number" ? meta.revision : 0;
      // A restarted object cannot know whether the publisher is still there
      // until the next heartbeat arrives.
      if (meta.feed) this.state.feed = { ...meta.feed, connected: false };
    });
  }

  async fetch(request: Request) {
    const url = new URL(request.url);
    const now = Date.now();
    switch (`${request.method} ${url.pathname}`) {
      case "GET /runs":
        return Response.json(this.state.runsResponse(now), { headers: { "cache-control": "no-store" } });
      case "GET /health":
        return Response.json({ ok: true, runs: this.state.runs.size, revision: this.state.revision, feed: this.state.feed, subscribers: this.subscribers.size });
      case "GET /subscribe":
        return this.subscribe(url.searchParams.get("run_id"), now);
      case "POST /runs":
        return this.ingestRuns(await request.json() as IngestRuns, now);
      case "POST /heartbeat":
        return this.heartbeat(await request.json() as { publisher?: string }, now);
      default:
        return Response.json({ error: "not found" }, { status: 404 });
    }
  }

  async alarm() {
    const now = Date.now();
    if (this.state.expire(now)) {
      this.saveMeta();
      this.broadcast({ runs: [], removed: [] }, now);
    }
    this.write(": keepalive\n\n");
    // Stay idle (and unbilled) when nobody watches and nothing can go stale.
    if (this.subscribers.size || this.state.feed.connected) await this.schedule();
  }

  private async ingestRuns(message: IngestRuns, now: number) {
    const update = this.state.applyRuns(message, now);
    const sql = this.ctx.storage.sql;
    for (const run of message.runs) sql.exec("INSERT OR REPLACE INTO runs (id, body) VALUES (?, ?)", run.id, JSON.stringify(run));
    for (const id of update?.removed ?? []) sql.exec("DELETE FROM runs WHERE id = ?", id);
    this.saveMeta();
    if (update) this.broadcast(update, now);
    return Response.json({ ok: true, revision: this.state.revision });
  }

  private async heartbeat(message: { publisher?: string }, now: number) {
    const reconnected = this.state.heartbeat(message.publisher ?? "unknown", now);
    this.saveMeta();
    if (reconnected) this.broadcast({ runs: [], removed: [] }, now);
    await this.schedule();
    return Response.json({ ok: true, revision: this.state.revision });
  }

  private subscribe(selected: string | null, now: number) {
    const stream = new TransformStream<Uint8Array, Uint8Array>();
    const subscriber = { writer: stream.writable.getWriter(), selected };
    this.subscribers.add(subscriber);
    this.send(subscriber, this.state.snapshot(selected, now));
    void this.schedule();
    return new Response(stream.readable, {
      headers: {
        "content-type": "text/event-stream; charset=utf-8",
        "cache-control": "no-cache",
        "x-accel-buffering": "no",
      },
    });
  }

  private broadcast(update: { runs: Run[]; removed: string[] }, now: number) {
    for (const subscriber of this.subscribers) this.send(subscriber, this.state.message(update, subscriber.selected, now));
  }

  private send(subscriber: Subscriber, message: unknown) {
    this.writeTo(subscriber, `data: ${JSON.stringify(message)}\n\n`);
  }

  private write(text: string) {
    for (const subscriber of this.subscribers) this.writeTo(subscriber, text);
  }

  /**
   * Never wait on a viewer: a stalled stream must not hold up ingest. A viewer
   * that falls far behind is dropped; its browser reconnects and resyncs.
   */
  private writeTo(subscriber: Subscriber, text: string) {
    const writer = subscriber.writer;
    if ((writer.desiredSize ?? 0) < -MAX_BACKLOG) {
      this.subscribers.delete(subscriber);
      void writer.abort("viewer fell behind").catch(() => {});
      return;
    }
    writer.write(this.encoder.encode(text)).catch(() => {
      // The viewer went away; its stream is closed.
      this.subscribers.delete(subscriber);
    });
  }

  private async schedule() {
    if ((await this.ctx.storage.getAlarm()) === null) {
      await this.ctx.storage.setAlarm(Date.now() + KEEPALIVE_MS);
    }
  }

  private saveMeta() {
    const sql = this.ctx.storage.sql;
    for (const [key, value] of Object.entries({ order: this.state.order, revision: this.state.revision, feed: this.state.feed })) {
      sql.exec("INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)", key, JSON.stringify(value));
    }
  }
}
