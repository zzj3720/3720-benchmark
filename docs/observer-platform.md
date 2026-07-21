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
increasing sequence, source timestamp, task identity, action, complete
post-action state, and compact result. Reads do not advance a virtual clock,
trigger cooldowns, alter scoring, or enter the Agent command audit.

- Parabox projects its native `parabox-events-v1` log.
- Swarm projects its `swarm-audit-v1` initial state and command responses.
- Sausage writes the common event schema directly.

## Multi-run live gateway

`observer-platform/live-gateway/server.py` is the authority used by the public
dashboard. It auto-discovers active Compose services labelled `game`, matches
their case-insensitive trial id to `.harbor/jobs`, and reads private event logs
from the containers. It also selects the newest saved artifact for inactive
game/model combinations and extracts visible messages from the active Agent's
saved native session.

The gateway publishes only:

```text
GET /health
GET /v1/runs
GET /v1/runs/<run-id>
```

The summary feed includes model, Agent, score, current original level name,
live/final status, timestamps, and score history. A detail read adds the
authoritative game state, recent environment events, and visible Agent
activity. Hidden reasoning is neither required nor exposed.

Score charts normalize every run to cumulative Agent execution time starting
at `0h`; the horizontal axis never uses calendar time. Continuation gaps and
infrastructure-only failed segments are excluded, while live runs extend the
current active segment until the next gateway refresh.

## Public path

The console at [live.benchmark.3720.org](https://live.benchmark.3720.org)
polls `/api/live/v1/runs` every two seconds. Vinext forwards that strict
allowlist to the loopback gateway on port 3740. A named Cloudflare Tunnel
connects the local site to `benchmark-live-origin.3720.org`, and a Worker custom
domain exposes the final hostname. Both public layers reject writes; game
commands, session files, Docker, and Harbor artifacts are never directly
reachable from the internet.
