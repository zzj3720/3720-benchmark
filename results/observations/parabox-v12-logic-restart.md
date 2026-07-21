# Parabox v12 logic restart

Date: 2026-07-20

This run discards prior model progress and starts every scored trial from a
fresh 0/364 campaign. The game rules and 364-level catalog remain v11; “v12”
identifies the strengthened logic-only prompt and the new agent matrix.

## Task identity and prompt

- Harbor task checksum:
  `7e63d299d7445d32ce0b4109dc44c1ba8cda3fe9981aa140b61ae20512c64c8c`
- Dataset digest:
  `sha256:7ff1d05d8051dbe1e23b55cf865efe770ab8702987f3ed8f9694b1d0db83d43e`
- `instruction.md` SHA-256:
  `b82eefcbe575edb6ec0ee528a07f6949dc9995db0738d4e9a71832c4736d8b85`
- The prompt explicitly requires manual logical reasoning from displayed board
  states, forbids solvers/search scripts/automated planners/external answers,
  requires the greatest possible effort toward all 364 puzzles, and says that
  a partial score or difficult puzzle is not a completion condition.
- Codex trials create one persisted Goal with the same 364/364 completion
  condition. Claude Code 2.1.215 uses its native `/goal`. Qoder CN 1.1.0 has no
  proven native Goal lifecycle, so both Qoder trials use the strengthened
  long-horizon prompt without an external continuation loop.

These identifiers describe the frozen model matrix. The post-run environment
fixes add the documented `select` command and the structured sidecar event
stream; their separately validated task checksum is
`5642387b9aa2b0fd2df2a3ef2dfe8b866b128a7ca14d11b301d03359df1e4beb`,
and the revised instruction SHA-256 is
`9d5440e4408dab7eed2c698c94a7497c3762a3d2a04f0cf54e4696cee39e655e`.

## Controls

Both controls used the task checksum above:

| Control | Reward | Exception |
|---|---:|---|
| Oracle | 364 | none |
| Nop | 0 | none |

The full Rust test suite, including replay of all imported walkthroughs,
completed in 0.17 seconds. The Harbor Oracle agent phase took about 11 minutes
49 seconds because it deliberately traverses the public API and sleeps 550 ms
after every select and 32-move batch. That is a positive API-path E2E, not the
verifier replay time. No privileged Oracle endpoint was added.

## Fresh model matrix

| Trial | Harness | Model configuration |
|---|---|---|
| DeepSeek Flash | Claude Code | `deepseek-v4-flash`, XHigh |
| DeepSeek Pro | Claude Code | `deepseek-v4-pro`, XHigh |
| Kimi K3 | Claude Code | `k3[1m]`, Max, 1,048,576-token context |
| Qoder CN | Qoder CN CLI 1.1.0 | `Qwen3.8-Max-Preview`, Max |
| GPT Luna | isolated Codex 0.144.0 | `gpt-5.6-luna`, XHigh |
| GPT Terra | isolated Codex 0.144.0 | `gpt-5.6-terra`, XHigh |
| GPT Sol | isolated Codex 0.144.0 | `gpt-5.6-sol`, XHigh |
| GLM-5.2 | Qoder CN CLI 1.1.0 | `gm51model`, Max |

Every trial preserves its raw CLI session plus the authoritative sidecar state
and append-only API audit. Agent network access remains allowlisted and the
Agent image contains only the thin game client.

## Final diagnostic results

Every puzzle contributes exactly one point. Elapsed time does not enter the
score; it only defines the fixed 7200-second action cutoff. Token count breaks
ties, but all final puzzle scores are distinct.

| Rank | Model | Score | Agent runtime | Input + output tokens | End condition |
|---:|---|---:|---:|---:|---|
| 1 | GPT-5.6 Sol XHigh | 25 | 120.0 min | 32,818,350 | expected timeout |
| 2 | GPT-5.6 Terra XHigh | 22 | 120.0 min cumulative | 48,019,924 | exact continuation-chain cutoff |
| 3 | GPT-5.6 Luna XHigh | 16 | 120.0 min | 53,966,300 | expected timeout |
| 4 | Qwen3.8 Max Preview | 15 | 120.0 min | unavailable | expected timeout |
| 5 | Kimi K3 1M | 14 | 120.0 min | 6,876,780 | expected timeout |
| 6 | DeepSeek V4 Pro | 9 | 120.0 min | 130,893,680 | expected timeout |
| 7 | DeepSeek V4 Flash | 6 | 72.9 min | 80,275,994 | normal CLI exit after unproductive Goal turns |
| 8 | GLM-5.2 | 3 | 44.5 min | unavailable | Qoder `max_tokens`; verifier image pull failed |

