# 3720 Live Operations

`observer-platform/` is the public, read-only live console for 3720 game
benchmarks. It shows real Harbor runs rather than browser fixtures:

- a separate scoreboard for Parabox, Swarm, Sausage, and Emergency Operator;
- one score-over-effective-agent-time series per model, measured from the
  logical run's first eligible Agent execution window and excluding pauses,
  infrastructure-only attempts, and gaps between continuation segments;
- a drill-down for the selected run with its current objective, authoritative
  environment state, latest action/result, visible native Agent session
  messages, and recent append-only events;
- both active trials and the most recent saved result for each game/model.

The browser holds one Server-Sent Events subscription. Selecting a run replaces
the stream with one that also carries that run's detailed activity:

```text
GET /api/live/subscribe
GET /api/live/subscribe?run_id=<run-id>
```

The snapshot endpoints remain available for diagnostics, but the live UI does
not poll them.

The gateway wakes subscribers from Docker lifecycle events, `tail -F` streams
for each active game's event log and native Agent session log, and Sausage's
blocking observer feed. With no source event it sends only an SSE keepalive;
there is no timed snapshot refresh behind the subscription.

Emergency Operator publishes read-only clock snapshots once per second after a
shift starts, so calls, ETAs, incident health, alarms, and score remain live
while the Agent is waiting. These observer ticks never deliver an alarm or
enter the scoring audit.

The Vinext route forwards them to the loopback live gateway on port 3740. The
gateway auto-discovers active Harbor `game` containers, associates them with
their job/trial directories, and reads their private event logs through Docker.
This is necessary because Harbor gives each game sidecar the egress container's
network namespace instead of publishing a host port. Archived results are read
from both the configured `.harbor/jobs` collection and Harbor's default `jobs`
directory; the gateway never calls a game mutation endpoint.

A separately launched Sausage sidecar on port 3733 is also shown, but is marked
`No agent attached` until a real Harbor Agent run exists. Missing sources are
shown as unavailable—there is no demo-data fallback.

The current gateway has a legacy compatibility reader for Harbor continuation
paths and checkpoint manifests. It is not the intended identity model for new
runs. The target runtime ledger and migration boundary are specified in
[`docs/tracks/live-observability.md`](../docs/tracks/live-observability.md).

The production URL is [live.benchmark.3720.org](https://live.benchmark.3720.org).
A Cloudflare Worker accepts only `GET` and `HEAD`, forwards to the named Tunnel
origin, and the application route again allowlists only the two live endpoints.

## Local operation

Node.js 22.13 or newer and Docker are required.

```bash
vp install
uv run python live-gateway/server.py
vp run test
vp run start
```

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
