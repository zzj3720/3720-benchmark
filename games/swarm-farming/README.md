# Swarm terminal adapter

This directory adapts the official Swarm 0.8.0.0 engine for deterministic
Harbor episodes. It does not reimplement game rules.

`swarm-harbor` has three modes:

- `serve SCENARIO AUDIT PORT DEADLINE_TICKS` owns one game state and exposes
  the model-facing HTTP API;
- `verify SCENARIO AUDIT DEADLINE_TICKS` initializes the same scenario and
  replays every audited command and response;
- `oracle SCENARIO PROGRAM DEADLINE_TICKS` is a development-only scenario
  check.

`POST /v1/run` parses and starts a Swarm program without advancing the game.
`POST /v1/advance/N` executes exactly `N` original engine ticks unless the
scenario wins or reaches its deadline first. This separation is the
model-controlled virtual clock. A later token-driven clock can reuse the same
advance operation without adding another simulation path.

The frozen Farming case is derived from the upstream
`data/scenarios/Tutorials/farming.yaml`. Its rules, seed, world, initial state,
and objectives are unchanged; the embedded `solution` field is removed.
See [UPSTREAM.md](UPSTREAM.md) for the pinned source and license.

Build and check the scenario locally:

```bash
./test.sh
```

The check runs the Oracle twice for exact determinism, confirms an unsolved
audit scores zero, and requires malformed and response-tampered audits to fail.

Package the self-contained task artifacts from the repository root:

```bash
games/swarm-farming/scripts/package_task.sh
```
