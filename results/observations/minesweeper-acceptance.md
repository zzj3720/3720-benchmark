# No-Guess Minesweeper acceptance

Date: 2026-07-24

## Frozen benchmark

- Task: `3720/minesweeper`
- Dataset digest:
  `sha256:da37a08a43f055d372edce0c9ae213f4f34980eadede747031afdd750c95393c`
- Harbor task checksum:
  `67c06adb2bfba81ff1e395fe9171611e195f95bb49661b0e921bbbfa341ae773`
- Campaign SHA-256:
  `2021e632ee672bfe443061a85ae5251128c9157069e5d5f5d3ef7c9494a14650`
- Campaign: fifty original levels in Cadet, Operator, Specialist, Expert, and
  Master tiers
- Attempt policy: selecting a level starts its only attempt. A loss permanently
  consumes it, and a started level cannot be abandoned for another level.
- Score: solve any five of the ten levels in a tier to earn one point, from 0
  through 5. Six losses in the active tier make five wins impossible and end
  the campaign at its current score.
- Public commands are `show`, `levels`, `select`, `reveal`, `flag`, `chord`,
  and `submit`. The game has no reset operation.

## Guarantee and replay gates

- Every first reveal and its clipped 3×3 neighborhood are mine-free.
- The first coordinate and level seed fully determine mine placement.
- A candidate board is rejected unless `local-subset-v1` completes it using
  ordinary adjacent-count rules and subset differences.
- The same proof must meet the level's `proof-profile-v1`. Opening expansion
  has a lower and upper bound; the other proof measurements retain minimums:

  | Levels | Opening band | Proof rounds | Subset rounds | Frontier |
  | --- | ---: | ---: | ---: | ---: |
  | `cadet-01` | 10–75% | 1 | 0 | 4 |
  | remaining Cadet | 10–65% | 3 | 1 | 8 |
  | Operator | 8–45% | 7 | 5 | 16 |
  | Specialist | 6–35% | 10 | 8 | 20 |
  | Expert | 5–25% | 13 | 10 | 24 |
  | Master | 3–15% | 16 | 13 | 30 |

- `tier-opening-distribution-v1` certifies the complete set of first-click
  boards in each tier. The weighted mean is `sum(opening) / sum(safe cells)`.
  The upper median is the element at index `count / 2` after sorting individual
  opening ratios:

  | Tier | First clicks | Actual range | Weighted mean (band) | Upper median (band) |
  | --- | ---: | ---: | ---: | ---: |
  | Cadet | 519 | 11.11–72.73% | 42.09% (40–44%) | 44.44% (42–47%) |
  | Operator | 990 | 8.08–45.00% | 27.04% (25–29%) | 27.03% (25–29%) |
  | Specialist | 1,425 | 6.12–34.83% | 20.54% (19–22%) | 20.69% (19–22%) |
  | Expert | 1,789 | 5.08–25.00% | 15.76% (15–17%) | 15.79% (15–17%) |
  | Master | 2,315 | 3.00–15.00% | 10.55% (10–12%) | 10.95% (10–12%) |

- Rust tests generate and certify all 7,038 possible first-click choices across
  the fifty campaign levels. The maximum accepted-candidate index is 661,
  below the frozen 1,000-attempt runtime bound.
- The public-client E2E intentionally loses one level, confirms that neither
  reselecting that level nor abandoning the attempt is possible, and confirms
  that `/v1/reset` does not exist. It then passes all five tiers, observes score
  events, restarts the sidecar, restores score 5, verifies the complete audit,
  and confirms a changed response is rejected. It also checks the serialized
  Master proof metrics exposed through the observer state.
- Strict Clippy, the observer build, observer-runtime tests, and every static
  task check pass.

The packaged client SHA-256 values are
`3a1546af55f6663d4ffda870437eb87cd2874e5a61aa5a2a61b18c2438af075d`
(`amd64`) and
`0148bc8b2082a46e91ea2dc8233dee639b50522d24fb89c69dc3249fc4833067`
(`arm64`). The server values are
`e7637382decab49c8a29bf33bd50c63db17e43ca20188d98aa741512875cf1f5`
and
`97f8bfb15616ea8c6398d852608b901edb6f3c5332c6678c2b12a26f909fefef`.
The verifier values are
`07361e9f5de1e6e24fb976f462076555137dc47e71eb27a7bb64e69dc5378fa9`
and
`2a96dc06c8179edd3f3b663d50fb33fef1bf1998439e7340f7bbe371555fdd4b`.

## Harbor controls

| Job | Agent | Reward | Exceptions |
| --- | --- | ---: | ---: |
| `minesweeper-one-shot-oracle-acceptance/minesweeper__3GtKFqb` | Oracle | 5 | 0 |
| `minesweeper-one-shot-nop-acceptance/minesweeper__FyrAjTt` | Nop | 0 | 0 |

The Oracle used only the packaged `minesweeper` client and passed every tier by
solving its first five available levels. The separate verifier reconstructed
every first-click-dependent board and returned 5 after replaying 969 commands.
Nop left the task untouched and returned zero.

The first Flash calibration used the superseded retryable 0–50 rules and
reached 13 before it was stopped. It is invalid calibration evidence and must
not be included in ranked results. A fresh one-shot calibration is required.
