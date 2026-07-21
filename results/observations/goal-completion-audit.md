# Sausage and live observer goal completion audit

Date: 2026-07-21

## Requirement evidence

| Requirement | Authoritative evidence | Result |
|---|---|---|
| Rust Stephen's Sausage Roll-compatible engine | One `Game3d` implementation loads all 86 owned-install puzzle entries. The current `cargo test --all-targets` run passed 22 unit tests and all 4 Oracle tests; the complete walkthrough test passed in 162.67 seconds. Strict Clippy passed. | Proven |
| Complete original campaign import | The parser reads 205 island states, 30 shrine groups, and 86 playable puzzles from owned Steam build 20267558. The solution-free runtime archive has exactly 86 entries and frozen SHA-256 `e9abac37fcfc492cbd04ebabaa784cad7e81f14e84f39776b7b5658d8473611e`. | Proven |
| Complete walkthrough replay | The frozen development Oracle contains 86 typed segments, 11,769 puzzle actions, and 11,683 non-final checkpoints. Every checkpoint matches the original engine and every segment completes its expected puzzle. | Proven |
| Model-facing task and isolated scoring | Final package digest `sha256:5d4b2c72dd4fa69b4f040c768bf226d5dc0eb54731cf2b8119bc9b3fc192e89e`; Harbor Oracle job `2026-07-21__05-15-38` scored 86 with no exception and Nop job `2026-07-21__05-17-10` scored zero. | Proven |
| Common live sidecar subscription API | All three dataset tasks serve `GET /v1/observe/snapshot` and cursor/long-poll `GET /v1/observe/events`. Current live containers returned `benchmark-observer-snapshot-v1`, complete state, and CORS from ports 3731–3733. A real Parabox Agent client released a blocked poll with sequence 2. Relay tests passed 3/3. | Proven |
| All current benchmark tasks integrated | `dataset.toml` contains Parabox, Swarm, and Sausage. Their final digests are `sha256:12579834a970f268d7652d7a7cbe9f0d5d7ffafb00126590490358007f383f8c`, `sha256:bfc9a0777f9c5f63b953223f85d6a7a0c31ce8824111e9d3cd58acfb76b10819`, and the Sausage digest above. Re-running `harbor add` skipped all three as current. | Proven |
| Unified observation platform | The console renders a task wall, game-specific state, latest action/result, and merged cross-task timeline; subscriptions use independent cursors and abortable reconnect loops. Build, 2 rendered-product tests, and ESLint passed. Exact commit `975c99b97c393ea2685b3630f5d84728e28e7cdd` is deployed privately as Sites version 2 at `https://benchmark-live-ops-3720.zuozijian1994.chatgpt.site`. | Proven |
| Task packaging and repository gates | Every `ci_checks/check-*.sh` script passed against all three tasks. Current Parabox final-package Nop job `2026-07-21__05-26-09` completed with reward zero and no exception; Swarm final-package Oracle job `2026-07-21__05-05-21` scored 997,627 at tick 2,374 with no exception. | Proven |

## Scope and size

There is no clean PR baseline: the parent worktree already contains unrelated
user changes and the observer console is an independent nested Sites repository.
Against the parent repository's current `HEAD`, the complete tracked worktree
diff is 12 files, 137 insertions, and 422 deletions. The untracked inventory is
1,646 paths (about 314 MB), dominated by frozen task campaign data, packaged
native binaries, and retained acceptance evidence.

The goal-scoped production surfaces (Sausage engine/server/verifier, observer
relay, Parabox and Swarm observer integration, thin Agent clients, and observer
console) total 9,938 code lines across 26 source files. These paths are absent
from the parent `HEAD`, so the parent-repository comparison is 0 to 9,938 code
lines. The independently versioned console's final documentation/metadata
follow-up changed 49 lines and deleted 87 starter lines; its deployed
application surface is 1,988 code lines across four files.

Copyrighted Sausage campaign data and the derived Oracle remain local and are
not approved for public redistribution.
