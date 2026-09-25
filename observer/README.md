# Live observer

Shared, read-only live-observation stack for every game in `games/`:

- `runtime/` — Rust `run-recorder` (sole journal writer), `live-publisher`
  (pushes projections to Cloudflare), `live-gateway` (the same projection
  served locally), `run-archive`, and replay tools.
- `relay/` — sidecar relay that exposes native Parabox/Swarm logs through the
  common `/v1/observe` API.
- `web/` — the public console: one Cloudflare Worker with its `LiveHub`
  Durable Object and R2 bucket.

Runs are launched through [`tools/bench/bench`](../tools/bench/README.md), which
calls [`tools/observer/harbor-run`](../tools/observer/harbor-run) to build the
recorder and attach the Harbor journal plugin. Game-specific
rendering lives in `games/<game>/observer/`.

The protocol, storage layout, and deployment procedure are in
[docs/live-platform.md](../docs/live-platform.md).
