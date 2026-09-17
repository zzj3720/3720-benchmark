# Emergency Operator Terminal

This crate is the first real-time game engine in the benchmark. It models an
emergency dispatch shift with ringing calls, dialogue choices, hidden incident
locations, continuously degrading health, vehicle travel, responder work, and
alarms that can wake an Agent but can never execute an action for it.

`data/campaign/911-career.json` is the packaged five-chapter career: 14 duties,
60 fixed phone calls, 42 overlapping CAD reports, and 102 dispatch scenes over
a 30-minute real-time shift. The original 155.5-minute event schedule is
uniformly scaled during compilation; arrival and action windows, scene timers,
responder work, travel, and health pressure retain their relative timing. It is
deterministically compiled from the owned-install extraction with:

```bash
uv run games/emergency-operator/scripts/build_911_campaign.py
uv run games/emergency-operator/scripts/build_911_campaign.py --check
```

`data/campaign/pilot.json` remains a small original acceptance fixture for
focused engine tests, but is not the benchmark campaign.

`data/911-operator/` is a separate gameplay-only extraction from a locally
owned Steam installation. It preserves all installed call trees, city road
graphs, gameplay definitions, and the five-chapter base career layout while
excluding audio, images, UI, models, and localization. Regenerate or verify it
with:

```bash
uv run games/emergency-operator/scripts/import_911_operator.py
uv run games/emergency-operator/scripts/import_911_operator.py --check
```

The importer validates the supported game build, reads Unity `TextAsset`
definitions, disassembles the career chapter methods, and refuses unknown
source hashes instead of guessing at a changed format. The compiler then
normalizes the dialogue graph and scene action DSL into the single v2 campaign
format consumed by the Rust engine. Its score ceiling is a generated upper
bound over incident outcomes and every positive score effect; the
`reference-run` binary exercises every event without wall-clock sleeping.

Game content, rules, APIs, tests, scripts, and live rendering are owned by this
directory. The pure `Session`
advances to caller-supplied elapsed milliseconds. The live
server supplies those values from its monotonic clock; the verifier supplies
the recorded values without sleeping. Every mutating game request contains
exactly one immediate action. There is no batch, future action, callback,
conditional trigger, pause, speed, or explicit time-advance command.

After start, the sidecar publishes one read-only observer snapshot per second.
These clock events keep the live console current but never enter the scoring
audit, deliver an alarm, or perform an Agent action.

Run the engine tests with:

```bash
cargo test --manifest-path games/emergency-operator/Cargo.toml
cargo run --manifest-path games/emergency-operator/Cargo.toml --bin reference-run
```
