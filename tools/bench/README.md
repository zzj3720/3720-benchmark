# bench

One entry point for scored runs and task packaging. It wraps
`tools/observer/harbor-run`, so every run is recorded in the run journal and
published to the live console.

```sh
tools/bench/bench run new sokoban --profile pi-deepseek-v4-flash --account main
tools/bench/bench run status
tools/bench/bench run pause <run>            # stop the segment, then checkpoint it
tools/bench/bench run resume <run> --account backup --note fix.md
tools/bench/bench check                      # before committing task changes
tools/bench/bench package sokoban            # rebuild a task from games/sokoban
tools/bench/bench audit <run>
```

## What a run records

`bench run new` refuses to start while `tools/`, `games/`, `tasks/`,
`observer/` or `dataset.toml` have uncommitted changes. The run executes
exactly the recorded commit, so no code is copied into the run directory.

`runs/<run>/run.json` (`benchmark-run-v1`) holds the commit, the task digest,
the profile digest and one entry per segment: Harbor job, agent class,
account, commit, notes and the checkpoint it resumed from. Harbor jobs go to
`.harbor/jobs/`, so the journal is always `.harbor/run-journals/<run>` and the
live publisher sees it without mirroring.

## Games and profiles

- `games/<game>/run.toml` — task path, objective template (`{session}` is the
  agent's native session name), campaign kwargs for agents that need them, and
  the task artifacts that make up the game's checkpoint state.
- `tools/agents/profiles/<profile>.toml` — agent class, model, kwargs, required
  environment variable names, network hosts, logs to collect, where the native
  session lives in the agent logs, per-game start agents, and per-game resume
  agents.

Profiles name environment variables, never values. `--account NAME` reads the
values from `~/.config/3720-benchmark/accounts.toml`:

```toml
[main.env]
CODEX_AUTH_JSON_PATH = "/Users/me/.config/3720-benchmark/main/auth.json"

[backup.env]
CODEX_AUTH_JSON_PATH = "/Users/me/.config/3720-benchmark/backup/auth.json"
```

## Checkpoints and resume

`bench run checkpoint` (or `pause`) writes `runs/<run>/checkpoints/<n>/`
from the stopped segment's trial: the native agent session, the synced
workspace, and the game artifacts declared in `run.toml`. The journal segment
must be sealed. `manifest.json` (`benchmark-checkpoint-v1`, see
`docs/tracks/live-observability.md`) is written last and records a hash for
every artifact; a role the trial did not produce is recorded as `null`.

`bench run resume` verifies those hashes, picks the profile's resume agent for
the game, and passes each checkpoint artifact to the agent keyword it accepts
(`resume_sessions_dir`, `resume_workspace_dir`, `resume_game_state_path`,
`resume_game_audit_path`, `resume_game_events_path`). It fails before launching
if the agent requires state the checkpoint does not hold. `--note` files become
Harbor extra instructions for that segment and are kept with the run.
