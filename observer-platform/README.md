# 3720 Live Operations

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

The browser holds one Server-Sent Events subscription. Selecting a run replaces
the stream with one that carries only that run's revision alongside the compact
scoreboard:

```text
GET /api/live/subscribe
GET /api/live/subscribe?run_id=<run-id>
```

When the revision changes, the browser reads the selected run detail once.
Replay frames are loaded only after an attempt is selected. Large immutable
game data is exposed through content-addressed assets and cached independently,
so it never rides along with SSE updates or every replay response.

The Rust gateway wakes subscribers from filesystem notifications on the one
chain journal. Compressed replay traces and immutable scenes live beside it as
content-addressed objects and are read only for the latest state or selected
attempt. Historical runs come from an immutable local archive. With no source
event it sends only an SSE keepalive; there is no timed snapshot refresh behind
the subscription and no Docker or Harbor scan on a request.

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
A Cloudflare Worker accepts only `GET` and `HEAD`, forwards to the named Tunnel
origin, and the application route again allowlists only the two live endpoints.

## Local operation

Node.js 22.13 or newer and Rust are required. Docker is needed only to launch
benchmark tasks, not to serve the live console.

```bash
vp install
cargo run --release \
  --manifest-path ../tools/observer/runtime/Cargo.toml \
  --bin live-gateway -- --root ..
vp run test
vp run start
```

The host service is published from an immutable, versioned release rather than
the mutable `dist/` directory. After a successful build and test run, publish
the current output atomically with:

```bash
vp run publish:local
```

The release keeps content-addressed chunks from the previous version so an HTML
document already cached at the edge cannot reference a file removed during the
switch. It also installs the release's Rust gateway binary and reloads its
LaunchAgent definition, so a runtime migration cannot accidentally restart an
older gateway command. The Cloudflare Worker serves static chunks and observer
assets from its immutable cache, preserves HTTP compression for JSON, and
routes the small SSE feed directly to the read-only gateway.

The defaults are:

```text
live gateway  http://127.0.0.1:3740
Vinext         http://127.0.0.1:3000
```

Set `LIVE_GATEWAY_ORIGIN` when the server-side proxy targets a different
gateway. For a local browser preview, set `VITE_LIVE_GATEWAY_ORIGIN` on the
Vinext dev process as well; this keeps an isolated fixture gateway separate
from the default live data on port 3740. `GET /health` on the gateway is the
process health check.

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
