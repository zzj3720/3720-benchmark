# Real-time games

Real-time games measure whether an Agent can remember several pending jobs,
estimate when each one needs attention, and return at the right wall-clock
time. Kitchen and operator games use this policy; Swarm remains a separate
virtual-time family.

## Clock

- A run is idle until the Agent explicitly starts it.
- After start, state advances against a monotonic wall clock even while the
  Agent reasons, calls another tool, disconnects, or observes another task.
- Every campaign fixes its durations in advance. A model cannot pause, change
  speed, advance time explicitly, or receive latency compensation.
- Durations are deliberately much longer than the source game's human input
  windows so normal model and API latency is small relative to the decisions
  being tested.
- Scheduled world events and continuous processes are deterministic functions
  of the campaign seed and elapsed milliseconds. Before handling a request the
  engine advances through all events due at or before that request timestamp.

## Agent surface

The Agent may:

- observe current state;
- perform one immediate, atomic game action;
- create or cancel one alarm;
- wait to be woken by an alarm it previously created;
- submit the current result.

It may not submit a batch, action queue, future action, conditional trigger,
script, macro, or callback. An alarm stores only a deadline and a short reminder.
Firing or delivering it never reads or mutates game state and never performs a
game action. After waking, the Agent must observe and act for itself.

Each API request contains at most one state-changing game command. The audit
records receipt time and command separately so a trajectory review can reject
automation outside the supplied client. A later harness-level action ticket may
enforce one state change per model turn; the game protocol does not expose an
endpoint that can mint or schedule such tickets itself.

## Replay

The live sidecar records elapsed milliseconds from the start of the run rather
than trusting client timestamps. An isolated verifier starts the same frozen
campaign and advances a pure deterministic engine to each audited timestamp
before applying its one command. It requires exact response equality. Replay
never sleeps.

After replaying the last recorded command, the verifier advances the pure
engine to the frozen shift deadline before computing reward. A run therefore
has a deterministic terminal result even when the Agent makes no more requests
after start; replay does not depend on another live request arriving.

## Observation

The operator-facing observer stream is read-only and outside the Agent surface.
It may continuously show the current state, alarms, calls, orders, units,
actions, and outcomes. Watching a task never pauses it. Observer reads are not
accepted as evidence of an Agent observation and cannot deliver alarms to the
Agent. Once a run starts, a one-second read-only clock publisher emits current
snapshots so autonomous travel, work, decay, arrivals, and deadlines stay
visible without manufacturing Agent commands or scoring-audit entries.

## Mixed runs

Mixed evaluations share a wall-clock start and a single Agent attention loop,
but every game keeps its own rules and state. A kitchen continues cooking while
the Agent handles a call; an incident continues worsening while the Agent
returns to an oven. Alarm delivery identifies only the originating task and
reminder, after which the Agent decides what to inspect and what single action
to take.
