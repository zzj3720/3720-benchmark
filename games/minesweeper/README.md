# No-Guess Minesweeper

An original Minesweeper benchmark implemented in Rust. One crate owns the
deterministic rules engine, thin terminal client, authoritative REST sidecar,
observer state, and isolated replay verifier.

The campaign contains fifty progressive levels across Cadet, Operator,
Specialist, Expert, and Master tiers. Every first reveal is safe, as is its
surrounding 3×3 area.
Each level is a one-shot attempt: a detonation permanently records a failure
and the level cannot be selected again. Winning five of a tier's ten levels
passes that difficulty and awards one point, for a maximum score of five. Six
failures in the active tier end the campaign because five wins have become
impossible.
After that first reveal, the sidecar deterministically samples candidate mine
layouts until the frozen `local-subset-v1` solver proves the position can be
completed using only standard adjacent-count rules and subset inference. The
same proof must meet the level's frozen `proof-profile-v1`: initial expansion
must stay inside both a lower and an upper bound, while deduction rounds,
subset rounds, and maximum constraint frontier meet their minimums. A board
that would require guessing, open too much, or open too little is never
presented.

The campaign also freezes `tier-opening-distribution-v1` bands. Across every
possible first click in a tier, both the safe-cell-weighted mean opening ratio
and the upper median opening ratio must remain inside their configured lower
and upper bounds. The upper median is the element at index `count / 2` after
sorting individual opening ratios. This prevents a handful of valid outliers
from hiding a tier whose typical board is too easy or too hard.

## Local development

```bash
cargo test --manifest-path games/minesweeper/Cargo.toml
MINESWEEPER_LISTEN_ADDR=127.0.0.1:3720 \
MINESWEEPER_AUDIT=/tmp/minesweeper-audit.jsonl \
MINESWEEPER_EVENTS=/tmp/minesweeper-events.jsonl \
cargo run --manifest-path games/minesweeper/Cargo.toml --bin minesweeper-server
```

In another shell:

```bash
cargo run --manifest-path games/minesweeper/Cargo.toml --bin minesweeper -- levels cadet
cargo run --manifest-path games/minesweeper/Cargo.toml --bin minesweeper -- select cadet-01
cargo run --manifest-path games/minesweeper/Cargo.toml --bin minesweeper -- reveal 2 2
```

Coordinates are zero-based `ROW COLUMN`. `reveal` accepts up to 64 coordinate
pairs; `flag`, `chord`, `show`, and `submit` are also available.

The tests validate that all 7,038 possible first clicks across the fifty frozen
levels are safe, no-guess solvable, meet their per-board proof bands, and meet
their tier-wide opening-distribution bands. They also play the complete
campaign through the public client and reject tampered audits through
deterministic replay.
