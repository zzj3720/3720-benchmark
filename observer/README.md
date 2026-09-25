# Live observer

Shared, read-only live-observation stack for every game in `games/`:

- `runtime/` — Rust `run-recorder` (sole journal writer), `live-gateway`,
  `run-archive`, and replay tools.
- `relay/` — sidecar relay that exposes native Parabox/Swarm logs through the
  common `/v1/observe` API.
- `web/` — the public console and its Docker Compose deployment.

Runs are launched through [`tools/observer/harbor-run`](../tools/observer/harbor-run),
which builds the recorder and attaches the Harbor journal plugin. Game-specific
rendering lives in `games/<game>/observer/`.

The protocol, storage layout, and deployment procedure are in
[docs/live-platform.md](../docs/live-platform.md).
