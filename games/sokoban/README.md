# Sokoban Classics

A conventional Sokoban benchmark with a deterministic Rust engine, a thin
terminal client, an authoritative REST sidecar, replay verification, and live
observer rendering.

The frozen campaign contains 305 levels in four progressive tiers:

| Tier | Levels | Unlock requirement | Intended difficulty |
| --- | ---: | ---: | --- |
| Novoban | 50 | starts unlocked | beginner, increasing |
| Microban | 155 | solve 25 Novoban | beginner to intermediate |
| Sasquatch | 50 | solve 78 Microban | intermediate |
| Sasquatch III | 50 | solve 25 Sasquatch | intermediate to very hard |

All levels in an unlocked tier may be selected freely. A newly solved level is
worth one point; replaying it does not score again. The maximum score is 305.

## Local development

```bash
cargo test --manifest-path games/sokoban/Cargo.toml
SOKOBAN_LISTEN_ADDR=127.0.0.1:3720 \
SOKOBAN_AUDIT=/tmp/sokoban-audit.jsonl \
SOKOBAN_EVENTS=/tmp/sokoban-events.jsonl \
cargo run --manifest-path games/sokoban/Cargo.toml --bin sokoban-server
```

In another shell:

```bash
cargo run --manifest-path games/sokoban/Cargo.toml --bin sokoban -- levels novoban
cargo run --manifest-path games/sokoban/Cargo.toml --bin sokoban -- select novoban-001
cargo run --manifest-path games/sokoban/Cargo.toml --bin sokoban -- move R
```

`move` accepts full direction names, single-letter directions, or a compact
string of up to 64 moves. `undo [N]`, `reset`, `show`, and `submit` are also
available.

Run `vp node games/sokoban/scripts/import_campaign.mjs` to reproducibly refresh
the normalized campaign from the SLC source files listed in
`data/campaign/source.json`.

The development Oracle covers all 305 levels. Rebuild the public-solution
portion with `vp node games/sokoban/scripts/import_solutions.mjs`, then merge it
with the locally generated Novoban solutions using
`vp node games/sokoban/scripts/merge_oracle.mjs <solutions.sok>`. The merged
ordinary-move traces and their provenance live under `data/oracle/`; the Rust
`oracle` integration test replays every trace through the same `Session` used
by the sidecar.
