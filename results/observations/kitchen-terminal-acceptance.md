# Kitchen Terminal acceptance

The current Level 6 task was accepted on 2026-07-25 against task digest
`sha256:e5f19b36446d9e89904a19cfba0aa0a4fc45a955e15521597648f7160faaea98`.
Earlier Level 1 calibration evidence is retained below for comparison.

## Level 6 soup difficulty gate

- Frozen original campaign kitchen 6, `1P_SoupKitchenCircle`.
- Onion, mushroom, and tomato soup each require three separately chopped
  ingredients, cooking in a pot, plating, and delivery.
- The 240-second original round is scaled by five to a 20-minute shift. Orders
  open 125 seconds apart and expire after 750 seconds, so deadlines occur
  before the shift ends.
- The three initial plates permit only three deliveries without recycling.
  Subsequent orders require dirty-plate return, sink washing, and explicit
  collection of a clean plate.
- Model-facing orders expose exact ingredient quantities and the authoritative
  root cooking step. The live ticket rail presents the same recipe plan.
- Maximum score is 260; the original star boundaries remain 30, 70, and 120.
- Frozen level SHA-256:
  `8e28d9da02889c5bff662edf677292dbba954727d911406134f7ca22a3b007d4`.

Harbor job `kitchen-terminal-soup-l6-cook-clock-fix-oracle-r1`, trial
`kitchen-terminal__F6tYqoa`, completed without an exception in 18m16s. The
hidden Oracle used only the packaged model-facing client, delivered four soups,
recycled and washed a dirty plate for the fourth delivery, and received reward
88 with two stars. The isolated verifier replayed all 377 timestamped commands
and returned the same score.

Harbor job `kitchen-terminal-soup-l6-cook-clock-fix-nop-r1`, trial
`kitchen-terminal__KP43r9w`, completed without an exception in 56s and received
reward 0.

All 30 engine/API tests pass, including a deterministic full-shift replay of
all 30 imported kitchens. Two real HTTP E2Es cover dual-chef play,
restart/restore, observer output, tamper rejection, exact cooking-fire timing,
and independent verifier replay. Strict Clippy, Rust formatting, and all 21
Harbor static task checks also pass.

### Post-calibration corrections

The initial Level 6 Luna and DeepSeek Flash runs exposed two benchmark faults
and are not comparable scores:

- The live sidecar advanced itself at one-second observation intervals, while
  the verifier advanced directly between Agent command timestamps. Cooking
  ignition and fire spread were not explicit simulation boundaries, so the two
  paths could assign different fire timestamps and responses.
- Pi's persistent-goal completion check rejected a terminal negative score,
  even when the game returned `complete: true`, and resumed the completed Agent
  until its outer timeout.

The engine now schedules cooking ignition and each fire-spread threshold as
exact boundaries. Fire created at a boundary cannot spread retroactively
through the preceding interval, and an extinguished burnt pot cannot ignite
again without new food. Cooking progress starts only when the container's base
and contents resolve to a complete recipe; incomplete soup pots no longer burn.
The Pi completion check and task reward parser accept signed terminal scores
below the task ceiling, while the isolated verifier remains the scoring
authority.

The old mismatching audits were retained as diagnostic evidence rather than
rewritten or accepted through a compatibility fallback. The new public HTTP
fire E2E reproduces the live clock path and requires exact replay by the same
verifier shipped in the Harbor package.

## Historical Level 1 acceptance

The initial task was accepted on 2026-07-25 against task digest
`sha256:aced920f00a5de6725c7385e9ff39e83cd3c0a9711d3a32cd44bd86dc3366c00`.

### Frozen task

- Original Overcooked campaign kitchen 1, `Skyscraper_Test_1p`.
- The original 100-second round, order deadlines, passive systems, and hazards
  are scaled by 12, producing a 1,200,000 ms monotonic-wall-clock shift.
- Campaign SHA-256:
  `ff7ebd5e8bfcbccdedd2c06e96f0592e2930f0f32a4251179f28cb0ffaaacee4`.
- Level SHA-256:
  `54304e24f009a45284e3bc7d020ed0a8bf70787a85efa606af39c0046c4f285e`.
- Every state-changing operation remains immediate and atomic. Reminder alarms
  only wake the Agent; they cannot inspect state or execute an action.

### Planning interface

