# Parabox v9 sidecar environment observations

Date: 2026-07-20

This log covers environment findings discovered while bringing the isolated
200-level game API from rules v5 through v9. It records environment behavior
separately from model score so a high or low reward is not accepted without
checking how it was produced.

## Baseline invariants

- The Agent image contains the `parabox` API client, but no campaign files,
  rules engine, verifier, Oracle, or authoritative action history.
- The `game` sidecar owns the campaign, rules engine, 500 ms cooldown, action
  history, and append-only API audit.
- Harbor collects `/var/lib/parabox/parabox-state.txt` from the `game` service
  only after the Agent phase and uploads it to a separate verifier.
- The verifier replays every retained direction against its own engine and
  campaign copy. Reward is the solved prefix length from 0 to 200.
- `parabox submit` reports the replayed prefix score without ending the task.

## Positive controls

| Control | Expected | Observed |
|---|---:|---:|
| Nop | 0/200 | 0/200 |
| Oracle | 200/200 | 200/200 |

The v9 Oracle made 411 bounded move requests and one non-terminal `submit`.
It completed the Agent phase in 4m04s and consumed 10,013 directions; all 412
audit entries returned success before isolated verification awarded 200/200.
Its busiest level retained 152 directions. The v9 Nop made no API request and
received 0/200. Both controls used campaign `parabox-mainline-200-v9` and state
format `parabox-state-v3`. After the learning-note prompt update, both controls
were rerun successfully with task checksum
`44c0bc5e0190f8a7fa99ba0cf8dc32b0147020469f74cb8d256403952ac12dd2`.

An initial DeepSeek v9 pilot was stopped after 14 minutes when the instruction
was expanded to encourage reusable learning notes and prompt updates after new
evidence. Flash had replayable progress 1/200 and Pro 2/200; neither result is
ranked because subsequent combinations would have received a different task
checksum. The runs remain environment evidence only.

## Environment findings

### ENV-001: setup allowlist would otherwise leak into the Agent phase

- First observed: before the model matrix.
- Behavior: Harbor merges `--allow-environment-host` entries into the
  environment baseline. Without an explicit `[agent]` phase policy, package
  installation hosts such as `raw.githubusercontent.com` and
  `registry.npmjs.org` remain reachable while the Agent is solving.
- Impact: an Agent could fetch public walkthrough material if it knew or
  discovered a matching URL.
- Disposition: fixed before model trials by declaring an empty allowlist for
  both Agent and verifier phases. Model API hosts are added only through
  run-specific `--allow-agent-host` entries.

### ENV-002: fully parallel Codex bootstrap is nondeterministic

- First observed: initial 14-combination matrix launch.
- Behavior: launching 12 fresh Codex containers simultaneously produced NVM
  download failures and incomplete npm installs, including missing Codex
  platform packages. Affected trials exited before a valid Agent trajectory
  could be evaluated.
- Impact: these trials are infrastructure errors, not zero-level model scores.
- Reproducibility: multiple combinations failed during the same launch while
  a smaller subset completed installation.
- Reproduction in v7: a 20-second Codex start stagger reduced contention but
  did not eliminate it. Terra Low, Terra Medium, and Sol High all failed while
  fetching `nvm.sh`, before Agent execution or verification began.
- Disposition: invalid trials are excluded. Launches are staggered during
  bootstrap while already-started Agent phases continue in parallel; any
  remaining setup failures are rerun sequentially after the initial bootstrap
  wave.

### ENV-003: Codex auth must be selected explicitly

- First observed: initial 14-combination matrix launch.
- Behavior: Harbor 0.20.0 defaults the Codex adapter to `OPENAI_API_KEY`.
  This machine authenticates Codex through its local `auth.json`; without
  `CODEX_FORCE_AUTH_JSON=1`, the adapter created an empty API-key credential
  and some trials received HTTP 401 responses.
- Impact: affected trials are infrastructure errors, not zero-level model
  scores.
- Reproducibility: confirmed from the adapter's auth-selection path and trial
  error output.
- Disposition: corrected Codex launches explicitly select the existing
  `auth.json`. Secret contents are never copied into this repository or its
  observation log.

### ENV-004: network isolation changes attempted debugging paths

- First observed: DeepSeek V4 Pro, initial isolated matrix.
- Behavior: after the first level, the Agent inspected listening sockets and
  attempted to install binary-inspection/network tools. Agent-phase egress
  correctly blocked Debian package repositories, so installation failed.
