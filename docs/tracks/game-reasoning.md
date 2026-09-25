# Game Reasoning

This track uses deterministic games as compact, inspectable environments for
agent reasoning. Each game is implemented independently and preserves its own
rules and level progression; mechanics from different originals are not mixed
into one synthetic game.

## Why games

Games provide persistent state, legal actions, progressive teaching, objective
outcomes, and reproducible trajectories without requiring professional domain
knowledge. Later games can add partial observability, images, time, scheduling,
or collaboration while retaining exact state-based verification.

## Case structure

- One Harbor task may contain a continuous sequence of levels when the original
  game teaches mechanics cumulatively.
- The same agent session must carry what it learns into later levels.
- Original level order is preserved when legally available.
- The verifier replays actions against an isolated rules engine; it does not
  trust completion flags written by the agent.
- A terminal track exposes symbolic state. A future visual track may render one
  image per step while using the same underlying game state.

## Patrick's Parabox

`parabox-intro` is the first pilot. Its current rules v11 adapt all 364 original
puzzles as one episode, including the nonlinear main route, challenge branches,
and optional side puzzles. They progressively exercise pushing, recursive entry
and exit, empty and self-referential spaces, box absorption, reference copies,
swapping, centering, cloning, transfer, open-sided spaces, flips, cycles,
multiple players, possession, infinite recursion, priority, extrusion, and
inner push. Its terminal API returns JSON and represents every visible space as
a two-dimensional character array with explicit row and column ordering. It
enforces a shared 500 ms cooldown and accepts at most 32 directions per batch,
with no cumulative action limit. The task tells agents not to use random or
exhaustive search because recursive state spaces are too large, while still
allowing a struggling agent to continue acting. It also requires solving from
the observed game state and self-discovered mechanics instead of retrieving
walkthroughs, solution sequences, or other external answers. The Agent image
contains only a thin API client; a separate sidecar owns the rules engine,
level data, rate limit, authoritative action history, and append-only API audit
collected for verification and session review.

The instruction encourages externalized learning during the continuous
campaign: on complex levels, agents may record concise mechanics, invariants,
and reusable reasoning patterns, update them promptly as new evidence changes
their understanding, and consult them in later analogous situations.

The repository gives the Agent up to 240 hours so a fixed short cutoff does not
become part of puzzle difficulty. This is an outer safety ceiling: calibration
operators may end a run earlier based on observed completion, explicit refusal,
persistent lack of productive progress, or resource constraints. Early
termination never changes the integer score and must preserve the native
session, workspace, game state, audit, and score-event stream.

The Rust source, game unit tests, verifier, canonical campaign, scripts, and
live renderer form one vertical slice under `games/parabox-intro`. A packaging
step cross-builds Linux `amd64` and
`arm64` release binaries into the self-contained Harbor task, so development
tests and Oracle data do not become Agent inputs.

The original level data was extracted from a locally owned Steam build. The
private task records that build and file hashes; do not redistribute the level
files without permission from the copyright owner.

Acceptance evidence is recorded in
[`results/observations/parabox-v11-acceptance.md`](../../results/observations/parabox-v11-acceptance.md).

## Swarm

`swarm-farming` starts the time-planning family with the official Swarm 0.8.0.0
Farming tutorial. It keeps the original engine, deterministic seed, classic
world, initial robot inventory, and ordered objectives: deliver at least 256
lambdas to the base, then make curry. The upstream scenario's embedded solution
is removed.

The model loads a Swarm program and advances a virtual clock explicitly in
bounded tick intervals. Loading, observing, and submitting do not consume
ticks. Advancing is irreversible and stops early only on victory or the fixed
1,000,000-tick deadline. A solution that wins at tick `t` receives
`1,000,001 - t`; a non-winning history receives zero. The task therefore tests
both temporal program design—parallel planting, harvesting, transfer, and
crafting—and metacognitive scheduling of when to observe.

The same 240-hour outer Agent ceiling applies, although the deterministic
one-million-tick world deadline can make an episode terminal much sooner.

The Agent image contains only a thin HTTP client. A sidecar linked against the
pinned original libraries owns game state, tick execution, and an append-only
command audit. The isolated verifier starts the same frozen scenario and
replays every command, comparing the exact response at each step before
computing reward. Both clock policies discussed for the track can share this
single engine operation: the current pilot lets the model request ticks,
whereas a later harness can translate output-token counts into calls to the
same advance function.

Acceptance evidence is recorded in
[`results/observations/swarm-farming-acceptance.md`](../../results/observations/swarm-farming-acceptance.md).

## Emergency Operator

`emergency-operator` starts the real-wall-clock time-planning family with an
original 25-minute emergency dispatch shift. Three calls arrive over time and
may overlap with travel and responder work. Dialogue choices reveal hidden
locations and change health decay; two police units, one fire engine, and two
medical units must cover incidents with different role requirements.

The shift begins only when the Agent explicitly starts it. From then on a
monotonic clock continues through model reasoning, tool latency, disconnection,
and waits. Durations are stretched to multi-minute windows. The Agent may make
one immediate action per request or set a reminder alarm; it cannot pause,
advance time, queue actions, attach an action to an alarm, or submit a future or
conditional operation. Alarm delivery merely wakes the Agent, which must then
observe, decide, and act again.

The live Rust sidecar records each command at its own elapsed millisecond. The
isolated verifier advances the same pure engine to those recorded times,
requires exact response equality, and computes a 0-to-660 terminal score after
advancing to the frozen shift deadline without sleeping. A separate one-second
observer ticker keeps the read-only operations console current without entering
the scoring audit or delivering Agent alarms. The pilot uses only original
benchmark content and contains no 911 Operator data.

