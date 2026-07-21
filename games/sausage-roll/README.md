# Sausage Terminal

This directory is the source of truth for the complete Stephen's Sausage Roll
benchmark game, including its Rust terminal rules adaptation and locally owned
data under `data/`.

The implementation has three independent inputs:

- `data/campaign/merged_binary.gz` is the complete
  86-puzzle campaign extracted
  from a locally owned Steam installation;
- `src/` is a clean-room Rust rules engine and parser;
- `data/oracle/all.dem` is the public full-game
  direction replay used only for
  compatibility testing.

Current compatibility status:

- the binary campaign importer reads all 205 island states, 30 shrine groups,
  and 86 playable puzzles from the owned build;
- the walkthrough importer splits the full run into 86 typed entry/replay/
  checkpoint segments containing 11,769 puzzle actions;
- the original engine has replayed the complete normalized 16,361-input run
  through all 86 puzzles;
- one unified clean-room Rust engine exactly replays all 86 puzzle segments,
  all 11,769 actions, and all 11,683 non-final original-engine checkpoints;
- covered mechanics include pushing, turning, rolling, cooking, grill retreat,
  fork attachment, gravity, ladders, pivots, stacks, moving islands, exit
  attachment, and general three-dimensional passive forces.

`data/campaign/entries.tar.gz` contains one
solution-free original entry state for each puzzle. The model-facing `Session`
loads only that archive and the owned campaign; the direction guide and
original-engine checkpoints are never required by runtime code.

The packaged Harbor task gives the Agent only a thin terminal HTTP client. The
sidecar owns the Rust engine, campaign state, append-only command audit, and
observer event stream. A separate verifier starts from the same solution-free
entries and accepts a score only after every audited command reproduces the
exact recorded API response.

Run the current parser and provenance checks with:

```bash
cargo test --manifest-path games/sausage-roll/Cargo.toml
cargo run --manifest-path games/sausage-roll/Cargo.toml \
  --bin sausage-inspect
cargo run --manifest-path games/sausage-roll/Cargo.toml \
  --bin sausage-walkthrough
```

Build the self-contained Linux task artifacts with:

```bash
games/sausage-roll/scripts/package_task.sh
```

The original level data remains copyrighted by its owner and must not be
redistributed without permission.
