# Sausage Roll deterministic acceptance

Date: 2026-07-21

## Scope

- Task: `3720/sausage-roll`
- Dataset digest:
  `sha256:5d4b2c72dd4fa69b4f040c768bf226d5dc0eb54731cf2b8119bc9b3fc192e89e`
- Owned Steam build: `20267558`, Unity `2018.4.25f1`
- Campaign: all 86 original puzzles in strict walkthrough order
- Reward: one integer point per independently verified solved puzzle
- Campaign gzip SHA-256:
  `a498f4bce9ebbf7825c261fe1ecb1b312f8adc07c3efb54ab8fd6bdd0005b694`
- Solution-free entry archive SHA-256:
  `e9abac37fcfc492cbd04ebabaa784cad7e81f14e84f39776b7b5658d8473611e`
- Development/Oracle walkthrough SHA-256:
  `ebb7a048d02658c9b8adc7a599cd16eb7a9ff614091797c0623285d3bc960671`

The level material was extracted from the user's locally owned installation.
It remains copyrighted by its owner and is not intended for public
redistribution.

## Engine compatibility

- The owned campaign parser reads 205 island states, 30 shrine groups, and 86
  playable puzzles.
- The solution-free runtime archive contains exactly 86 `.state` files and no
  directions or checkpoints.
- The complete guide contains 86 typed segments and 11,769 puzzle actions.
- The original game replayed the complete normalized 16,361-input run.
- One unified clean-room Rust engine replays all 86 guide segments.
- The Rust state after every one of 11,683 non-final actions exactly matches
  its original-engine checkpoint.
- The final action of every segment reaches the correct completion exit.

## API, package, and verifier gates

- The model-facing HTTP API accepts at most 32 planned moves and stops a batch
  at a level boundary.
- A containerized Oracle used only `/usr/local/bin/sausage`, completed all 86
  levels, and submitted score 86.
- The isolated packaged verifier replayed 411 exact API commands from empty
  state, reported `verified: 411 commands, 86/86 levels`, and wrote reward 86.
- The Nop history produces reward zero.
- Session, audit, and observer histories persist outside the Agent container.
- Packaged server binaries are native x86-64 and aarch64 ELF executables with
  SHA-256 values
  `a4190d69fe4653a9e55cabc2a30e4e7da40c095738d5f5a941fd00b39c3d9327`
  and
  `01fe9618990b95197c64e24e7149c9cd621e84a48486cd5339e9530726121caf`.
- All task static checks passed.

## Harbor acceptance

| Job | Agent | Reward | Exceptions |
|---|---|---:|---:|
| `2026-07-21__05-15-38` | Oracle | 86 | 0 |
| `2026-07-21__05-17-10` | Nop | 0 | 0 |

The first Oracle launch attempted to publish a task-side host port while Harbor
had assigned its shared egress-control network namespace. Docker rejected that
combination before game startup. Host publication was moved into optional
operator-only Compose overlays; the unchanged sidecar API remains internal to
the standard Harbor environment. The clean jobs above exercise the corrected
package.

## Live observer

The Sausage sidecar emits `benchmark-observer-event-v1` directly. A live
long-poll starting after sequence 1 was released by a model-facing `status`
command and returned sequence 2 with the complete Land's End state and score
zero. Observer requests themselves did not append audited commands.