Terra's later diagnostic continuations reached 23, but its first 7200 seconds
end at score 22 and the later point is excluded. Kimi's saved state independently
replays to 14/364. GLM's saved state independently replays to 3/364; its Harbor
verifier failed only because Docker Hub timed out while resolving the verifier
base image. GLM also ran against the frozen prompt that omitted the canonical
`select` syntax, so its score is retained as diagnostic evidence rather than a
fair formal model comparison.

Final native-session audits found no high-risk or successful cheating evidence.
Kimi contains 62 extracted actions and GLM contains 16; both have zero findings.

## Environment observations

### ENV-012: Claude Code installer transport is transient

The first Kimi attempt failed before agent execution because the Claude Code
bootstrap download ended with `curl: (18) HTTP/2 stream ... not closed cleanly`.
The first replacement successfully installed and entered agent execution, then
ended with an upstream `api_error` after 16,606 input and 9,620 output tokens.
The launcher now retries both `NetworkConnectionError` and `UnknownApiError`.
These attempts are infrastructure exceptions and are excluded from model
ranking.

### ENV-015: Kimi Coding Plan quota prevents a valid trial

The next Kimi job exhausted all three configured attempts. Its final API
response explicitly reported that the Coding Plan usage limit for the current
billing cycle had been reached; each attempt reported zero model tokens. No
valid Kimi score exists for this restart, and launching additional instances
before the quota refresh would only repeat the same external failure.

A later user-provided replacement credential was stored only in a separate
ignored, mode-600 local env file and launched the `r4-new-key` trial. That job
successfully entered the real K3 `[1m]` Claude Code agent phase and began
solving puzzles, so ENV-015 applies only to the exhausted earlier credential;
the replacement run is the final valid 14-point matrix result. It used the full
7200-second budget, finished `b6`, entered `b7`, inspected both recursive boxes,
and timed out during continued reasoning rather than voluntarily stopping.

### ENV-013: Qoder CN does not report subscription-model tokens

Qoder CN 1.1.0 emits `stream-json` usage and `modelUsage` objects whose token
fields are all zero even for successful calls. The Harbor adapter deliberately
leaves token fields `null` and records `token_accounting: unavailable`; it must
not receive a zero-token tie-break advantage. Its integer puzzle score remains
observable, but it is ineligible for token-based tie ranking unless Qoder
exposes authoritative accounting.

The configured Qoder model is `Qwen3.8-Max-Preview`; the CN CLI reports the
internal route as `qmodel_preview`. Both names are retained as evidence.

### ENV-016: an apparent Qoder Goal path was a false lead

Early runtime metadata mentioned `goal` and `loop`, and a model-facing smoke
appeared to use proposed `GetGoal` and `UpdateGoal` tools. Direct isolated
reproduction later showed that `/goal set` exits with code 42 before emitting
stdout or stderr, both during and after the long benchmark released its
authentication state. Official tool documentation does not expose either Goal
tool. The experimental adapter and its paused probe state were removed.

Qoder results therefore use the standard persistent CLI session and the
long-horizon benchmark prompt only. They are not presented as native-Goal
runs. ENV-021 records the confirming evidence and official feature-request
status.

### ENV-017: the original overworld maps are not represented

The current campaign preserves each area's name, access dependency, gate
threshold, lookahead, level predecessor, and immediate-successor relationship.
It does not preserve the original walkable overworld/hub layouts, level-entry
coordinates, player position in an area, or spatial gate traversal. `levels`
therefore returns a flat catalog and `select <reference>` jumps directly to any
available puzzle.

This is materially different from the original campaign and narrows the
meaning of “level-selection awareness” to list-based task switching. A faithful
follow-up should model the overworld as separate persistent spaces with movement
and entry actions; adding more conditions to `select` would not recover the
missing spatial information.

### ENV-018: `codex exec` tears down native Goal continuation

GPT Terra's first segment ended normally at 15 points after 56 minutes even
though its persisted Goal remained active. The raw Codex session proves that
Codex itself queued the next Goal turn: immediately after the first
`task_complete`, it wrote a new `task_started` and the internal Goal
continuation message. About 33 ms later, `codex exec` shutdown injected
`turn_aborted`, so Harbor collected a normal-looking partial result.

