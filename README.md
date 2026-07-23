# 3720 Benchmark

A task bank and review pipeline for benchmarks in
[Harbor](https://harborframework.com) format.

This repository is not tied to one capability, domain, or fixed taxonomy. It can
contain any benchmark that belongs in the shared task bank. Each curated dataset
or track owns its scope, task-selection rules, and metrics while sharing one
Harbor task format and quality bar.

The repository starts from Harbor's official
[benchmark template](https://github.com/harbor-framework/benchmark-template).

## Repository layout

```text
.
├── dataset.toml                 # Umbrella Harbor dataset manifest
├── games/                       # Complete per-game vertical slices
├── tasks/                       # Harbor task bank
│   └── hello-world/             # Toolchain smoke task; not a scored case
├── results/                     # Reviewed scores and acceptance evidence
├── docs/
│   └── tracks/                  # Track-specific goals and case design
├── task-template.toml           # Shared defaults for new tasks
├── rubrics/                     # Proposal, implementation, and trial review
├── ci_checks/                   # Static task checks
└── .github/workflows/           # Oracle, Nop, trial, and review automation
```

Track-specific notes live under [docs/tracks](docs/tracks/README.md).
Multitasking, task interleaving, and asynchronous collaboration are grouped into
one optional track there; they do not define the repository.

Each benchmark game is a complete vertical slice under [`games/`](games/README.md):
its rules engine, API, scoring and replay verifier, authoritative campaign data,
development and packaging scripts, and live-console renderer stay together in
`games/<game>/`. Packaging copies only runtime artifacts and a frozen data
snapshot into the corresponding Harbor task. Raw model trajectories and
recovery workspaces remain local rather than entering the repository.

Each game sidecar also exposes a common read-only state subscription. The
private [live operations console](docs/observer-platform.md) can watch Agent
actions, authoritative environment state, virtual time, progress, and results
across concurrent benchmark runs.

Scored game tasks use a 240-hour Agent timeout as a safety ceiling, not as a
required run length. Calibration runs may be stopped earlier by the operator
when the episode is complete, the Agent explicitly refuses to continue,
progress has become persistently unproductive, or available resources require
it. The saved native session, workspace, authoritative environment state, and
append-only action trace remain the evidence boundary for any early stop.

## Create a task

Install Harbor:

```bash
uv tool install harbor
```

Scaffold a task:

```bash
harbor task init "3720/<task-name>" \
  --include-canary-strings \
  --metadata-template task-template.toml \
  --tasks-dir tasks/
```

Fill in the task's `category`, `tags`, and track-specific evidence, then validate
it:

```bash
for check in ci_checks/check-*.sh; do
  bash "$check" "tasks/<task-name>"
done

HARBOR_TELEMETRY=off tools/observer/harbor-run \
  --path "tasks/<task-name>" \
  --agent oracle

HARBOR_TELEMETRY=off tools/observer/harbor-run \
  --path "tasks/<task-name>" \
  --agent nop
```

Read [CONTRIBUTING.md](CONTRIBUTING.md) before proposing a scored case.

## CI configuration

Pull-request CI never executes a real benchmark. It runs static task checks and
the implementation rubric review only; `tasks/hello-world` is available as a
separate plumbing smoke. Oracle/Nop validation, model trials, and adversarial
trials require an explicit maintainer command or local/dedicated runner.
Automated rubric review requires `ANTHROPIC_API_KEY`; manually dispatched agent
trials use the provider keys configured in `.github/harbor-run-defaults.yml`.
Do not commit secret values—add them as repository secrets only when those
manual workflows are enabled.

See [TASK_REVIEW_AUTOMATION.md](TASK_REVIEW_AUTOMATION.md) for the complete
workflow.

## Dataset status

The root `dataset.toml` contains five game-reasoning cases: the complete
364-puzzle `3720/parabox-intro` campaign, the model-controlled-clock
`3720/swarm-farming` time-planning pilot, and the complete 86-puzzle
`3720/sausage-roll` three-dimensional spatial-planning campaign, plus the
continuous-wall-clock `3720/emergency-operator` dispatch shift and the
progressive 305-level `3720/sokoban` classic box-pushing campaign.
`tasks/hello-world` only verifies the Harbor and CI plumbing and is deliberately
excluded from benchmark results.

Additional curated datasets can later be added as track-specific slices of the
same task bank.

## Template updates

The upstream template can be tracked with:

```bash
git remote add template https://github.com/harbor-framework/benchmark-template.git
git fetch template
```

Merge template updates deliberately; repository-specific verifier, network, and
evidence rules take precedence.
