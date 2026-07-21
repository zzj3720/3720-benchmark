# Result tooling

`export_harbor_session.py` turns a single-trial Harbor job into a reviewable,
commit-safe session bundle:

```bash
uv run tools/results/export_harbor_session.py \
  .harbor/jobs/<job> \
  results/sessions/<job>
```

The bundle retains Harbor configuration and results, the normalized ATIF
trajectory, verifier output, exception details, task checksum, and collected
artifacts, including the sidecar-owned Parabox API audit. It redacts common
credential shapes and local home paths.

New runs also retain `parabox-events.jsonl`, the sidecar's structured,
timestamped score and level-selection history. Progress charts can consume this
artifact without parsing model-specific sessions.

`audit_parabox_trajectory.py` checks trajectory commands for hidden-material,
state, solver, network, direct-API, binary-inspection, random-search, and
rate-limit-bypass references. It also reports learning-note references and
writes so prompt-following updates can be reviewed. If the corresponding
sidecar artifacts are present, it summarizes submissions, restarts, failed
requests, accepted/rejected move
counts, attempted batch sizes, and retained per-level action histories:

```bash
uv run tools/results/audit_parabox_trajectory.py \
  results/sessions/<job>/trajectory.json
```

`score_parabox_trial.py` computes the versioned, traceable Parabox score from
Harbor's trial `result.json`:

```bash
uv run tools/results/score_parabox_trial.py \
  .harbor/jobs/<job>/parabox-intro__*/result.json \
  --output results/scores/<job>.json
```

For one logical run resumed across multiple Harbor jobs, pass the ordered
segments as a continuation chain:

```bash
uv run tools/results/score_parabox_trial.py \
  .harbor/jobs/<initial>/parabox-intro__*/result.json \
  .harbor/jobs/<continuation-1>/parabox-intro__*/result.json \
  --chain \
  --output results/scores/<logical-run>.json
```

The chain validates a shared task checksum, model, reasoning effort, and
monotonically non-decreasing cumulative score. Its score is the last segment's
cumulative puzzle count, while token usage is summed across every segment.

The output retains the source SHA-256, exact JSON field paths, trial identity,
raw token values, diagnostic Agent time, integer score, and a traceable rank
key. Every solved puzzle contributes exactly one point. Trials are ranked by
integer score descending, then by total tokens ascending only when scores tie;
token use never changes the puzzle score. Wall time is diagnostic only because
it is sensitive to service stability. Missing token accounting preserves the
verified integer score but makes the trial ineligible for token-based tie
ranking; an infrastructure exception invalidates the score. Neither condition
silently substitutes zeroes. `AgentTimeoutError` is treated separately as the
expected fixed-runtime cutoff: when the verifier still produces a valid
integer reward, that score remains rankable and the cutoff is recorded in the
traceable output.

Install and trial logs, native agent logs, lock files, and credentials are
deliberately excluded. The exported `session-audit.json` retains the native
session SHA-256, extracted actions, and risk flags; keep the ignored raw Harbor
job locally when deeper provider-specific debugging is required.