This is a headless-client lifecycle mismatch rather than a Terra decision.
Codex 0.144.0's Goal runtime starts another turn whenever an active Goal becomes
idle, but the `codex exec` event processor requests shutdown after the original
turn reports `Completed`. The temporary Harbor adapter now detects exactly the
queued-Goal-plus-abort suffix and runs `codex exec resume --last` until the Goal
reaches a terminal state or Harbor's task timeout ends.

Terra's continuation restored the original private sidecar state, append-only
audit, raw Codex session, and 28-line reasoning notebook. Direct sidecar
inspection confirmed that it resumed from the 15-point state at `b5`; no solved
puzzle was replayed. The original Harbor adapter had not retained Codex's
state database, so this first recovery recreated the same objective after
`get_goal` correctly returned no persisted Goal.

A two-turn smoke then tested retaining the Goal database across
`codex exec resume`. Codex correctly completed the first turn, automatically
started the second Goal turn, verified both required files, and marked the Goal
complete. However, `codex exec resume` simultaneously waited for the explicit
prompt turn it had requested and never exited; Harbor reported
`AgentTimeoutError` despite reward 1. This confirms that preserving an active
Goal while also supplying the prompt required by the current headless resume
CLI creates a second lifecycle race. The temporary adapter therefore preserves
the session and external task state but recreates the exact same Goal objective
per CLI segment. Goal accounting is segment-local and must be aggregated from
the authoritative rollout. A second two-turn smoke exercised this recreated-
Goal path end to end: both turns completed through the same resumed Codex
session, the verifier returned reward 1, and Harbor reported no exception.
The detector accepts both observed abort suffixes—with and without the
persisted internal Goal message—because Codex can queue and abort the automatic
turn before that message reaches the rollout.

Terra's second continuation exposed a third shutdown ordering at 23 points:
the rollout ended with `task_complete`, a new `task_started`, `turn_context`,
and the internal Goal continuation message, but no final `turn_aborted`.
`codex exec` had exited after queuing the Goal turn and before writing the
abort event, so the two abort-only checks again misclassified an active Goal
as a normal finish. The detector now also accepts this exact marker-bearing
unfinished-turn suffix; it does not accept an unfinished ordinary prompt turn.
Six focused unit tests cover all three observed orderings and the negative
cases. A fresh two-turn Harbor smoke exercised the revised adapter end to end,
returned reward 1, and reported no exception before the 23-point Terra state
was resumed.

That first resumed campaign probe then revealed a semantic loop: because the
Goal database is segment-local, Terra recreated the Goal and ended the explicit
turn expecting the automatic Goal turn to do the work; `codex exec` aborted
that automatic turn as designed. Two consecutive segments therefore only
confirmed the unchanged 23-point score. The probe was cancelled and is not a
scored continuation. Resume instructions now state that Goal restoration is
setup, that the automatic turn will be aborted, and that concrete task work
must happen in the same explicit turn. A three-phase resumed-session smoke
proved both sides of this boundary: the first explicit segment created and
verified a phase-two artifact, the next resumed segment created the final
artifact and completed the Goal, and Harbor returned reward 1 with no
exception. Terra was then restarted from the clean pre-probe 23-point state.

The corrected Terra turn performed concrete branch work, but Codex emitted a
fourth shutdown shape: after `task_complete`, the Goal runtime logged
`thread ... not found` and could not record any new `task_started` event.
Suffix-only detection therefore remained inherently incomplete. The adapter
now also reads the authoritative lifecycle inside the latest completed turn:
a successful `create_goal` without a later terminal `update_goal` requires
continuation even when no queued-turn event was recorded. A terminal
`update_goal` stops continuation. Eight focused tests cover this lifecycle
fallback and all previously observed suffixes; replaying the actual affected
Terra rollout returns pending continuation.

The long-term implementation should host Codex through a single subscribed
app-server session. In that mode Codex owns all Goal continuation decisions,
while the Harbor adapter only keeps the transport alive and observes terminal
Goal status or timeout.

### ENV-019: raw Codex rollout audit required a new extractor

Codex 0.144.0's persisted rollout stores Code Mode actions as
`response_item` records whose payload is a `custom_tool_call`; the earlier
session auditor only understood `item.started` command events from
`codex exec --json`. It therefore reported zero GPT actions even when hundreds
of game calls were present, which was a false negative in the traceability
pipeline.

