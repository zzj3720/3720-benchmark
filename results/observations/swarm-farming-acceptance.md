# Swarm Farming deterministic acceptance

Date: 2026-07-20

## Scope

- Task: `3720/swarm-farming`
- Dataset digest:
  `sha256:bfc9a0777f9c5f63b953223f85d6a7a0c31ce8824111e9d3cd58acfb76b10819`
- Upstream: Swarm `0.8.0.0`, commit
  `da708abbe5a3d3b339f27e2090d92fe9f4f97ea3`
- Frozen scenario SHA-256:
  `7200bf3fc437b7d25017bd453b7a2f39bef5d0ae51f5c65ca852b066fb07ec0d`
- Deadline: 1,000,000 original engine ticks
- Reward: `1,000,001 - win_tick` on success, otherwise zero

The task preserves the official Farming rules, seed, world, inventory, and
ordered objectives. Only the embedded upstream solution is removed. The model
loads programs without consuming game time and advances the authoritative
virtual clock explicitly.

## Deterministic gates

- Every repository static task check passed.
- The packaged scenario hash matches the development source and contains no
  embedded solution field.
- The adapter Oracle wins at tick 2,374 for a reward of 997,627.
- A header-only unsolved audit verifies to zero.
- Malformed JSON and response-tampered audits are rejected.
- Two complete Harbor Oracle audits are byte-identical, with SHA-256
  `8a3b72a812fbe4384c0ca2b581270dcf430ecdd154cc6a042680ca6792be4e6f`.
- The isolated verifier replays every request through a fresh original engine
  state and compares each exact response before calculating reward.
- Packaged `amd64` and `arm64` binaries are native ELF executables. Their
  SHA-256 values are
  `219ddbf608cdec647c5040de57f124bbb520556295501ea7771a41408481a515`
  and
  `f292a6e96d4fa75e9e35148ce93cafbc4cb5dc09be7e571ed516794e732a2668`.
- A live server can restore an append-only audit written after startup. The
  next public request validates its header, replays every exact response from
  the original initial state, and resumes from the reconstructed sequence.
  The permanent container E2E advances a source world to tick 123, restores it
  into a second server, and independently verifies the resulting audit.

## Harbor acceptance

| Trial | Agent | Reward | Win tick | Exceptions |
|---|---|---:|---:|---:|
| `swarm-farming__ZE9mfji` | Oracle | 997,627 | 2,374 | 0 |
| `swarm-farming__aLjCBAq` | Oracle repeat | 997,627 | 2,374 | 0 |
| `swarm-farming__r6v4PXV` | Nop | 0 | — | 0 |
| `swarm-farming__JYVaNwn` | Restorable package Oracle | 997,627 | 2,374 | 0 |
| `swarm-farming__jCL5fCn` | Restorable package Nop | 0 | — | 0 |

The first environment build attempt hit a Docker Hub TLS handshake timeout and
is treated as an infrastructure exception, not a score. Subsequent clean jobs
produced the results above.

## Calibration status

This pass establishes deterministic engine, API, packaging, Oracle, Nop, and
replay behavior. Real-model calibration began with concurrent two-hour
DeepSeek Flash XHigh (Claude Code) and GPT Luna XHigh (Codex) trials. DeepSeek's
first launch hit a truncated HTTP/2 dependency download and was replaced by a
retry-enabled run; the replacement entered the game normally. Both models
produce native CLI sessions plus the sidecar-owned append-only audit while
running. Their final rewards are required before using this pilot to set
difficulty bands or compare models.

The benchmark Agent timeout is now eight hours. Trials that were already live
under the former two-hour task checksum keep that immutable segment deadline;
their native session and sidecar state are the continuation boundary for the
remaining cumulative budget rather than grounds to restart from tick zero.
The instruction now also states 28,800 seconds, removing the stale two-hour
sentence.

