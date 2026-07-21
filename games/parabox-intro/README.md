# Parabox Terminal

This directory is the source of truth for the complete `parabox-intro` game:
the Rust terminal adaptation, unit tests, independent replay verifier,
locally extracted campaign data, packaging scripts, and live renderer.

The Harbor task is a packaged artifact. Run:

```bash
games/parabox-intro/scripts/package_task.sh
```

The packager cross-builds Linux `amd64` and `arm64` binaries. The Agent image
receives only a thin `parabox` API client. A separate game sidecar receives the
rules engine, server, and frozen campaign data, while the verifier image gets
its own engine and campaign copy. Rust source and the game's unit tests stay
here.

The game API returns one `parabox-api-v3` JSON object for every command. Current
spaces are represented as two-dimensional character arrays ordered
top-to-bottom and left-to-right. Calls are serialized through a server-side
500 ms cooldown. A `move DIR...` request can batch a planned direction sequence
and stops as soon as the current puzzle is solved. The authoritative action
history remains in the sidecar; Harbor collects it directly for independent
verifier replay. Every solved puzzle adds exactly one integer point. The API
reports the updated score immediately, and `parabox submit` reports the same
score without ending the task so an Agent can check progress and continue.
The sidecar additionally emits an append-only `parabox-events-v1` JSONL stream
containing timestamped score, score-delta, solved-level, and selected-level
state for every request. This is the authoritative source for score timelines.

For local development:

```bash
cargo test --manifest-path games/parabox-intro/Cargo.toml

cargo run --manifest-path games/parabox-intro/Cargo.toml \
  --bin solve-level -- games/parabox-intro/data/campaign/levels/a8.level

PARABOX_CAMPAIGN_DIR="$PWD/games/parabox-intro/data/campaign" \
PARABOX_STATE=/tmp/parabox-server-state.txt \
PARABOX_API_RATE_STATE=/tmp/parabox-api-rate.txt \
PARABOX_LISTEN_ADDR=127.0.0.1:3720 \
cargo run --manifest-path games/parabox-intro/Cargo.toml --bin parabox-server

PARABOX_API_ADDR=127.0.0.1:3720 \
cargo run --manifest-path games/parabox-intro/Cargo.toml --bin parabox -- show
```

`solve-level` is a development-only oracle generator and is not packaged into
the Harbor environment. The API address, server state path, and cooldown path
are configurable for local tests; the packaged sidecar fixes them inside its
isolated container.

The extracted original levels remain copyrighted by their respective owner and
must not be redistributed without permission.
