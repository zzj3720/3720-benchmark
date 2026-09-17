# Sokoban Classics acceptance

Date: 2026-07-23

## Campaign and Oracle

- The frozen campaign contains 305 levels: 50 Novoban, 155 Microban,
  50 Sasquatch, and 50 Sasquatch III.
- `games/sokoban/data/oracle/solutions.tsv` contains one ordinary LURD trace
  for every campaign level. Its SHA-256 is
  `76de54ff1f349ea8c5d679d2f396ae5c76818c6d145e8e4266c9abd02ad4806a`.
- The hidden Harbor copy is byte-identical to the development Oracle.
- `cargo test --manifest-path games/sokoban/Cargo.toml --test oracle` replays
  all 305 traces through the Rust `Session` and reaches score 305 with all
  four tiers complete.

Source and transformation provenance is frozen in
`games/sokoban/data/oracle/source.json`. Novoban traces were generated locally
with Festival 3.1. The other three packs use public KSokoban coordinate-and-push
records that were accepted only after exact board comparison, expanded into
ordinary player moves, and replayed by the Rust engine.

## Real entry-point verification

A full local HTTP run used only the packaged `sokoban` client against the live
sidecar. It issued 2,197 commands, solved every level, submitted score 305, and
produced an authoritative audit. A fresh `sokoban-verifier` process replayed
that audit from the frozen campaign and reported:

```text
score: 305
max_score: 305
verified: 2197 commands through deterministic replay
```

The uncompressed full-run audit is about 13 MiB and the observer event stream
is about 30 MiB. These are runtime artifacts and are not committed.

## Harbor controls

| Job | Agent | Reward | Exceptions |
| --- | --- | ---: | ---: |
| `sokoban-full-oracle-305-20260723` | Oracle | 305 | 0 |
| `sokoban-full-nop-0-20260723` | Nop | 0 | 0 |

The Oracle job completed in 1 minute 39 seconds. The isolated verifier replayed
the sidecar audit and returned 305. The Nop job completed in 44 seconds and
returned zero. Both jobs used the packaged multi-architecture task through
Harbor's normal Docker entry points.

The dataset records task digest
`sha256:e5dadb66123ab29a487147f76c366605a3f2c360e931d7ca3b77950f4631a14a`.