The model no longer receives a walkable grid or issues one-cell movement.
Every currently reachable station, counter, supply, delivery point, plate
return, and loose item appears in `destinations` with its target id, semantic
kind, and authoritative `travel_ms`. `kitchen go TARGET` commits the active
chef to the current shortest traversable route. The same monotonic game clock
advances during travel, and arrival never performs an interaction
automatically.

Model-facing state does not repeat coordinates, path cells, or inactive
authoring objects. Full map state remains in the sidecar snapshot used by the
observer and deterministic verifier. The public HTTP E2E asserts that the
initial planning response stays below 10,000 bytes.

### Positive and negative E2E

Harbor job `kitchen-terminal-destination-oracle-r4-compact-final` completed
without an exception in 3m55s and received reward 26. The Oracle used only the
model-facing `kitchen` client and the same semantic destination list as an
Agent. It fetched and chopped lettuce and tomato, assembled a plate, delivered
it, and submitted. The isolated verifier replayed the complete timestamped
audit and required exact response equality.

Harbor job `kitchen-terminal-destination-nop-r3-compact-final` completed
without an exception in 45s and received reward 0.

The real HTTP E2E additionally covers wall-clock travel and alarm waits,
sidecar restart from its saved audit, observer snapshots, exact replay, and
rejection of client-supplied timestamps.

### Real Agent calibration

DeepSeek V4 Flash calibration job
`kitchen-terminal-goal-pi-deepseek-v4-flash-xhigh-r5-destinations`, trial
`kitchen-terminal__5fuHnwU`, played the full authoritative shift and produced
terminal `submit` evidence with `complete: true` and score 122 / 338. It
delivered five orders:

| Effective time | Delta | Score |
|---:|---:|---:|
| 5.2 min | +24 | 24 |
| 8.1 min | +24 | 48 |
| 11.5 min | +24 | 72 |
| 14.1 min | +24 | 96 |
| 17.2 min | +26 | 122 |

The audit contains 158 commands: 30 destination choices, 47 waits, 57
interactions, 7 work starts, 6 alarms, 9 explicit state reads, one start, and
one final submit. No grid movement command exists. Committed travel accounts
for 243.0 seconds and held work for 350.3 seconds. Ten failed actions were
seven early checks of an empty plate return, one empty-workstation operation,
and two interactions issued a few seconds before travel completed. These are
time and workflow errors rather than pathfinding failures.

The run used 35,671,291 input tokens, of which 35,173,888 were cache reads,
23,937 output tokens, and $0.1748256664.

The trial's recorded Harbor reward is 0 because the task package was
overwritten with the compact response projection while the 20-minute Agent
segment was still running. The sidecar used the original frozen projection,
while the verifier build picked up the replacement and rejected sequence 2.
Rebuilding the verifier from the trial's locked projection replayed all 158
commands exactly and reported score 122 with three stars. This was an
acceptance-operation version race, not a game or Agent failure; subsequent
gates use isolated job names and do not overwrite a package used by an active
trial.

### Other gates

- Rust: 25 engine/API tests, strict Clippy, and the live HTTP
  kill/restart/replay E2E pass.
- Goal adapter: terminal completion accepts any valid final score below the
  configured 338-point ceiling and rejects premature Agent finals.
- Importer: all 30 kitchens, 120 player variants, 33 scene layouts, and 37
  order nodes match the frozen extraction.
- All 21 Harbor static task checks pass.
- Linux `amd64` and `arm64` clients, sidecars, verifiers, and hidden Oracles
  are packaged as native ELF executables.

### Parallel-chef gate

Accepted on 2026-07-25 after replacing the single global held-work slot with
one slot per chef. Travel, chopping, washing, and extinguishing now continue
for either chef while the Agent controls the other one. `switch` changes only
which chef receives the next atomic command; it does not cancel either chef's
travel or held work. Two chefs cannot occupy the same destination cell or hold
work on the same target.

The public HTTP E2E stages lettuce and tomato at separate boards, starts both
chefs chopping, observes two simultaneous entries in `works`, waits once for
both to finish, then plates and delivers the combined salad. It also restarts
the sidecar and independently replays the timestamped audit.

Harbor job `kitchen-terminal-parallel-chefs-oracle-r1`, trial
`kitchen-terminal__9XG5sFg`, completed without an exception in 2m54s. The
Oracle used only the packaged `kitchen` client, overlapped the first order's
two ingredient workflows across both chefs, and received reward 26. The
isolated verifier replayed the same audit and also reported 26.

Harbor job `kitchen-terminal-parallel-chefs-nop-r1` completed without an
exception in 44s and received reward 0.