The auditor now recognizes raw Codex rollouts, extracts nested Code Mode tool
calls, and distinguishes Goal-control calls from executable commands so the
words “do not run a solver” inside the Goal objective do not become a false
cheating alert. The first corrected live snapshot extracted 340 Luna actions,
233 Sol actions, and 231 actions from Terra's combined original-plus-resumed
session. None contained high-risk evidence; the review-only findings were
reasoning-note edits, one cooldown sleep, and one note read.

### ENV-020: the fixed runtime is a valid benchmark cutoff

The task explicitly asks each Agent to keep working until all puzzles are
solved or the configured runtime ends. Harbor represents that second condition
as `AgentTimeoutError`, then still runs the verifier and captures the integer
score. The earlier scoring tool invalidated every exception, which would have
discarded otherwise complete 16-point Luna, 25-point Sol, and 15-point Qoder
results.

Scoring version 5 treats `AgentTimeoutError` as an expected cutoff when an
isolated verifier establishes state integrity. For new runs it uses the
sidecar-owned event stream to take the last score no later than the exact
configured deadline (`agent_execution.started_at + timeout`), rather than the
later exception-cleanup timestamp. Elapsed duration is not a score factor;
actions beyond the configured cutoff are simply ineligible. Other
infrastructure exceptions still invalidate the score unless an explicit
artifact-verifier recovery supplies equivalent evidence.

### ENV-021: Qoder CLI does not have a native Goal lifecycle

Two isolated Qoder CN smoke trials attempted a proposed `/goal set` entry point
with `GetGoal` and `UpdateGoal` tools. Both exited with code 42 before emitting
stdout or stderr, including a rerun after the long Qoder benchmark released its
authentication state. The failure is therefore deterministic, not a concurrent
login lock.

