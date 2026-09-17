# 3720 Live Operations

The supported deployment is now `compose.yaml`: immutable web/gateway images,
read-only history mounts, and a separate disposable query-cache volume. See
[`docs/live-platform-v2.md`](../docs/live-platform-v2.md) for the storage/API
contract, memory budgets, migration, and rollback.


`observer-platform/` is the public, read-only live console for 3720 game
benchmarks. It shows real Harbor runs rather than browser fixtures:

- a separate scoreboard for Parabox, Swarm, Sausage, Emergency Operator, and
  Sokoban;
- one score-over-effective-agent-time series per model, measured from the
  logical run's first eligible Agent execution window and excluding pauses,
  infrastructure-only attempts, and gaps between continuation segments;
- a drill-down for the selected run with its current objective, authoritative
  environment state, latest action/result, visible native Agent session
  messages, and recent append-only events;
- both active trials and the most recent saved result for each game/model.

The browser holds one versioned Server-Sent Events subscription:

```text
GET /api/live/v1/subscribe?protocol=2
GET /api/live/v1/subscribe?protocol=2&run_id=<run-id>
```

The first message is a snapshot. Later messages contain changed runs, removed
IDs, and appended score points. The browser preserves the last valid snapshot
on errors, retries detail independently of new game events, and serializes
revision updates. The v1 full-snapshot feed remains available for older pages.

Run detail includes the current state, recent visible Agent activity, and up to
200 attempts. Older catalog pages and bounded replay windows are fetched on
demand. Assets are immutable and use a bounded browser cache. The gateway uses
a disposable SQLite index with a 4 MiB page cache per connection, a 32 MiB
weighted detail cache, and at most two simultaneous projection/replay jobs.
The authority remains the journal, not SQLite.

Finalized history is stored in independent zstd segments with a 16 MiB decoded
limit (32 MiB is supported), indexed by sequence range and SHA-256. Active runs
rotate at the same limit and retain only the open tail as JSONL. Segment
boundaries never split an event. Source data is read-only inside Docker; the
recorder and archive CLI run on the host and own all authoritative writes.

Emergency Operator publishes read-only clock snapshots once per second after a
shift starts, so calls, ETAs, incident health, alarms, and score remain live
while the Agent is waiting. These observer ticks never deliver an alarm or
enter the scoring audit.

The Vinext route forwards them to the loopback live gateway on port 3740. New
Harbor runs are launched through `tools/observer/harbor-run`; its recorder
manifest is the discovery boundary and its journal is the only live source.
Archived results are served from `.harbor/live-archive`. The gateway never
calls a game mutation endpoint.

A separately launched Sausage sidecar on port 3733 is also shown, but is marked
`No agent attached` until a real Harbor Agent run exists. Missing sources are
shown as unavailable—there is no demo-data fallback.

The runtime ledger and migration boundary are specified in
[`docs/tracks/live-observability.md`](../docs/tracks/live-observability.md).

The production URL is [live.benchmark.3720.org](https://live.benchmark.3720.org).
A Cloudflare Worker accepts only `GET` and `HEAD`, rewrites `/api/live/v1/*` to
the gateway's `/v1/*`, and forwards to the named Tunnel origin. The Vinext
application route applies the same read-only allowlist (`/v1/runs`,
`/v1/assets/<id>`, `/v1/subscribe`) and streams responses through, so a
self-hosted `vinext start` serves the same endpoints without the Worker in
front.

## Docker operation

Docker Compose, Node.js 24+ (for host checks), and Rust (for the host recorder)
are required. From the repository root:

```bash
npm --prefix observer-platform ci
npm --prefix observer-platform test
cargo test --manifest-path tools/observer/runtime/Cargo.toml
python3 observer-platform/scripts/publish_docker.py --preview
python3 observer-platform/scripts/publish_docker.py
```

Publication builds versioned images and the host recorder, verifies the
candidate on ports 14000/14740, compares run identities/scores with the current
service, and only then switches ports 3000/3740. Failure restores the previous
Docker release or the existing macOS LaunchAgents. The native services are
disabled only during the first successful switch. Public content-hashed chunks
are retained across releases so cached HTML remains usable.

The web and gateway each have a 256 MiB container limit, with swap disabled.
Only loopback ports are published. The gateway's `/health` becomes ready after
its query index has been rebuilt; startup can take longer with a cold cache.
A writer heartbeat lets the Docker gateway observe the host recorder without
assuming that macOS and the Linux VM share file-lock ownership.

For development, use `npm run dev` inside this directory and start the Rust
gateway separately. The browser normally uses the same-origin API route.
Set `LIVE_GATEWAY_ORIGIN` on the Node process for an alternate gateway;
`VITE_LIVE_GATEWAY_ORIGIN` is an explicit browser override for isolated fixtures.

## Render QA

The development-only gallery renders ignored, locally generated fixtures through
the production observer components. Parabox samples 64 recursive states from
its complete walkthrough. Sausage samples 64 entry/mid/final states across the
86-puzzle, 11,769-action walkthrough and includes height, grills, ladders,
detached forks, cooked faces, multi-island puzzles, and exit-ready states.

Generate the fixture you need before starting the dev server:

```bash
vp run qa:parabox:data
vp run qa:sausage:data
vp dev
```

Then open `http://localhost:5173/qa-gallery` for Parabox or
`http://localhost:5173/qa-gallery?game=sausage` for Sausage (using the port
printed by Vite). Sausage is paginated to eight WebGL scenes at a time so the
browser never creates all 64 contexts at once.
