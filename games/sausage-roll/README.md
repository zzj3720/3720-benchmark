# Sausage Terminal

This directory is the source of truth for the complete Stephen's Sausage Roll
benchmark game, including its Rust terminal rules adaptation and the campaign
data under `data/`.

The implementation combines these owned inputs and clean-room components:

- `data/campaign/merged_binary.gz` is the complete
  86-puzzle campaign extracted
  from a locally owned Steam installation;
- `data/campaign/overworld.sav` is an original-engine map snapshot used to
  recover the settled 205-island overworld layout;
- `data/campaign/overworld.tar.gz` contains the 86 post-puzzle map snapshots
  used to restore the original transition heights while preserving simulated
  horizontal island movement;
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
- the persistent overworld exactly replays all 4,592 between-puzzle actions,
  including world-sausage rewards, stacked transfers, and moving island groups;
- covered mechanics include pushing, turning, rolling, cooking, grill retreat,
  fork attachment, gravity, ladders, pivots, stacks, moving islands, exit
  attachment, and general three-dimensional passive forces.

`data/campaign/entries.tar.gz` contains one solution-free original entry state
for each puzzle. The model-facing `Session` uses those states plus the map
snapshot and post-puzzle map checkpoints; the direction guide remains isolated
from runtime code and is used only for compatibility tests.

The model-facing session starts on the traversable overworld. Every unfinished
puzzle entrance is available: the model chooses one, walks to its absolute
position, faces the required direction, and enters it. After solving the
puzzle, play continues from that puzzle's original exit on the same persistent
map. The saved map snapshot is normalized to the campaign's untouched player
and zero-progress metadata at load time; it does not reveal puzzle solutions.

Observer snapshots expose a player-centered live projection and one reusable
full-map description. The frontend applies current island transforms and
completion masks to that shared map, so event rows stay small while the live
view can switch between following the player and the full 205-island world.
The first observer lifecycle record stores that map once as a gzip-compressed
asset; subsequent dynamic states and per-instruction replay frames do not
repeat it. The public gateway assigns the decoded asset a content hash, and the
browser caches it independently from live state and selected replay segments.

The packaged Harbor task gives the Agent only a thin terminal HTTP client. The
sidecar owns the Rust engine, campaign state, append-only command audit, and
observer event stream. A separate verifier starts from the same solution-free
entries and accepts a score only after every audited command reproduces the
exact recorded API response.

For replay, a batched `move` or `undo` records the complete observer state after
each instruction from that same authoritative execution. The frames are stored
inside the operation's JSONL record as a `gzip+base64` instruction trace; they
are not returned to the Agent and are not maintained as a second timeline.

Run the current parser and provenance checks with:

```bash
cargo test --manifest-path games/sausage-roll/Cargo.toml
cargo run --manifest-path games/sausage-roll/Cargo.toml \
  --bin sausage-inspect
cargo run --manifest-path games/sausage-roll/Cargo.toml \
  --bin sausage-walkthrough
```

Package original-engine post-puzzle saves in campaign order with the Rust
importer:

```bash
cargo run --release --manifest-path games/sausage-roll/Cargo.toml \
  --bin sausage-import-overworld -- \
  games/sausage-roll/data/campaign/entries.tar.gz /path/to/saves \
  games/sausage-roll/data/campaign/overworld.tar.gz <run-prefix>
```

Build the self-contained Linux task artifacts with:

```bash
games/sausage-roll/scripts/package_task.sh
```

`data/campaign/` is extracted from a locally owned Stephen's Sausage Roll
installation and is committed here so that a clone can replay the campaign
without external assets. `data/oracle/all.dem` comes from the Apache-2.0
`jbzdarkid/SSRDecompile` project at the revision pinned in
`data/oracle/UPSTREAM.md`.