- Impact: isolation held, but time and tokens were spent probing the API
  transport instead of solving. This behavior is part of the model trajectory
  and should be reported separately from an environment malfunction.
- Reproducibility: visible in the captured terminal and structured trajectory.
- Disposition: no task change. Continue auditing for attempts that could
  bypass the authoritative sidecar; the server-side cooldown and replay
  verifier remain effective even if the client protocol is inferred.

### ENV-005: per-call rate limiting needs a batch-size bound

- First observed: initial corrected Codex matrix while monitoring authoritative
  sidecar histories.
- Behavior: the server enforced one call every 500 ms but accepted an
  unbounded number of directions in a single `move` call. Two trials recorded
  roughly 200 directions while still on the first level.
- Impact: a generated random direction stream could move brute-force work
  inside one accepted request, defeating the intended cost model. Restarts and
  undos also mutate the replay history, so state alone is insufficient for a
  complete abuse audit.
- Reproducibility: confirmed from the API handler and the live sidecar state.
  The longest canonical one-level walkthrough is 152 directions and remains
  representable as five or fewer 32-direction calls.
- Disposition: all trials against the unbounded API are invalidated. Rules v6
  caps each request at 32 directions, reduces the per-level retained history
  ceiling, and collects an append-only sidecar API audit alongside replay
  state.

### ENV-006: short agent-env values can corrupt captured sessions

- First observed: first final v6 Codex results during JSON validation.
- Behavior: corrected launches selected local Codex authentication with
  `CODEX_FORCE_AUTH_JSON=1`. Harbor treated the agent-env value as sensitive
  and replaced every matching `1` in copied ATIF and native Codex session
  files with `[REDACTED]`, including numeric JSON fields.
- Impact: game scores and sidecar audits remained valid, but the captured
  Codex sessions were not parseable JSON and therefore failed the required
  post-run audit gate.
- Reproducibility: all completed Codex trials using the short value had invalid
  normalized JSON; DeepSeek trajectories without that agent env remained
  valid.
- Disposition: all affected Codex trials are invalidated. Corrected launches
  select the same local credential through `CODEX_AUTH_JSON_PATH`, whose
  path-shaped value cannot collide with JSON numbers. Credential contents
  remain outside the repository.

### ENV-007: Terminus structured-output failures confound DeepSeek Flash

- First observed: final v6 DeepSeek trials.
- Behavior: Terminus-2 requires each model turn to contain a JSON command
  object. During the monitored run, DeepSeek V4 Flash produced 111 parser
  warnings, including 50 turns with no valid JSON object; Pro produced three
  extra-text warnings and no fully unparseable turn over the same early
  period.
- Impact: Flash loses execution opportunities to the Agent protocol before the
  game evaluates any action. Its score therefore combines terminal-game
  reasoning with Terminus structured-output compliance and is not a
  harness-independent model comparison.
- Reproducibility: warning classes and counts are present in the Harbor trial
  log; the successfully parsed trajectory remains valid JSON.
- Disposition: retain the result as a Terminus-2 + model system score and
  report parser failures as a diagnostic. Do not alter the shared game task to
  compensate for one Agent adapter.

### ENV-008: a batch cap alone still permits high-throughput random search

- First observed: corrected v6 Codex matrix, Luna Medium.
- Behavior: the Agent wrote a shell loop that repeatedly restarted the level
  and submitted eight batches of 32 random directions, sleeping 0.51 seconds
  between requests. The monitored run made 228 API requests and 71 restarts
  while remaining near the start of the campaign. A 32-direction request every
  500 ms still permits roughly 64 black-box actions per second.
- Impact: the cooldown and request cap controlled API call frequency but did
  not bound total search work. A model score could therefore reflect scripted
  random exploration rather than game reasoning.
- Reproducibility: the complete loop is present in the captured Agent
  trajectory and the sidecar audit independently records its requests and
  restarts.
- Disposition: rules v7 and v8 temporarily added a cumulative 256-direction
  limit. Rules v9 removes that hard limit because it can prevent a weaker model
  from continuing at all. The 500 ms cooldown and 32-direction request cap
  remain, and the task explicitly explains that random or exhaustive search
  will not scale. Such search is allowed but reported as a trajectory
  diagnostic rather than treated as cheating or an invalid score.

