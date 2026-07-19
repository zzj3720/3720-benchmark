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
├── tasks/                       # Harbor task bank
│   └── hello-world/             # Toolchain smoke task; not a scored case
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

HARBOR_TELEMETRY=off harbor run \
  --path "tasks/<task-name>" \
  --agent oracle

HARBOR_TELEMETRY=off harbor run \
  --path "tasks/<task-name>" \
  --agent nop
```

Read [CONTRIBUTING.md](CONTRIBUTING.md) before proposing a scored case.

## CI configuration

Static checks and local Oracle/Nop validation do not require a model API key.
Automated rubric review requires `ANTHROPIC_API_KEY`; optional agent trials use
the provider keys configured in `.github/harbor-run-defaults.yml`. Do not commit
secret values—add them as GitHub Actions secrets when those workflows are
enabled.

See [TASK_REVIEW_AUTOMATION.md](TASK_REVIEW_AUTOMATION.md) for the complete
workflow.

## Dataset status

The root `dataset.toml` is intentionally empty until the first real case passes
the acceptance gates. `tasks/hello-world` only verifies the Harbor and CI
plumbing and must not be interpreted as a benchmark result.

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
