# Parabox agent sessions

Only trials run against the isolated game API are retained here. Earlier
embedded-runtime trials were discarded because the Agent could read the
campaign and rules engine, construct exhaustive solvers, patch local binaries,
or modify local state. Their scores are not comparable with the isolated task.

Each future bundle contains the resolved Harbor configuration, normalized ATIF
trajectory, authoritative sidecar state collected by Harbor, verifier result,
and a shortcut audit. `session-audit.json` records the raw native CLI session's
SHA-256, extracted tool actions, and review flags without committing the much
larger provider-specific stream log or credentials. Qoder does not emit an
ATIF trajectory, so its session audit is the reviewable behavioral record.
When ATIF is available, `audit-report.json` additionally checks hidden
materials, network access, solver/runtime shortcuts, direct sidecar access,
rate-limit bypasses, state mutation, and exact Oracle-trace matches.