The [official built-in tool documentation](https://docs.qoder.com/en/cli/sdk/tools)
does not list either Goal tool, and Qoder's
[June 2026 feature request](https://forum.qoder.com/t/feature-request-native-goal-command-for-autonomous-goal-driven-multi-round-execution/10389)
states that the CLI still lacks native autonomous multi-round Goal execution.
Qoder's Quest product is goal-oriented, but it is a different runtime surface.
The invalid adapter was removed and the benchmark matrix again uses the proven
standard Qoder CLI. A post-removal Qwen 3.8 Max Preview smoke completed the
real file-writing task with reward 1 and no exception. A future persistent
version would need an explicitly benchmark-owned Stop Hook/custom-command
implementation and must be labeled as a harness workaround rather than native
Qoder behavior.

### ENV-022: full Harbor Oracle/Nop controls remain valid

After the live adapter and scoring changes, fresh isolated Harbor controls
replayed the packaged task through its real entry points. Oracle solved all
364 puzzles and received reward 364 with no exception; Nop changed no game
state and received reward 0 with no exception. The Rust suite also passed all
17 engine/API tests, the result and adapter suites passed all 41 tests, and all
task static checks passed. This validates the packaged campaign, original
unlock graph, private sidecar, 500 ms API behavior, and verifier independently
of any evaluated model.

### ENV-023: Qoder exposes GLM-5.2 through a legacy backend ID

Passing the display label `GLM-5.2` to Qoder CN 1.1.0 produced a session whose
reported model was `gm51model`. An interactive `/model` refresh then showed
GLM-5.2 as the selected frontier model and persisted exactly
`model.name = "gm51model"` in Qoder's settings. The apparently older name is
therefore the current backend ID, not evidence of a fallback to GLM-5.1.

The first probe trial was stopped before scoring while this mapping was
uncertain. The replacement trial uses the confirmed `gm51model` ID directly,
records GLM-5.2 as its display identity, and entered the real campaign
successfully. Temporary selector and trust-directory changes made during the
probe were restored afterward.

### ENV-024: the task tutorial omitted the canonical level-selection command

After solving `a3`, GLM-5.2 correctly reached `selection_required` and read the
documented commands, but the tutorial listed `levels` without listing
`select <level-reference>`. It then guessed `select`, `start`, and `play` in
one shell command to discover how to enter `a4`. The successful progress is
valid, but this detour measures CLI-interface guessing rather than puzzle
reasoning.

The task instruction now lists the canonical `select <level-reference>` command
and explains that it accepts any available reference returned by `levels`.
Live trial instructions remain frozen, so the change does not alter any active
result; it applies to future formal runs.

### ENV-025: sidecar progress was complete but not self-describing

The original sidecar retained every timestamped API command in
`parabox-audit.tsv` and the final solved order, selected level, and surviving
per-level histories in `parabox-state.txt`. This preserves move attempts,
selections, inspections, undo, restart, and submit calls. However, the audit
did not state the score and selected level before and after each request, while
undo/restart intentionally remove failed histories from the final state. Score
curves therefore required model-specific session parsing.

New task checksum `5642387...` adds the sidecar-owned
`parabox-events.jsonl`. Each append-only `parabox-events-v1` request record
shares its absolute timestamp with the full command audit and stores result
code, argument count, score and selected level before and after the request,
score delta, and solved-level events. The analyzer derives score milestones,
selection milestones, and per-level move/inspect/undo/restart activity directly
from this artifact. Harbor collects it alongside state and audit, the session
exporter preserves it, and the isolated verifier checks its monotonicity and
agreement with final state.

A positive Harbor Oracle E2E forced an Agent cutoff after 72 seconds. The
separate verifier passed and the event artifact contained 149 valid records,
148 requests, no invalid rows or score decreases, 83 selection transitions,
and activity for 56 levels. This proves the real Agent API, sidecar collection,
separate verifier, and analysis path—not only Rust unit functions.

### ENV-026: Harbor can collect child-process actions after its Agent deadline

The same 72-second Oracle E2E exposed an environment-control race. Harbor raised
`AgentTimeoutError`, but the shell process inside the main container continued
calling the game sidecar while Harbor uploaded logs and prepared artifact
collection. The isolated verifier saw a valid final state at 55 points, even
though only 47 points existed at the exact configured deadline; eight points
arrived afterward. This is not model behavior and must not enter a leaderboard.

The structured event stream makes the race directly observable. Scoring v5
binds the deadline to the Agent start plus the configured timeout, hashes the
event artifact, identifies the exact source line at the cutoff, verifies the
final event score against the isolated verifier, and reports the excluded
post-Agent delta. The E2E scoring evidence therefore reports 47 rather than
Harbor's raw 55. A future Harbor runtime fix should kill the full in-container
process group before log upload; until then the scorer is the authoritative
cutoff projection.

### ENV-027: Codex sessions were only copied after a clean Agent exit

Harbor's stock Codex adapter keeps the active rollout under
`/tmp/codex-home/sessions` and copies it into `/logs/agent/sessions` only after
`codex exec` returns. A host, container, or Harbor failure before that cleanup
would therefore retain the streamed `codex.txt` but lose the native resumable
rollout—the exact failure mode the benchmark needs to diagnose.

The isolated Codex adapter now starts a five-second checkpoint loop around
every Codex segment. It copies the live rollout into the Harbor-mounted Agent
log directory while execution is still in progress, then stops the loop after
the stock final copy. The running Sol, Luna, and Terra trials were also given
the same checkpoint loop without restarting them. Their native rollouts were
visible on the host at 2.8 MB, 3.8 MB, and 4.1 MB respectively while the
containers were still active. The new Swarm Farming Luna calibration likewise
persisted a 354 KB live native rollout before completion.

Unit coverage checks the source and destination paths, checkpoint cadence, and
best-effort shutdown command. The complete Agent/result suite passes 56 tests.

### ENV-028: Harbor results did not preserve which local env file supplied Kimi

The first four-hour Kimi continuation restored its native Claude Code session
and entered the real resume command, but every Harbor retry received an
explicit HTTP 403 saying that the current billing-cycle usage limit had been
reached. The provider returned zero tokens. The job config preserved neither
the `--env-file` path nor a non-secret key fingerprint, so its artifacts alone
could not prove which of the two locally configured Kimi accounts had been
used. Inspection then found that the authoritative matrix script still bound
Kimi to `.env`, which then held the earlier key; `.env.kimi` held the later
key. This run is an infrastructure/configuration result, not a zero-score
model result.

A replacement job explicitly binds `.env.kimi`, whose locally checked
fingerprint corresponds to the later user-supplied key. Kimi accepted that
credential, restored the 14-point state and native session, and began emitting
model thinking events without the prior 403. Secret values and full
fingerprints remain outside committed artifacts. Future launch manifests
now record a safe operator-provided credential label so this provenance does
not depend on shell history. Ad-hoc calls receive an explicit
`unlabeled-local-key` fallback rather than recording secret material. The
later key has since replaced the earlier `KIMI_API_KEY` in the single
authoritative `.env`; the redundant `.env.kimi` was removed. The matrix uses
`.env`, labels the credential `later-user-supplied-key`, and no longer retries
quota/authentication errors as if they were transient network failures.

### ENV-029: Qoder could wait forever inside a successful streaming request

GLM-5.2 reached its output limit while reasoning about `a4`. Qoder correctly
inserted a continuation message and opened the next inference request, but that
request returned HTTP 200 headers without completing its body. Qoder's own
transport sets no response-body idle timeout, so the CLI, Harbor trial, and
container all remained healthy while the game received no action. This is a
provider/runtime stall, not a refusal. The response eventually resumed after
about six minutes and GLM progressed from 3 to 5 points, proving that an
arbitrary short inactivity cutoff would misclassify slow reasoning.

The Qoder adapter now supervises the native process. Ten minutes without any
new streamed event terminates only the stalled request and resumes the same
Qoder session, up to eight times. Normal exit—including an explicit refusal—
still ends immediately, while an explicit billing error remains non-retryable.
Supervisor start, exit, status, and timestamp records live in a separate JSONL
artifact. A resumable adapter can also restore the native Qoder directory plus
the exact private state, audit, and event stream into a replacement Harbor
trial. The live GLM run was not interrupted; a complete 4-point checkpoint was
captured before it recovered naturally.

A positive Harbor E2E with the supervised adapter completed
`hello-world__RJTWpFJ` at reward 1 with no exception. Its supervisor artifact
records exactly one start and one clean exit (`code: 0`, `stalled: 0`), its raw
Qoder stream ends in `subtype: success`, and Harbor retained the native session.
This proves the normal production entry path does not create an unnecessary
continuation.

### ENV-030: concurrent Claude Code installation needs trial-level retry

The first fresh DeepSeek Flash and Pro launches both failed before model
execution because the Claude Code bootstrap download ended with a truncated
HTTP/2 stream. Neither trial made a game request, so neither is a model result.
The authoritative launch matrix now retries `NetworkConnectionError` twice.
The initial retry-enabled replacement froze the then-current two-hour
DeepSeek window, created live Flash and Pro trials, and exposed their native
Claude sessions and private sidecar streams while they ran. The task was later
standardized to an eight-hour declared Agent window. The operator subsequently
set the formal continuation-chain budget to 24 hours for every model that had
not explicitly refused to continue. Completed effective Agent time is carried
forward, and each model resumes from its saved native session, workspace, and
private sidecar state until its non-overlapping chain reaches 24 hours.
Infrastructure failures that never enter Agent execution do not consume that
budget. DeepSeek Flash and Pro explicitly refused further work and therefore
retain their shorter final chains rather than being restarted.

The task instruction now states the same 28,800-second limit as `task.toml`;
the earlier static four-hour sentence was removed. The task checksum remains
frozen during the 24-hour experiment; longer continuation segments use an
Agent-timeout multiplier rather than editing the model-visible task between
segments. A shared Claude resume mixin restores the native session and optional
`/app` workspace. Both Kimi and DeepSeek continuation adapters combine that
state with the exact Parabox state, audit, and score-event stream, so provider
segments can be joined without replaying solved levels or contaminating the
session.

### ENV-031: legacy continuation headers caused false zero verifier scores

Early continuation adapters restored the private game state after the sidecar
had already emitted a fresh `sidecar_started` event with score zero. Every
later request still recorded its authoritative `score_before`, score delta, and
resulting score, and the final state contained the correct solved-level set.
The isolated verifier correctly rejected the discontinuous stream, so three
otherwise valid four-hour GPT continuations received raw reward zero.

`ParaboxResume` now validates every event record before a continuation starts.
For this one proven legacy shape only, it rebases the startup record to the
first request's authoritative pre-request score and selection. It then verifies
timestamp and score monotonicity, request-to-request continuity, score deltas,
and exact agreement with the restored final state. Any other discontinuity
fails closed.

The affected Sol, Luna, and Terra results were recovered by copying their
state, audit, and event streams into the same isolated verifier image used by
the task. Each immutable recovery bundle hashes the original Harbor result,
all source artifacts, the normalized event stream, verifier source manifest,
container image, verifier output, and CTRF report. The recovered rewards are
40, 20, and 27 respectively. The ranking scorer accepts a recovered result only
when every attested hash still matches; it does not generally trust a state
file or replace verifier output.

### ENV-032: long event streams exceeded the container exec argv boundary

The first 20-hour GPT continuation attempts failed during setup before Agent
execution because a restored JSONL event stream was embedded in one
`docker compose exec` command. The encoded stream exceeded the platform's argv
limit. These attempts made no game request and consume none of the 24-hour
effective Agent budget.

Restoration now truncates the destination once and appends base64-decoded
32-KiB chunks, then checks the exact byte count inside the sidecar. Focused
tests cover large streams and invalid event continuity. Fresh Sol, Luna, and
Terra continuations passed setup with the same source streams and entered
Agent execution, proving the fix through the real Harbor launch path.

### ENV-033: session-only checkpoints were insufficient for exact continuation

Native CLI sessions preserve provider conversation state, but puzzle notes and
other Agent-created files live in `/app`. A continuation that restores only
the native session and sidecar can therefore lose external memory even though
its score remains intact. This is especially material because the prompt asks
Agents to maintain and update reusable logical discoveries.

Every resumed Parabox trial now copies `/app` to
`/logs/agent/workspace` atomically every 30 seconds while the Agent is running.
The Qoder CN resume adapter accepts a validated workspace directory and
restores it before launching the same native session. A host-side private
checkpoint loop also captures the active Qwen and GLM workspace, native
session, state, audit, and event stream while their older trial images run.
Checkpoint and credential directories are ignored by Git and restricted to
the local user. Future continuation launches collect the workspace and the
`parabox-resume-provenance.json` record alongside the raw provider session.

### ENV-034: the repository timeout is a 240-hour safety ceiling

The earlier 24-hour cumulative continuation plan was replaced before those
chains completed. Both scored game tasks now declare an Agent timeout of
864,000 seconds and state the same value in their model-visible instructions.
The limit is intentionally much longer than the expected useful calibration
window. Operators decide when to interrupt a live run based on completion,
explicit refusal, sustained lack of productive progress, or resource
constraints, while preserving all continuation evidence.

The timeout-only Parabox package has dataset digest
`sha256:423b0c6a7824bb3621e4eecd853be070b48ec3421252d7ea68a1df6b47e4a4c6`,
task checksum
`8d961670802d42d4e4c6cf00be0447972bb1f770dbd1898bae236fc0ca316130`,
and instruction SHA-256
`b4bf2f956df9fc0e1b26223399f1de9bcf783ad681a2754d7b8e337718dc9cb1`.
A clean Harbor Nop trial loaded that package, entered the real environment
path, verified to zero, and ended with no exception. The longer ceiling does
not retroactively alter task checksums or timeout deadlines of already-running
segments.

GLM-5.2 was subsequently stopped by operator decision at 13 points on `b6`.
Immediately before termination, the native Qoder session, `/app` workspace,
state, audit, and event stream were copied to a private mode-restricted
checkpoint and the three trial containers were removed. Qwen3.8 remained
running; Qoder calibration now retains only that model.

Stopping the earlier shared Qwen/GLM host checkpoint loop also exposed why
checkpoint ownership must be per trial. Qwen's already-running `r7` segment
predated the in-container workspace loop, so removing the shared process left
its live `/app` without a second copy even though its native session and game
state remained intact. The container had not failed and no data was lost: an
immediate private snapshot archived its session, workspace, state, audit, and
events, and a Qwen-only atomic checkpoint loop replaced the shared one. New
continuations start their own in-container loop through `ParaboxResume`.

### Operator probe disclosure

At audit timestamp `1784545881468`, the operator issued one `levels` request to
the live Qoder sidecar to confirm the response schema while checking for
overworld data. It succeeded after an intentional cooldown wait but remains an
out-of-band public API action. It is excluded from Agent behavior counts; all
subsequent monitoring reads sidecar files directly and does not call the game
API.

### ENV-014: parallel memory use is lower than the declared limits

With all seven configured agents running, each main container initially used about
145–303 MiB despite the 2 GiB per-container limit. Total Docker use, including
game and network sidecars, remained below 2 GiB of the roughly 7.8 GiB Docker
allocation, with no swap or OOM. The host subsequently reported about 70%
system-wide memory available. After Kimi became quota-blocked, the remaining
six valid runs continued concurrently; resource pressure did not require
throttling.

## Session audit

`tools/results/audit_parabox_session.py` hashes every raw session, extracts
shell/file actions with source line numbers, and flags network tools,
benchmark-internal paths, solver/external-answer terms, interpreter runtimes,
system discovery, and non-game commands. High-risk findings are evidence for
manual review; review-only findings are not treated as cheating by themselves.

The initial live snapshot found no high-risk action in any of the seven
sessions. Commands were limited to the public Parabox client and local notes.
One review-only Luna command was `sleep 0.6`, used after a rate-limit response.
Live snapshots and generated JSON reports are kept under the ignored local
directory `.harbor/live-audit/parabox-v12/`; final downloaded sessions remain
the authoritative evidence. Final exported bundles include
`session-audit.json`, which binds the raw native CLI session by SHA-256 and
retains extracted actions and review flags without committing the much larger
provider stream or credentials. This export path was exercised against both a
successful Qoder CLI Harbor trial and an errored Kimi Claude Code trial.

A later live audit of the five active four-hour trials found zero high-risk
actions across Qwen3.8-Max-Preview, GLM-5.2, GPT Sol, GPT Luna, and GPT Terra.
The sampled sessions contained 8, 17, 336, 494, and 479 extracted actions
respectively. Review-only findings in Codex sessions did not establish a
successful solver, answer lookup, or benchmark-internal access and therefore
are not counted as cheating.

One later Luna audit produced a syntactic high-risk hit because the Agent wrote
the sentence “no search or solver” into its own manual-notes file. The matched
action creates prose notes; it neither executes a solver nor retrieves an
answer. Manual review therefore classifies it as an auditor false positive,
not successful cheating. The six other current live-session audits have zero
high-risk hits, and none of the seven shows a successful score-changing
shortcut.

A later five-model snapshot extracted 632 Sol, 1,142 Luna, 1,016 Terra, 101
Kimi, and 183 Qwen actions. Sol, Terra, Kimi, and Qwen had zero high-risk
matches. Luna had three syntactic matches: two wrote notes explicitly saying
that no solver, search, or external walkthrough was used, and one recreated the
logic-only Goal text that forbids those methods. Manual inspection confirms all
three are negative policy statements rather than executed searches or solvers.
No session shows a successful unauthorized change to game state or score.

## Live behavioral observations

### BEH-001: cooldown misuse can be misdiagnosed as a game defect

Both DeepSeek variants remained on `a3` after the other active harnesses had
passed it through the same isolated API. Their sessions speculated that the
box-entry mechanic was broken even though the Oracle control and other models
demonstrate the mechanic on the same task checksum. DeepSeek Flash also issued
commands such as `parabox restart && parabox move ...`; the second public API
call necessarily lands inside the documented 500 ms cooldown. One live
snapshot contained 10 explicit `rate_limited` responses for Flash and 2 for
Pro. This is currently classified as an Agent tool-timing and attribution
failure, not an environment defect.

### BEH-002: list-based level-selection awareness differs sharply

Only explicit `select` calls beyond mandatory single-choice transitions count
as evidence of elective level switching; automatic immediate successors do
not.

- GPT Sol showed the strongest selection behavior. At 20 points it had made 22
  explicit selections, deliberately preserving `b2`, learning from `b3`,
  switching among core and optional branches, and revisiting `b5`, `b7`,
  `b11`, and `b16` after acquiring relevant mechanics.
- GPT Terra finished with 15 points and 17 explicit selections. Its messages
  explicitly describe leaving constrained boards available, inspecting
  parallel lessons, and returning with new squash or reference mechanics.
- GPT Luna reached 11 points and made eight explicit selections. It explicitly
  postponed `b2`, solved the independently available `b4`, and rotated through
  `b6`, `b2`, and optional `b5`.
- Qoder reached 11 points with only the two mandatory selections `a4` and
  `b1`; all later progress followed immediate successors. Despite prolonged
  difficulty on `b2`, it did not elect to use the other available puzzles.
- The native-Goal DeepSeek trials had not yet reached an elective branch in
  the captured snapshot. Flash remained on `a3`; Pro's one selection was the
  mandatory `a4`, so neither yet provides evidence about branch strategy.

### BEH-003: a persistent Goal can amplify a reasoning dead end

DeepSeek Flash ultimately stopped at 6 points on `a7`. It formed the incorrect
belief that the puzzle was impossible, submitted the same partial score, and
then used later Goal turns to repeat that it had no remaining approach. Those
turns neither changed the game state nor tested a new hypothesis. The completed
Harbor result had no infrastructure exception, but recorded 79,816,859 input
tokens (79,663,872 cached) and 459,135 output tokens.

This distinguishes persistence from productive persistence: native Goal
continuation prevents a voluntary partial answer from ending the run, but does
not itself force information gain. Progress monitoring should therefore report
the current puzzle, time since the last state-changing action, recent restart
and submit counts, and the repeated-failure hypothesis alongside score. A
future harness-level stagnation policy may notify the Agent that no new evidence
has been produced, but must not inject a puzzle hint or silently terminate a
low-scoring model.
