# Contributing Cases

This repository accepts Harbor tasks across multiple benchmark tracks.

## Shared acceptance gates

Every scored case must satisfy all of the following:

1. **Clear measurement target.** The task names the capability and track it is
   intended to measure.
2. **Evidence of value.** The case comes from a real workflow, observed agent
   failure, representative dataset, or another documented source—not an
   arbitrary puzzle.
3. **Reproducible environment.** Inputs, state, dependencies, random seeds, and
   resource limits are frozen or generated deterministically.
4. **Outcome-based verifier.** Tests accept valid alternative solutions and do
   not require one reference implementation.
5. **Oracle and Nop.** The reference solution passes; doing nothing fails.
6. **Stable verifier.** Repeated verification is deterministic.
7. **Instruction-test alignment.** Every stated requirement is tested and every
   tested requirement is stated.
8. **Controlled external dependencies.** The task declares its Harbor network
   mode explicitly. Any live dependency must be required by the track and
   designed so results remain reproducible.
9. **No secrets.** Store variable names or safe references, never secret values.

Track-specific gates may be stricter. For example, async-orchestration cases
require a serial control and virtual time; see
[docs/tracks/async-orchestration.md](docs/tracks/async-orchestration.md).

## Add a task

Install Harbor:

```bash
uv tool install harbor
```

Create the standard task structure:

```bash
harbor task init "3720/<task-name>" \
  --include-canary-strings \
  --metadata-template task-template.toml \
  --tasks-dir tasks/
```

Complete:

```text
tasks/<task-name>/
├── instruction.md
├── task.toml
├── environment/
│   └── Dockerfile
├── solution/
│   └── solve.sh
└── tests/
    ├── Dockerfile
    ├── test.sh
    └── test_*.py
```

Use absolute paths in instructions and tests. Declare the runtime network policy
explicitly and avoid live services unless the track requires them. Use a
separate verifier and declare only the artifacts it needs.

## Validate locally

Run the same structural checks as CI:

```bash
for check in ci_checks/check-*.sh; do
  bash "$check" "tasks/<task-name>"
done
```

Run the reference and no-op agents:

```bash
HARBOR_TELEMETRY=off tools/observer/harbor-run \
  --path "tasks/<task-name>" \
  --agent oracle

HARBOR_TELEMETRY=off tools/observer/harbor-run \
  --path "tasks/<task-name>" \
  --agent nop
```

Run the implementation rubric:

```bash
harbor check \
  "tasks/<task-name>" \
  --rubric rubrics/task-implementation.toml
```

Include source evidence, Oracle/Nop results, verifier-stability evidence, and at
least one representative agent trial in the pull request.

## Review principles

- Verify behavior and final state, not private implementation details.
- Avoid arbitrary waits, live dependencies, unseeded randomness, and hidden
  requirements.
- Keep one clear authoritative path through each task environment.
- Do not add a fallback or compatibility branch without evidence that the track
  requires it.
- Version any change that alters verifier semantics.

The generic Harbor automation is documented in
[TASK_REVIEW_AUTOMATION.md](TASK_REVIEW_AUTOMATION.md), and human review guidance
is in [REVIEWING.md](REVIEWING.md).