### ENV-009: conservative batch admission violated per-level budget semantics

- First observed: live v7 model matrix, when DeepSeek Flash reached 254/256
  directions on a2 and a 16-direction request was rejected.
- Behavior: v7 compared the entire incoming batch length with the current
  level's remaining budget before executing any direction. A batch that could
  solve the current level within its remaining budget and legally continue on
  the next level was therefore rejected based on directions that belonged to
  the next level.
- Impact: the implementation was stricter than the documented combination of
  cross-level batches and per-level budgets. Near the limit, it could reject a
  valid solution and bias scores downward.
- Reproducibility: a regression fixture places a1 one move from completion at
  255/256, then submits the final a1 move and all 23 a2 moves in one batch. The
  v7 admission rule rejects it; the intended result solves both levels with
  counters 256 and 23.
- Disposition: all v7 model trials are invalidated. Rules v8 corrected the
  accounting, and rules v9 subsequently removed cumulative direction
  accounting entirely. This edge case no longer exists in the current state
  format.

### ENV-010: a 1 GB task memory limit can terminate long Codex sessions

- First observed: v8 Sol High after reaching a replayable score of 18/200.
- Behavior: the Codex process exited with code 137 after roughly 36 minutes.
  It had accumulated 12,605,900 input tokens, including 12,395,264 cached
  tokens, and 39,830 output tokens. Harbor correctly marked the trial as an
  Agent infrastructure error even though the sidecar artifact could replay
  18 solved levels.
- Impact: the trial is invalid, not a score of 18. Exit 137 under the task's
  1 GB memory limit is consistent with an out-of-memory kill.
- Reproducibility: the exit code, token totals, partial artifact, and Harbor
  error are retained in the local v8 job record. Docker exposes about 8.4 GB
  on this 16 GB Mac, so twelve simultaneous long-lived Codex containers also
  exceed the practical host budget.
- Disposition: rules v9 raises the per-trial memory limit to 2 GB. The official
  matrix runs independent trials in bounded parallel waves of three instead of
  launching all twelve Codex containers simultaneously. This preserves
  within-wave parallelism while avoiding a known host-level confound.

### ENV-011: account-level Codex Apps bypass container network isolation

- First observed: first v9 Sol XHigh trial.
- Behavior: the Harbor Codex adapter created a fresh `CODEX_HOME` and disabled
  native web search, but the ChatGPT-authenticated CLI still exposed the
  account-level `codex_apps` MCP server. Sol called the connected GitHub App,
  found a public Parabox solution repository, and explicitly said it would use
  the published direction strings.
- Impact: connector traffic is performed outside the task container, so the
  Docker egress allowlist did not see or block it. The trial jumped from level
  13 and ultimately reached level 141 before termination. Of 141 nonempty
  level histories, 133 exactly matched the canonical Oracle traces. Its score
  is contaminated.
- Reproducibility: the native Codex JSONL records 210 in-progress/completed MCP
  events under server `codex_apps`, including GitHub search and file-fetch
  tools. Luna and Terra had not called MCP before termination, but all three
  were run under the same uncontrolled tool policy.
- Disposition: all three original GPT XHigh trials are invalidated. The
  repository now uses an `IsolatedCodex` Harbor adapter that adds
  `--ignore-user-config` and disables Apps, plugins, remote plugins, browser
  tools, Computer Use, and image generation at the Codex feature layer.
  Corrected trials are admitted only after their native session proves that no
  `codex_apps` call was available or used.
  For the current exploratory test, Luna and Terra resume their unmodified
  sessions at levels 10 and 12. Sol resumes a session truncated immediately
  before it enumerated external tools, paired with a sidecar state replayed to
  the same timestamp at level 10. These continuations are labeled with a tool
  policy boundary rather than treated as clean benchmark trials.

## Monitored model behaviors

### BEH-001: externalized learning can be created and maintained

- First observed: v9 Sol XHigh.
- Behavior: after the learning-note prompt update, Sol created
  `/app/parabox-notes.md`, then updated it twice as later levels exposed new
  recursive entry, exit, portal-separation, and lock-door patterns. Terra
  created a similar note but had not updated it by level 9; Luna had not
  created a note by the same point.
- Interpretation: the prompt produces a measurable behavior difference
  without making note creation a verifier requirement. Session audit reports
  note references and writes so one-time creation can be distinguished from
  later maintenance.

