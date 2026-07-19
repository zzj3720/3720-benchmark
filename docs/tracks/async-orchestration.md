# Async Orchestration Track

This is one track in 3720 Benchmark. Its constraints apply to async-orchestration
cases, not to every task in the repository.

## Goal

Measure whether an agent can manage a changing software-development workflow,
not whether it possesses rare programming knowledge.

The benchmark is produced by failure mining:

```text
real development run
  -> orchestration failure
  -> frozen world state and event trace
  -> reproducible serial and asynchronous conditions
  -> Harbor task with deterministic verifier
```

## Episode model

One Harbor task represents one complete episode. Subtasks are not separate
Harbor steps because that would force them into a predefined serial order.

An episode contains:

- initial repository and workflow state;
- a virtual clock;
- jobs with dependencies, durations, value, and deadlines;
- simulated CI, review, issue, and message services;
- deterministic workers or collaborators;
- scheduled additions, revisions, cancellations, and failures;
- resource and communication constraints;
- a seeded event generator;
- an append-only trace consumed by the verifier.

The agent receives non-blocking actions such as starting work, reading status,
waiting for the next event, cancelling work, sending messages, and committing
results. Virtual time advances to meaningful events; tasks must never use
wall-clock sleeps to create difficulty.

## Paired conditions

Every scored task family should include at least:

| Condition | Purpose |
| --- | --- |
| Serial control | Establish that the agent can perform the atomic work |
| Async | Measure useful overlap and pending-state tracking |
| Async + change | Measure interruption, cancellation, and replanning |
| Async + collaboration | Measure delegation, communication, and integration |

This pairing supports attribution:

```text
async robustness = async score / serial-control score
change robustness = async-change score / async score
collaboration gain = team score - compute-matched solo score
```

## Initial task families

1. fan-out and fan-in;
2. interleaved pipelines;
3. urgent work arriving during a pending job;
4. cancellation and partial requirement revision;
5. worker failure and reassignment;
6. resource contention and deadlock avoidance.

## Scoring

The main score is normalized utility relative to the scenario oracle. Diagnostic
metrics should include:

- deadline-weighted completion;
- makespan regret;
- runnable-work idle ratio;
- interruption recovery delay;
- duplicate or stale actions;
- deadlock and starvation count;
- useful-message ratio;
- tool calls, tokens, cost, and wall-clock time.

Do not grade one exact action sequence. Multiple schedules can be equally valid.

## First environment boundary

The initial environment may simulate Git, issues, CI, and team messages, but it
must not depend on live GitHub, Linear, Slack, or model APIs. Multi-agent cases
come after single-agent asynchronous scheduling is stable enough to serve as a
control.
