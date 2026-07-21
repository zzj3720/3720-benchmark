# Kitchen Terminal

This crate starts the first fixed-station kitchen benchmark. Orders arrive on a
continuous real-time shift. The Agent starts one component at one station,
returns after its fixed cook duration, manually finishes it before the burn
deadline, assembles the completed components, and serves the order before the
customer leaves.

The pure engine accepts elapsed milliseconds from a caller so it can use the
same boundary as Emergency Operator: the live sidecar will supply monotonic
wall time, while the verifier will replay recorded times without sleeping.
There is no pause, explicit clock advance, batch, action queue, future action,
conditional trigger, or automatic station operation. The upcoming sidecar will
reuse the reminder-only alarm protocol; an alarm will never start or finish a
dish.

`data/campaign/pilot.json` is original benchmark content inspired by
fixed-station restaurant games as a genre. Game content, rules, tests, scripts,
and live rendering are owned by this directory. The pilot contains no data from Cook, Serve, Delicious!,
Papa's games, Diner Dash, or another commercial game.