Acceptance evidence is recorded in
[`results/observations/emergency-operator-acceptance.md`](../../results/observations/emergency-operator-acceptance.md).

## Kitchen Terminal pilot

`kitchen-terminal` starts the fixed-station kitchen family with an original
20-minute shift, four stations, three recipes, and four staggered orders. Each
component has a ready time and a later burn deadline. The Agent must start it,
return in the valid window, finish it manually, assemble the order, and serve
before the customer leaves. Burnt food blocks its station until the Agent
explicitly discards it.

The pure Rust engine already advances from caller-supplied elapsed milliseconds
and passes the first campaign and timing tests. Its upcoming live sidecar will
reuse the same monotonic-clock, one-action, reminder-only alarm, exact-audit,
and offline-replay boundary as Emergency Operator. The pilot contains no data
from a commercial kitchen game.

## Stephen's Sausage Roll

`sausage-roll` adds a complete three-dimensional spatial-planning curriculum.
Its 86 puzzles were extracted from the user's locally owned Steam installation.
Each runtime level starts from a solution-free original entry state and keeps
the original title, geometry, entities, height, and strict campaign order.

One clean-room Rust engine covers the complete rules surface used by the
campaign: player/fork movement, pushing, turning, sausage rolling and cooking,
grill retreat, skewering, detached-fork motion, gravity, ladders, pivots,
stacks, moving islands, transmitted passive forces, and exits attached to
moving objects. The imported walkthrough is a development and Oracle input
only. It contains 11,769 puzzle actions; compatibility tests compare 11,683
non-final states against checkpoints captured from the owned original engine.
All 86 segments reach their exact completion exit under the Rust engine.

The Agent sees a thin `sausage` client and structured 3D snapshots. The sidecar
owns the rules, frozen campaign, session, audit, and observer stream. Levels
advance in one strict sequence so later puzzles test retention and composition
of earlier mechanics. Each completed puzzle contributes one integer point from
0 through 86. The separate verifier starts from the same 86 solution-free entry
states and requires every audited API response to replay exactly.

The level data remains copyrighted by its owner and is not suitable for public
redistribution without permission.

Acceptance evidence is recorded in
[`results/observations/sausage-roll-acceptance.md`](../../results/observations/sausage-roll-acceptance.md).

## No-Guess Minesweeper

`minesweeper` adds partial observation and explicit constraint deduction through
an original fifty-level campaign. Cadet, Operator, Specialist, Expert, and
Master tiers increase proof length, coupled-frontier width, board size, and mine
density while preserving ordinary Minesweeper rules.

The first reveal and its surrounding 3×3 area are always safe. Its coordinates
become part of deterministic generation: the Rust engine samples candidate
layouts from the frozen level seed and accepts the first layout that its
`local-subset-v1` proof procedure can finish using adjacent-count deductions
and subset differences. The proof must also satisfy the level's frozen
`proof-profile-v1`: initial expansion stays inside a lower/upper band while
deduction rounds, subset rounds, and maximum constraint frontier meet their
minimums. `tier-opening-distribution-v1` additionally bounds the weighted mean
and upper median opening ratios over every possible first click in each tier.
Tests exhaust all 7,038 possible first reveals on every campaign level, so an
Agent never needs luck, an unverified guess, or an easier or anomalously hard
first-click loophole.

The thin `minesweeper` client exposes zero-based reveal, flag, chord, reset,
level selection, and submission commands. The Rust sidecar owns hidden mines,
progress, the append-only command audit, and observer events. The isolated
verifier reconstructs each first-click-dependent board and requires every
recorded response to replay exactly before awarding one point per uniquely
cleared level.

Acceptance evidence is recorded in
[`results/observations/minesweeper-acceptance.md`](../../results/observations/minesweeper-acceptance.md).

## Live observation

All current game sidecars expose the same read-only snapshot and cursor-based
long-poll API. Parabox and Swarm use a small Rust relay over their native
append-only histories; Sausage and Emergency Operator emit the common event
schema directly. Observer reads never mutate the game, consume virtual time,
deliver an alarm, trigger a model cooldown, or enter the scoring audit.

The private operations console follows all enabled tasks concurrently and
renders game-specific state, the latest Agent action, progress, connection
health, and one merged timeline. See
[`docs/live-platform.md`](../live-platform.md) for the protocol and
local port configuration.

## Parabox score

The verifier's raw reward is the integer number of solved puzzles from 0 to
364. Every puzzle contributes exactly one point. Ranked model trials use token
consumption only as a tie-break:

```text
total_tokens = input_tokens + output_tokens
score = solved_levels
rank by score descending, then total_tokens ascending
```

Cached input is already included in Harbor's input-token count and is not
added twice. Token use never changes the integer puzzle score, so an additional
solved puzzle always wins; fewer tokens matter only when two trials solve the
same number. Wall time is recorded for diagnosis but excluded from scoring
because provider and service stability can dominate it. Each score artifact
records its formula version, raw Harbor fields, source path and SHA-256,
integer score, token tie-break, and rank key; missing token accounting or an
infrastructure exception is never replaced with a zero. Missing token
accounting preserves the verified integer score but excludes the trial from
token-based tie ranking; an infrastructure exception invalidates the score.

## Acceptance evidence

In addition to the repository-wide gates, a game case must include:

- rules-engine tests for every mechanic needed by the included levels;
- an end-to-end playthrough through the same command surface given to agents;
- a replay verifier that rejects missing, malformed, and unsolved histories;
- frozen level hashes;
- Oracle success, Nop failure, and repeated deterministic verification;
- at least one real agent trajectory showing whether the case provides useful
  behavioral signal.
