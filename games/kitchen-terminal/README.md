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

The sidecar advances the shift from a monotonic wall clock. State exposes every
currently reachable semantic destination with its shortest-route travel time.
`go` commits the active chef to one destination; travel consumes real shift
time and performs no interaction on arrival. Every destination choice, chef
switch, interaction, and held-work transition must be issued as a separate
model command. There is no pause, client clock advance, batch request, action
queue, future or conditional action, or direct station automation.

Both original single-player chef avatars remain live. Each has independent
hands, travel, and held-work state. `switch` changes which chef receives the
next command without cancelling the other chef, so both can travel or work in
parallel; `stop` affects only the active chef.

Passive game timers use the configured wall-clock scale. Continuous held inputs
such as chopping, washing, and extinguishing preserve the original relative
durations but cap their stretch at 4×, so a long benchmark shift does not turn
an input that contains no intervening decision into minutes of idle waiting.

An alarm stores a reminder and `kitchen wait` can block until it is due. An
alarm never performs a game action. The audit records the actual elapsed time
and exact response for every command; the verifier deterministically replays
that transcript without sleeping. Restarting the sidecar replays the same audit
to restore the last committed game state and resumes the monotonic clock from
that elapsed time.

Model-facing responses omit raw walkable cells and include authored object
metadata only when it carries supplies, processing rules, items, plate stacks,
or other mutable state. Empty counters and delivery points remain available
through `destinations`, which reports each reachable target's id, name, kind,
and relative travel time. All reported milliseconds are already wall-clock
milliseconds.
Observer events are produced directly from the same authoritative session and
always contain the complete rendering snapshot.

## Local use

```bash
cd games/kitchen-terminal
cargo run --bin kitchen-server
```

In another shell:

```bash
cargo run --bin kitchen -- show
cargo run --bin kitchen -- start
cargo run --bin kitchen -- go object-123
cargo run --bin kitchen -- switch
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