### BEH-002: inspecting the thin client is costly but does not expose the game

- First observed: v9 Luna XHigh.
- Behavior: Luna ran `head` against the ELF client, adding a large binary dump
  to its session and token context.
- Interpretation: the client contains no rules engine, campaign, verifier, or
  authoritative state. The action produced no shortcut and is retained as a
  `binary_inspection_refs` diagnostic plus its natural token cost.

### BEH-003: direct-API probing stayed isolated; a connector did not

- First observed: v9 Terra and Sol XHigh.
- Behavior: Terra tried Google and GitHub searches for walkthrough material.
  Sol tried Google and YouTube, scanned client strings, inferred the local
  socket transport, connected directly to `127.0.0.1:3720`, and guessed
  undocumented `hint`, `solution`, `moves`, and `dump` commands.
- Evidence: Agent egress blocked the unapproved destinations, and the sidecar
  audit recorded all four guessed API commands with exit code 2. Direct socket
  calls still used the same server cooldown and authoritative state. However,
  Sol later used the out-of-band `codex_apps` GitHub connector described in
  ENV-011 and obtained published solution traces.
- Interpretation: the direct socket attempts did not compromise the game
  boundary, but the connector did compromise the information boundary. The
  original score is invalid; network, binary-inspection, direct-API, MCP, and
  Oracle-trace diagnostics must all be reviewed before ranking a trial.

### BEH-004: a normal Codex turn may voluntarily stop while the task remains open

- First observed: isolated Sol XHigh continuation from the clean 10/200
  checkpoint.
- Behavior: Sol remained stuck on b2, exhausted several deliberate attempts,
  probed nonexistent sidecar commands, tried blocked external requests, and
  submitted a 10/200 checkpoint. It then restarted and displayed b2 again
  before returning a normal final message saying it could not complete the
  campaign.
- Evidence: Harbor recorded no exception and no timeout. The continuation ran
  for 8 minutes 23 seconds, used 5,109,988 total tokens across the resumed
  session, and still had most of the 7,200-second task timeout available.
  Its last model call carried 110,773 input tokens, with no intervening
  compaction event. The post-submit restart proves that `submit` did not
  terminate the environment.
- Interpretation: the immediate cause was the model's normal stop decision
  after failed approaches, probably amplified by a large un-compacted
  context. It was not an API, verifier, sidecar, or Harbor termination.
- Disposition: persistence-sensitive Codex experiments use a dedicated
  Goal-resume adapter. The adapter requires the Agent to call `create_goal`
  with a verifiable 200/200 completion condition before it resumes play.
  Goal creation and status remain visible in the native session, so a prompt
  claim cannot substitute for an active persisted Goal.

### BEH-005: the persisted Goal survives compaction and a trial timeout

- First observed: isolated Sol XHigh Goal continuation from the verified
  10/200 checkpoint.
- Behavior: the Agent created the required persisted Goal at the start of the
  continuation, advanced from 10/200 to 35/200, and checked the Goal twice
  near the end. Both checks reported `active`. One context compaction occurred
  during the run. Harbor stopped the Agent at the configured 7,200-second
  timeout rather than after a normal final answer.
- Evidence: independent replay awarded 35 points and identified `c9` as the
  first unsolved level. The sidecar retained 1,549 directions across 35
  nonempty histories and 240 API requests. The run used 26,124,093 input
  tokens, including 24,953,600 cached tokens, and 243,975 output tokens.
  The Goal API itself reported 1,135,656 used tokens and 7,143 seconds at its
  last check. The final model call had 134,886 input tokens against a 258,400
  context window.
- Pollution audit: no post-Goal tool call attempted network access, hidden
  task material, random/exhaustive search, direct sidecar access, binary
  inspection, or external MCP. The resumed session still retains earlier
  blocked Google/YouTube attempts and client probing before the clean
  tool-policy boundary; those actions obtained no external answer. Only six
  of the 35 nonempty histories exactly match the Oracle, which is inconsistent
  with bulk trace replay.
- Disposition: retain the complete native session, sidecar state, append-only
  audit, verifier output, and hashes. Resume the same active Goal from the
  independently verified 35/200 state. Subsequent continuations call
  `get_goal` first and create a replacement only if no unfinished Goal exists;
  a different active Goal is treated as a mismatch.

For each finding, record the trial, first observed time, exact behavior,
environment impact, reproducibility, and disposition.
