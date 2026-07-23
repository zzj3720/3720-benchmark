# Kitchen Terminal

Kitchen Terminal is the complete Overcooked 1 base-campaign benchmark slice.
It contains the Rust rules engine and real-time sidecar, imported gameplay
data, scoring and replay verifier, command-line client, Observer renderer, QA
fixtures, tests, and task-packaging script.

The committed data is a deterministic, gameplay-only extraction from the
user's owned Steam Windows copy. It covers all 30 campaign kitchens, 120
one- through four-player variants, 33 scene layouts, the recipe graph, dynamic
platforms, doors, conveyors, hazards, and boss progression. It does not contain
the original audio, textures, models, localization, saves, account data, or
presentation-only motion.

## Time and action boundary

The sidecar advances the shift from a monotonic wall clock. Every move, dash,
chef switch, interaction, and held-work transition must be issued as a separate
model command at the time it should happen. There is no pause, client clock
advance, batch request, action queue, future or conditional action, or direct
station automation.

An alarm stores a reminder and `kitchen wait` can block until it is due. An
alarm never performs a game action. The audit records the actual elapsed time
and exact response for every command; the verifier deterministically replays
that transcript without sleeping.

## Local use

```bash
cd games/kitchen-terminal
cargo run --bin kitchen-server
```

In another shell:

```bash
cargo run --bin kitchen -- show
cargo run --bin kitchen -- start
cargo run --bin kitchen -- move north
cargo run --bin kitchen -- interact object-123
cargo run --bin kitchen -- alarm soup 12 "check the pot"
cargo run --bin kitchen -- wait
cargo run --bin kitchen -- submit
```

The server accepts `KITCHEN_LEVEL` (1-30), `KITCHEN_TIME_SCALE`,
`KITCHEN_SEED`, `KITCHEN_AUDIT`, and `KITCHEN_EVENTS`. The Observer endpoints
are `/v1/observe/snapshot` and `/v1/observe/events`.

Regenerate or verify imported data:

```bash
uv run scripts/import_overcooked.py
uv run scripts/import_overcooked.py --check
```
