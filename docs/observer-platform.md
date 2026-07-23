# Benchmark live observation

The live platform has two read-only layers: a common per-game observer schema
inside sidecars, and a host gateway that projects all current Harbor trials into
one multi-run feed.

## Per-game observer protocol

Each scored sidecar supports:

```text
GET /v1/observe/snapshot
GET /v1/observe/events?after=<sequence>&limit=<1..1000>&wait_ms=<0..30000>
```

Normalized events use `benchmark-observer-event-v1` and carry a monotonically
increasing sequence, source timestamp, task identity, action, post-action
dynamic state, and compact result. Large immutable scene data is emitted once
as a gzip-compressed observer asset and referenced by content hash. Reads do
not advance a virtual clock, trigger cooldowns, alter scoring, or enter the
Agent command audit.

- Parabox projects its native `parabox-events-v1` log.
- Swarm projects its `swarm-audit-v1` initial state and command responses.
- Sausage writes the common event schema directly.

Parabox native records additionally contain a private recursive scene graph for
the host dashboard. The common sidecar relay strips that field so richer human
rendering does not expand the state available to the benchmark Agent.

## Multi-run live gateway

The Rust `live-gateway` binary in `tools/observer/runtime` is the read-only
projection used by the public dashboard. New runs are read from the single
chain journal. Runtime lifecycle, sidecar game records, visible Agent messages,
tool actions, and workspace notes therefore arrive with one identity, one
sequence, and an already-stamped effective Agent time. Runs created before the
recorder are frozen in the local `.harbor/live-archive`; request handling never
scans Docker or joins live Harbor artifacts.

Recorder-aware sidecars append to a durable trial inbox. The Rust recorder
tails it incrementally and drains it idempotently by source sequence; the file
itself is also the recovery buffer. Agent session and note additions enter that
same recorder before publication. Live ingestion therefore does not poll
sidecar state.

The gateway publishes:

```text
GET /health
GET /v1/runs
GET /v1/runs/<run-id>
GET /v1/runs/<run-id>?replay_attempt=<attempt-id>
GET /v1/assets/<sha256>
GET /v1/subscribe?run_id=<run-id>
```

The SSE feed contains only run summaries and a selected-run revision. The
browser fetches detail after that revision changes; it does not receive the
complete current state on every SSE notification. A detail read adds the
authoritative dynamic game state, compact replay catalog, and explicit Agent
notes. A selected replay is loaded separately. Content-addressed assets are
immutable and cached by the browser and edge. Hidden reasoning is neither
required nor exposed.

Score charts normalize every run to cumulative Agent execution time starting
at `0h`; the horizontal axis never uses calendar time. Continuation gaps and
infrastructure-only failed segments are excluded, while live runs extend the
current active segment until the next gateway refresh.

## Public path

The console at [live.benchmark.3720.org](https://live.benchmark.3720.org) holds
one `/api/live/subscribe` connection. JSON detail and replay responses use HTTP
gzip; immutable assets use year-long content-hash caching. Vinext forwards the
strict read-only allowlist to the loopback gateway on port 3740. A named
Cloudflare Tunnel connects the local site to
`benchmark-live-origin.3720.org`, and a Worker custom domain exposes the final
hostname. Game commands, session files, Docker, and Harbor artifacts are never
directly reachable from the internet.
