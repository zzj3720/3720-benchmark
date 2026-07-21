# Emergency Operator Terminal

This crate is the first real-time game engine in the benchmark. It models an
emergency dispatch shift with ringing calls, dialogue choices, hidden incident
locations, continuously degrading health, vehicle travel, responder work, and
alarms that can wake an Agent but can never execute an action for it.

`data/campaign/pilot.json` is an original
acceptance fixture. It contains no data from 911 Operator. The format
deliberately mirrors only the public concepts in
the official Call Editor documentation so an importer can be evaluated after a
locally owned game installation is available.

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
```