This eight-hour setting was later superseded at the repository level by a
240-hour (864,000-second) outer Agent safety ceiling. The terminal Swarm result
at tick 1,000,000 remains final and is not rerun: changing wall-clock allowance
cannot change a world that has reached its frozen deterministic tick deadline.
The timeout-only package has dataset digest
`sha256:4746054236fe857e46a6038c681c68ca1ef31389b053879d0c6f4407df96f3a5`,
task checksum
`ff4e31978e5dbaf2e74934226e9354e7e7cd3eae1ee94796732613b01af7750b`,
and instruction SHA-256
`4186572b9f84adcf0bdb182c75cf632d9b143c859f9f948f898095313b7f2cab`.
A fresh Harbor Nop trial exercised the packaged environment and isolated
verifier with no exception and reward zero.

The Codex continuation adapter restores three private pieces together: the
native session, optional `/app` working files, and the sidecar audit. No public
restore endpoint exists. A live Luna checkpoint captured 1,979 requests plus
its session and workspace; a fresh original engine replayed the snapshot
without divergence to tick 662,889, score zero, with the first objective
(`get_many_lambdas`) complete.

The first completed DeepSeek replacement reached no objective and attempted to
finish after concluding that it could not find a multiplying-lambda strategy.
Its native `/goal` Stop evaluator correctly returned `ok: false`, but Claude
Code treated the evaluator's explanatory JSON as a non-blocking hook validation
error and exited anyway. That result is invalid for persistence calibration.
The shared Claude Goal adapter now detects the rejected Stop event in the
native session and resumes the same session and world until the Goal accepts
completion or Harbor's outer timeout expires. A new Flash trial exercises this
path end-to-end. Under the current operator rule, a model that explicitly says
it refuses or is unwilling to continue is stopped immediately; an unsuccessful
hypothesis or a statement that it has not yet found a solution is not by itself
a refusal.

## First real-model results

| Trial | Model / harness | Reward | Last tick | Completed objectives | Agent tokens |
|---|---|---:|---:|---:|---:|
| `swarm-farming__Tk6mYSh` | GPT Luna XHigh / Codex | 0 | 1,000,000 | 1 / 2 | 80,666,292 |
| `swarm-farming__X2QqwEW` | DeepSeek Flash / Claude Code continuation | 0 | 1,000,000 | 0 / 2 | 32,118,995 |

Luna completed `get_many_lambdas`: the base held exactly 256 lambdas after a
working concurrent farm. It then explored for the remaining curry ingredients,
submitted at the authoritative one-million-tick deadline, and ended without
the final curry objective. The engine therefore correctly returned zero under
the frozen official all-or-nothing reward. Its 2,174 sidecar requests, native
Codex session, workspace checkpoint, final audit, and verifier result are
retained. The final exported review bundle is
`results/sessions/swarm-farming-gpt-5-6-luna-xhigh-r1/`.

The DeepSeek continuation inherited an already exhausted world, confirmed that
the tick deadline made further game progress impossible, and explicitly ended.
It also probed several reset or privileged routes, but every such attempt
failed and neither changed authoritative state nor produced reward. Under the
operator's definition this is not successful cheating. The review bundles for
the initial and Goal-continuation segments are retained as
`results/sessions/swarm-farming-deepseek-v4-flash-xhigh-r2/` and
`results/sessions/swarm-farming-deepseek-v4-flash-xhigh-r3-goal-cont/`.

This pilot exposed a calibration limitation rather than a verifier defect:
official reward cannot distinguish Luna's completed farming objective from a
no-op result. Keep the frozen official reward as the primary score, but record
completed objective count as a diagnostic metric before expanding this game
to a larger model matrix.

## 2026-07-21 observer package update

The live audit header now records the complete deterministic tick-0 state, and
a read-only relay projects that header plus later exact responses into the
common observer schema on internal port 3721. The scoring verifier continues to
accept the additive header field and still replays commands against the pinned
original engine. A live cursor poll returned a real `swarm status` action,
tick 0, and the initial robot without advancing virtual time.

The same E2E found that the thin Agent client only attempted loopback even
though standard Compose uses the `game` service hostname. It now probes both
addresses before sending a request and still never retries a request after it
has connected. The repackaged task digest is
`sha256:bfc9a0777f9c5f63b953223f85d6a7a0c31ce8824111e9d3cd58acfb76b10819`.
A clean Harbor Oracle job, `2026-07-21__05-05-21`, won at tick 2,374 for
reward 997,627 with no exception on the repackaged client, sidecar, relay, and
verifier.
