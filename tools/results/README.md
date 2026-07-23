# Result tooling

New runs write runtime lifecycle, game state, scoring, Agent messages, Agent
tool actions, and workspace experience into one append-only Rust run journal.
Scoreboards, replay, continuation timing, and audit therefore consume the same
source instead of joining Harbor results, sidecar logs, and provider sessions
afterward.

Audit a completed or live chain with the Rust runtime:

```bash
cargo run --release \
  --manifest-path tools/observer/runtime/Cargo.toml \
  --bin run-audit -- \
  .harbor/run-journals/<chain>/journal.jsonl \
  results/audits/<chain>.json
```

High-risk findings identify suspicious access or automation for human review;
they are not by themselves evidence that game state or score changed. Final
cheating conclusions still require the authoritative game events in the same
journal.

The retired Python score, session-export, verifier-recovery, and observer
backfill scripts read several independently produced artifacts. Historical
runs were frozen once into `.harbor/live-archive`; new runs never use those
join paths.
