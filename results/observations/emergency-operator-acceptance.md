# Emergency Operator real-time acceptance

Date: 2026-07-21

## Scope

- Task: `3720/emergency-operator`
- Final dataset digest:
  `sha256:239d0fcac2d68a77489784188f63d4385cf3a5b9bc998f0be601405eba071b76`
- Final task checksum:
  `a8720a6389441b90350a9f409ab1452394c81f2785edb64944a2d21de1ec870a`
- Frozen campaign SHA-256:
  `ce687bdf8a99216a1b24d94715ce84bb763d01e58e91f417c24e0b371f4eac5e`
- Campaign: one original 25-minute shift, three calls, three incidents, and
  five response units
- Reward: deterministic integer score from 0 through the 660-point campaign
  upper bound

The pilot campaign is original benchmark content and contains no levels,
dialogue, art, audio, or other data from 911 Operator. The campaign provenance
file records that boundary in both the development crate and frozen task.

## Real-time and action-policy gates

- A run remains idle until the Agent explicitly starts it.
- The live server uses a monotonic wall clock. After start, calls arrive, health
  decays, and units travel and work while the Agent reasons or disconnects.
- The client cannot pause, change speed, advance time, or supply a timestamp.
  Unknown request fields, including an injected `at_ms`, are rejected.
- Every mutating request contains one immediate atomic game action. The API has
  no batch, queue, future action, conditional trigger, callback, script, or
  macro surface.
- An alarm stores only its deadline and reminder text. It can wake the Agent
  but cannot inspect state or execute an action. The Agent must observe and
  issue a separate command after waking.
- The packaged client exposes ordinary one-command invocations and a blocking
  alarm wait. Its acceptance E2E measured a real one-second wait before the
  alarm was delivered.

## Engine, API, and replay gates

- The Rust engine covers scheduled and ringing calls, branching dialogue,
  hidden incidents, continuous health decay, role-specific dispatch, unit
  travel/work/return, recall, alarms, terminal scoring, and missed outcomes.
- Eight engine tests and the live HTTP E2E pass. The HTTP E2E performs seven
  separate commands, waits on real time, checks the common observer stream,
  and verifies the resulting audit.
- The isolated verifier starts a fresh frozen campaign, advances directly to
  each server-recorded elapsed millisecond without sleeping, applies one
  command, and requires exact response equality.
- After the final command, replay advances to the fixed shift deadline before
  scoring. A run cannot avoid later losses merely by exiting early.
- A response-tampered audit is rejected by the permanent E2E.
- All repository static task checks pass, and strict Rust Clippy passes for the
  complete crate.
- The packaged server SHA-256 values are
  `3f74684bcab3871e2b35d03f5d39c1180a6fa6e7b48bc6bab8de022a19ab1909`
  (`amd64`) and
  `d51fbb1e42e2f9b7abab6e0cd24e3e1e79152c2daec6936d8de9f1c4ff1cacc7`
  (`arm64`). The verifier values are
  `2b6a152877f5a20de514d65138e9a85b81c6cb839b4de51a0e5e07a1e0df6b39`
  and
  `99011de9a49f707fdd22f6232e6ef6425c0bd07fd49e6b1761b9468dd878f95f`.

## Harbor acceptance

| Job | Agent | Reward | Commands | Exceptions |
|---|---|---:|---:|---:|
| `2026-07-21__21-11-18/emergency-operator__DSNuT9j` | Oracle | 614 | 21 | 0 |
| `2026-07-21__21-32-05/emergency-operator__DQXZkzV` | Nop | 0 | 0 | 0 |

The Oracle used only the model-facing `operator` client. Its Agent phase ran
for 14 minutes 25 seconds of real time, resolved all three incidents, and the
isolated verifier reported `score: 614`, `max_score: 660`, and 21 exact replayed
commands.

The Oracle preceded the final observer-clock publisher and therefore has task
checksum `c9a56b53c1aa0a9930d0863202938f8974fa8922c1b6c24081bcd473b78b1143`.
That additive publisher neither enters the score audit nor changes game state,
the client, campaign, command responses, or verifier. The final package was
then exercised through Compose with the real client: a reminder caused a real
wait, and an autonomous `clock` observer event appeared without adding an
audited Agent command. The final-checksum Nop job above rebuilt and verified
the complete package with reward zero and no exception.

## Live observer

The sidecar publishes the common read-only snapshot and cursor event schemas.
Once a shift starts, one observer-only clock event per second keeps calls,
health, units, alarms, and score current without delivering an Agent alarm or
manufacturing an Agent action.

The gateway normalized a live Oracle container and exposed its score and
objective state. Six Python gateway tests, the frontend build, and six Node
tests pass. Observer commit `85af8d3` is pushed to `sites/main`; private Sites
version 6 is deployed at
`https://benchmark-live-ops-3720.zuozijian1994.chatgpt.site`.

## Kitchen-family continuation

The same boundary has been applied to the first fixed-station kitchen engine.
Its original 20-minute pilot has four staggered orders and four stations.
Starting, finishing, discarding, assembling, and serving are separate manual
actions; food progresses and burns continuously between calls. Five engine
tests and strict Clippy pass. The live sidecar and frozen Harbor package are
the next implementation slice.
