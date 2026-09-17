# Emergency Operator acceptance

Date: 2026-07-23

## Frozen benchmark

- Task: `3720/emergency-operator`
- Dataset digest:
  `sha256:a7a69bf74367e72cc7401e134672758037d0c4588d81bbf80fce369fc2af09d0`
- Harbor task checksum:
  `a52bc1b85f652364ffa02eb57f97f2d41fae88b85566a34f5ec66b3268482f19`
- Campaign SHA-256:
  `2b3ef5c5326e693e65c24d62e308bb8a60fc9e25f424e495c69fc9ce4dadc9b2`
- Campaign: five chapters, 14 duties, 60 phone calls, 42 CAD reports,
  six response units, and a generated score ceiling of 34,405
- Runtime: one continuous 30-minute monotonic wall-clock shift

The campaign compiler maps the owned-install career's 9,330,000 ms schedule
onto 1,800,000 ms with the exact ratio `60/311`. Absolute arrivals, duty
boundaries, action windows, responder work, and scene timers use that ratio.
Vehicle speed and health-decay rates use its reciprocal. Work-growth rates are
unchanged because both work and elapsed time scale together. The generated
campaign and both isolated task copies are byte-identical.

## Behavior and replay gates

- A run remains idle until the Agent explicitly starts it.
- The client cannot pause, change speed, advance time, submit a future action,
  or batch operations. Every mutation is one immediate command.
- Reminder alarms can wake the Agent but cannot inspect state or execute an
  action.
- The GoalPi lifecycle accepts completion only after terminal evidence already
  observed in a successful Operator tool response. A final answer before that
  point is recorded as a premature stop and resumes the same native Pi session;
  an API/provider interruption is recorded separately. Continuation starts only
  after the current Pi process has actually exited, never while it is active.
- The Rust server uses monotonic wall time. The isolated Rust verifier replays
  every audited command at its recorded elapsed millisecond without sleeping,
  requires exact response equality, and advances to the fixed 1,800,000 ms
  deadline before scoring.
- Fifteen engine tests, the owned-content consistency test, the real HTTP
  alarm/observer/verifier E2E, strict Clippy, deterministic import/build checks,
  and all repository task checks pass.
- The reference scheduler enters all 102 event graphs without wall-clock sleep.
  Its intentionally greedy resource policy resolves 43 incidents, loses 59,
  and scores 8,244/34,405; it is a coverage harness, not a maximum-score Oracle.
- Render QA derives all samples from campaign arrivals, ETAs, and timers rather
  than fixed pre-scale timestamps.

The packaged server SHA-256 values are
`4dc5fea0b21eee19f5fc7d01640216c305d7d29f50af1aeebfdeb8d484633be5`
(`amd64`) and
`449c5021af6553b4e25ab79124c6b1b0cc384aeca54b7dd077ba6dc2a07142ff`
(`arm64`). The verifier values are
`341019f690161b0762ac76a87a33274df3f3f1afbfe3f0a5509f45dbd526cde8`
and
`69eaa271b90ec8abe76defac175b701a0d298c250530cda7d9941e474a107c99`.

## Harbor acceptance

| Job | Agent | Reward | Replayed commands | Exceptions |
|---|---|---:|---:|---:|
| `operator-30m-accepted-oracle-20260723/emergency-operator__2v8C2ge` | Oracle | 120 | 15 | 0 |
| `operator-30m-accepted-nop-20260723/emergency-operator__2akvn7J` | Nop | 0 | 0 | 0 |
| `operator-30m-goal-pi-deepseek-v4-flash-xhigh-r3/emergency-operator__Q2aoNee` | Pi / DeepSeek V4 Flash | 9,528 | 386 | 0 |

The positive Oracle uses only the model-facing `operator` client and a real
alarm wait. It proves the packaged Agent, game sidecar, wall clock, artifact
collection, and separate verifier path; it is not presented as an optimal
34,405-point policy. Nop verifies that an untouched run scores zero.

The Pi trial completed the full authoritative 1,800,000 ms shift before its
final answer. Its Goal lifecycle recorded `completed: true` and zero premature
finals; `operator submit` supplied both `complete: true` and
`shift.status: complete`. Pi auto-compacted once with no context error. Harbor
recorded 912,900 ordinary input tokens, 174,731,648 cache-read tokens, 55,157
output tokens, and a provider-reported cost of $0.6324985744.

An earlier diagnostic run exposed that full historical calls, transcripts,
incidents, and alarms were repeated in every state response until the request
exceeded the model's 1,048,565-token context limit. State projection now keeps
the current duty, unresolved cross-duty incidents, active calls, and pending
alarms; the terminal response is under 4 KB. GoalPi also reserves 65,536 tokens
for proactive Pi compaction and recognizes both raw API JSON and a successful
concise terminal projection already observed by the model. Six adapter tests
cover terminal proof, premature-final continuation, current-segment stop
attribution, interrupted segments, concise successful projections, and
failed/nonterminal projections.

## Remaining benchmark-readiness work

The server currently starts only with new audit and observer files; it does not
resume an interrupted live shift from a saved authoritative snapshot. A
30-minute trial is playable and verifiable as packaged, but production runs
should not be auto-resumed after a sidecar restart until state recovery is
implemented and tested.
