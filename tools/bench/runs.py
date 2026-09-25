"""Scored runs: launch from a clean commit, checkpoint, and resume.

A run lives in runs/<run-id>/ (local, not tracked):

    run.json                     benchmark-run-v1 manifest; segments appended on resume
    segments/<n>/config.json     Harbor job config for that segment
    segments/<n>/harbor.log      Harbor output
    checkpoints/<n>/manifest.json benchmark-checkpoint-v1, written last
    checkpoints/<n>/...          copied session, workspace and game artifacts

Harbor jobs go to .harbor/jobs so the recorder's journal lands in
.harbor/run-journals/<run-id>, where the live publisher reads it.
"""

from __future__ import annotations

import datetime as dt
import hashlib
import importlib
import inspect
import json
import os
import shutil
import signal
import subprocess
import time
import tomllib
from pathlib import Path
from typing import Any

from tools.bench.specs import GameSpec, Profile, load_game, load_profile

RUN_SCHEMA = "benchmark-run-v1"
CHECKPOINT_SCHEMA = "benchmark-checkpoint-v1"
TRACKED = ("tools", "games", "tasks", "observer", "dataset.toml")
ACCOUNTS = Path.home() / ".config" / "3720-benchmark" / "accounts.toml"

# Checkpoint role -> the resume keyword every resumable agent uses for it.
RESUME_KWARGS = {
    "agent_session": "resume_sessions_dir",
    "workspace": "resume_workspace_dir",
    "game_state": "resume_game_state_path",
    "game_audit": "resume_game_audit_path",
    "game_events": "resume_game_events_path",
}


class BenchError(SystemExit):
    pass


def now() -> str:
    return dt.datetime.now(dt.timezone.utc).replace(microsecond=0).isoformat()


# ---------------------------------------------------------------- provenance


def git(root: Path, *args: str) -> str:
    return subprocess.run(["git", *args], cwd=root, check=True, text=True, capture_output=True).stdout


def require_clean(root: Path) -> str:
    """The commit a run executes; refuses local changes to anything a run uses."""
    dirty = git(root, "status", "--porcelain", "--untracked-files=all", "--", *TRACKED).splitlines()
    if dirty:
        listing = "\n  ".join(dirty[:20])
        raise BenchError(f"commit these changes before a scored run:\n  {listing}")
    return git(root, "rev-parse", "HEAD").strip()


def tree_digest(path: Path) -> str:
    """Content digest of a file or directory tree (relative paths and bytes)."""
    if path.is_file():
        return hashlib.sha256(path.read_bytes()).hexdigest()
    lines = []
    for file in sorted(p for p in path.rglob("*") if p.is_file()):
        lines.append(f"{file.relative_to(path).as_posix()}\t{hashlib.sha256(file.read_bytes()).hexdigest()}")
    return hashlib.sha256("\n".join(lines).encode()).hexdigest()


# ---------------------------------------------------------------- manifests


class Run:
    def __init__(self, root: Path, run_id: str):
        self.root = root
        self.id = run_id
        self.dir = root / "runs" / run_id
        self.path = self.dir / "run.json"

    def load(self) -> dict[str, Any]:
        if not self.path.is_file():
            raise BenchError(f"no run {self.id} (expected {self.path})")
        return json.loads(self.path.read_text())

    def save(self, manifest: dict[str, Any]) -> None:
        self.dir.mkdir(parents=True, exist_ok=True)
        temporary = self.path.with_suffix(".tmp")
        temporary.write_text(json.dumps(manifest, indent=2) + "\n")
        os.replace(temporary, self.path)

    def segment_dir(self, index: int) -> Path:
        return self.dir / "segments" / str(index)

    def checkpoint_dir(self, index: int) -> Path:
        return self.dir / "checkpoints" / str(index)


# ---------------------------------------------------------------- configs


def job_config(
    root: Path,
    job_name: str,
    game: GameSpec,
    profile: Profile,
    agent: str,
    extra_kwargs: dict[str, Any],
    extra_instructions: list[Path],
) -> dict[str, Any]:
    kwargs = dict(profile.kwargs)
    if profile.game_kwargs:
        kwargs.update(game.kwargs)
    objective_kwarg = profile.raw.get("objective_kwarg", "goal_objective")
    if objective_kwarg:
        kwargs[objective_kwarg] = game.render_objective(profile.session)
    kwargs.update(extra_kwargs)
    agent_config: dict[str, Any] = {"name": agent, "kwargs": kwargs}
    if profile.model:
        agent_config["model_name"] = profile.model
    if profile.agent_hosts:
        agent_config["extra_allowed_hosts"] = profile.agent_hosts
    if profile.include_logs:
        agent_config["include_logs"] = profile.include_logs
    if profile.env:
        agent_config["env"] = {name: "${" + name + "}" for name in profile.env}
    config: dict[str, Any] = {
        "job_name": job_name,
        "jobs_dir": str(root / ".harbor" / "jobs"),
        "n_concurrent_trials": 1,
        "retry": {"max_retries": 2, "include_exceptions": ["NetworkConnectionError"]},
        "environment": {"type": "docker"},
        "agents": [agent_config],
        "tasks": [{"path": str(root / game.task)}],
    }
    if profile.environment_hosts:
        config["environment"]["extra_allowed_hosts"] = profile.environment_hosts
    if extra_instructions:
        config["extra_instruction_paths"] = [str(path) for path in extra_instructions]
    return config


def account_env(profile: Profile, account: str | None) -> dict[str, str]:
    """Environment the agent needs, from ~/.config/3720-benchmark/accounts.toml or the shell."""
    values: dict[str, str] = {}
    if account:
        if not ACCOUNTS.is_file():
            raise BenchError(f"--account needs {ACCOUNTS}")
        accounts = tomllib.loads(ACCOUNTS.read_text())
        if account not in accounts:
            raise BenchError(f"unknown account {account!r} in {ACCOUNTS}")
        values = {key: str(value) for key, value in accounts[account].get("env", {}).items()}
    missing = [name for name in profile.env if name not in values and name not in os.environ]
    if missing:
        raise BenchError(f"profile {profile.name} needs {', '.join(missing)} (set them or pass --account)")
    return values


# ---------------------------------------------------------------- launching


def docker_config(root: Path) -> Path:
    """A Docker client config without a credential store.

    The macOS keychain helper cannot be unlocked from non-interactive sessions
    (ssh, launchd), which makes every image pull fail; public pulls need no
    credentials. Plugins and the active context come from the user's config.
    """
    directory = root / ".harbor" / "docker-config"
    user = Path.home() / ".docker" / "config.json"
    current = json.loads(user.read_text()) if user.is_file() else {}
    # An explicit empty Docker Hub entry stops the CLI from falling back to
    # the keychain helper for anonymous pulls.
    config = {
        "auths": {"https://index.docker.io/v1/": {}},
        "cliPluginsExtraDirs": [str(Path.home() / ".docker" / "cli-plugins")],
    }
    if current.get("currentContext"):
        config["currentContext"] = current["currentContext"]
        contexts = Path.home() / ".docker" / "contexts"
        if contexts.is_dir() and not (directory / "contexts").exists():
            directory.mkdir(parents=True, exist_ok=True)
            (directory / "contexts").symlink_to(contexts)
    directory.mkdir(parents=True, exist_ok=True)
    (directory / "config.json").write_text(json.dumps(config, indent=2) + "\n")
    return directory



def launch(root: Path, run: Run, index: int, env: dict[str, str], foreground: bool) -> int | None:
    """Start Harbor for a segment; returns its process-group leader, or None once a foreground run ends."""
    segment = run.segment_dir(index)
    command = [str(root / "tools" / "observer" / "harbor-run"), "-c", str(segment / "config.json"), "-y"]
    environment = {
        **os.environ,
        **env,
        "BENCHMARK_CHAIN_ID": run.id,
        "HARBOR_TELEMETRY": "off",
        "PYTHONPATH": str(root),
    }
    environment.setdefault("DOCKER_CONFIG", str(docker_config(root)))
    if foreground:
        subprocess.run(command, cwd=root, env=environment, check=False)
        return None
    log = open(segment / "harbor.log", "ab")
    process = subprocess.Popen(
        command, cwd=root, env=environment, stdout=log, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL,
        start_new_session=True,
    )
    return process.pid


def new_run(root: Path, game_name: str, profile_name: str, run_id: str | None, account: str | None,
            dry_run: bool, foreground: bool) -> Run:
    game = load_game(root, game_name)
    profile = load_profile(root, profile_name)
    commit = "0" * 40 if dry_run else require_clean(root)
    run_id = run_id or f"{game_name}-{profile_name}-{dt.datetime.now():%Y%m%d-%H%M}"
    run = Run(root, run_id)
    if run.path.exists():
        raise BenchError(f"run {run_id} already exists")
    env = {} if dry_run else account_env(profile, account)
    config = job_config(root, run_id, game, profile, profile.start_agent(game_name), {}, [])
    manifest = {
        "schema": RUN_SCHEMA,
        "id": run_id,
        "game": game_name,
        "profile": profile_name,
        "created_at": now(),
        "commit": commit,
        "task": {"path": game.task, "digest": tree_digest(root / game.task)},
        "profile_digest": tree_digest(root / "tools" / "agents" / "profiles" / f"{profile_name}.toml"),
        "segments": [],
    }
    segment = run.segment_dir(0)
    segment.mkdir(parents=True, exist_ok=True)
    (segment / "config.json").write_text(json.dumps(config, indent=2) + "\n")
    manifest["segments"].append({
        "index": 0, "job": run_id, "agent": profile.start_agent(game_name), "account": account,
        "commit": commit, "started_at": now(), "pid": None, "checkpoint": None, "notes": [],
    })
    run.save(manifest)
    if not dry_run:
        manifest["segments"][0]["pid"] = launch(root, run, 0, env, foreground)
        run.save(manifest)
    return run


def stop(root: Path, run_id: str, timeout: float) -> None:
    """Ask Harbor to cancel the active segment; it still collects artifacts."""
    run = Run(root, run_id)
    manifest = run.load()
    pid = manifest["segments"][-1].get("pid")
    if not pid or not alive(pid):
        print(f"{run_id}: no active segment")
        return
    # Interrupt Harbor only: it cancels the trial, collects artifacts, and lets
    # the journal plugin seal the segment. Signalling the whole process group
    # would also kill the recorder before it records segment_finished.
    os.kill(pid, signal.SIGINT)
    deadline = time.monotonic() + timeout
    while alive(pid) and time.monotonic() < deadline:
        time.sleep(2)
    if alive(pid):
        raise BenchError(f"{run_id}: Harbor ({pid}) is still shutting down; check {run.segment_dir(len(manifest['segments']) - 1) / 'harbor.log'}")
    print(f"{run_id}: stopped segment {len(manifest['segments']) - 1}")


def alive(pid: int) -> bool:
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


# ---------------------------------------------------------------- checkpoints


def trial_dir(root: Path, job: str) -> Path:
    job_dir = root / ".harbor" / "jobs" / job
    trials = [path for path in job_dir.iterdir() if path.is_dir() and (path / "config.json").is_file()] if job_dir.is_dir() else []
    if len(trials) != 1:
        raise BenchError(f"expected one trial in {job_dir}, found {len(trials)}")
    return trials[0]


def journal_seal(root: Path, chain: str) -> dict[str, Any]:
    """Last sequence, effective time and segment of the chain's journal."""
    archive = root / "observer" / "runtime" / "target" / "release" / "run-archive"
    if not archive.is_file():
        subprocess.run(["cargo", "build", "--release", "--locked", "--manifest-path",
                        str(root / "observer" / "runtime" / "Cargo.toml"), "--bin", "run-archive"], check=True)
    output = subprocess.run([str(archive), "--cat", str(root / ".harbor" / "run-journals" / chain)],
                            check=True, capture_output=True, text=True).stdout
    last = segment = finished = None
    for line in output.splitlines():
        if not line.strip():
            continue
        row = json.loads(line)
        last = row
        if row.get("type") == "segment_registered":
            segment, finished = row.get("segment_id"), False
        elif row.get("type") == "segment_finished":
            finished = True
    if last is None:
        raise BenchError(f"journal for {chain} is empty")
    return {
        "sequence": last.get("sequence"),
        "effective_elapsed_ms": last.get("effective_elapsed_ms", 0),
        "segment_id": segment,
        "sealed": bool(finished),
        "sha256": hashlib.sha256(output.encode()).hexdigest(),
    }


def checkpoint(root: Path, run_id: str, allow_unsealed: bool = False) -> Path:
    run = Run(root, run_id)
    manifest = run.load()
    index = len(manifest["segments"]) - 1
    segment = manifest["segments"][index]
    if segment.get("pid") and alive(segment["pid"]):
        raise BenchError(f"{run_id}: segment {index} is still running; `bench run stop {run_id}` first")
    game = load_game(root, manifest["game"])
    profile = load_profile(root, manifest["profile"])
    trial = trial_dir(root, segment["job"])
    seal = journal_seal(root, run_id)
    if not seal["sealed"] and not allow_unsealed:
        raise BenchError(f"{run_id}: journal segment {seal['segment_id']} is not sealed (recorder still writing?)")

    target = run.checkpoint_dir(index)
    if target.exists():
        shutil.rmtree(target)
    target.mkdir(parents=True)
    artifacts: dict[str, Any] = {}

    def keep(role: str, source: Path, destination: str) -> None:
        if not source.exists():
            artifacts[role] = None
            return
        copy = target / destination
        copy.parent.mkdir(parents=True, exist_ok=True)
        if source.is_dir():
            shutil.copytree(source, copy, symlinks=True)
        else:
            shutil.copy2(source, copy)
        artifacts[role] = {"path": destination, "sha256": tree_digest(copy)}

    for role, container_path in game.artifacts.items():
        keep(role, trial / "artifacts" / container_path.lstrip("/"), f"game/{Path(container_path).name}")
    if profile.session_dir:
        keep("agent_session", trial / "agent" / profile.session_dir, "sessions")
    else:
        artifacts["agent_session"] = None
    keep("workspace", trial / "agent" / "workspace", "workspace")

    checkpoint_manifest = {
        "schema": CHECKPOINT_SCHEMA,
        "chain_id": run_id,
        "segment_index": index,
        "segment_id": seal["segment_id"],
        "created_at": now(),
        "effective_elapsed_ms": seal["effective_elapsed_ms"],
        "native_session": {"kind": profile.session},
        "run_journal": {"sequence": seal["sequence"], "sha256": seal["sha256"], "sealed": seal["sealed"]},
        "trial": str(trial.relative_to(root)),
        "artifacts": artifacts,
    }
    # The manifest is written last: its presence means the bundle is complete.
    temporary = target / "manifest.json.tmp"
    temporary.write_text(json.dumps(checkpoint_manifest, indent=2) + "\n")
    os.replace(temporary, target / "manifest.json")
    segment["checkpoint"] = str(target.relative_to(run.dir))
    run.save(manifest)
    compact(root, [segment["job"]])
    return target


def verify_checkpoint(directory: Path) -> dict[str, Any]:
    path = directory / "manifest.json"
    if not path.is_file():
        raise BenchError(f"{directory} has no manifest.json; the checkpoint is incomplete")
    manifest = json.loads(path.read_text())
    if manifest.get("schema") != CHECKPOINT_SCHEMA:
        raise BenchError(f"{path}: unsupported schema {manifest.get('schema')}")
    for role, artifact in manifest["artifacts"].items():
        if artifact and tree_digest(directory / artifact["path"]) != artifact["sha256"]:
            raise BenchError(f"{path}: {role} does not match its recorded hash")
    return manifest


# ---------------------------------------------------------------- storage

LOG_SUFFIXES = (".txt", ".log", ".json", ".jsonl")
COMPACT_BYTES = 1 << 20


def compact(root: Path, jobs: list[str] | None = None) -> tuple[int, int]:
    """zstd-compress large agent output logs of finished Harbor jobs.

    Only files directly in each trial's agent/ directory are touched (stdout
    transcripts and trajectories, which repeat the native session). Sessions
    and workspaces stay as they are because resuming reads them.
    Returns (files compressed, bytes saved).
    """
    files = saved = 0
    base = root / ".harbor" / "jobs"
    for job in sorted(jobs if jobs is not None else (path.name for path in base.iterdir() if path.is_dir())):
        job_dir = base / job
        if not (job_dir / "result.json").is_file():
            continue  # still running or never finished
        for log in job_dir.glob("*/agent/*"):
            if not log.is_file() or log.suffix not in LOG_SUFFIXES or log.stat().st_size < COMPACT_BYTES:
                continue
            before = log.stat().st_size
            subprocess.run(["zstd", "-q", "-10", "--long=27", "--rm", "-f", str(log)], check=True)
            saved += before - log.with_name(log.name + ".zst").stat().st_size
            files += 1
    return files, saved


# ---------------------------------------------------------------- resuming


def accepted_kwargs(agent: str) -> tuple[set[str], set[str]]:
    """(accepted, required) keyword names across the agent class's __init__ chain."""
    if ":" not in agent:
        return set(), set()  # Harbor built-ins (nop, oracle) take no resume state.
    module, _, name = agent.partition(":")
    cls = getattr(importlib.import_module(module), name)
    accepted: set[str] = set()
    required: set[str] = set()
    for klass in cls.__mro__:
        init = klass.__dict__.get("__init__")
        if init is None:
            continue
        for parameter in inspect.signature(init).parameters.values():
            if parameter.kind in (parameter.KEYWORD_ONLY, parameter.POSITIONAL_OR_KEYWORD) and parameter.name != "self":
                accepted.add(parameter.name)
                if parameter.default is parameter.empty and parameter.kind == parameter.KEYWORD_ONLY:
                    required.add(parameter.name)
    return accepted, required


def resume_kwargs(agent: str, directory: Path, manifest: dict[str, Any]) -> dict[str, str]:
    accepted, required = accepted_kwargs(agent)
    kwargs = {}
    for role, keyword in RESUME_KWARGS.items():
        artifact = manifest["artifacts"].get(role)
        if keyword in accepted and artifact:
            kwargs[keyword] = str(directory / artifact["path"])
    missing = sorted(keyword for keyword in RESUME_KWARGS.values() if keyword in required and keyword not in kwargs)
    if missing:
        raise BenchError(f"{agent} requires {', '.join(missing)}, which the checkpoint does not contain")
    return kwargs


def resume(root: Path, run_id: str, account: str | None, notes: list[Path], foreground: bool, dry_run: bool) -> int:
    run = Run(root, run_id)
    manifest = run.load()
    previous = manifest["segments"][-1]
    if not previous.get("checkpoint"):
        raise BenchError(f"{run_id}: segment {previous['index']} has no checkpoint; run `bench run checkpoint {run_id}`")
    directory = run.dir / previous["checkpoint"]
    checkpoint_manifest = verify_checkpoint(directory)
    game = load_game(root, manifest["game"])
    profile = load_profile(root, manifest["profile"])
    commit = "0" * 40 if dry_run else require_clean(root)
    env = {} if dry_run else account_env(profile, account)
    agent = profile.resume_agent(game.name)
    index = previous["index"] + 1
    segment_dir = run.segment_dir(index)
    segment_dir.mkdir(parents=True, exist_ok=True)
    kept_notes = []
    for number, note in enumerate(notes):
        kept = segment_dir / f"note-{number}.md"
        shutil.copy2(note, kept)
        kept_notes.append(kept)
    job = f"{run_id}--s{index}"
    config = job_config(root, job, game, profile, agent, resume_kwargs(agent, directory, checkpoint_manifest), kept_notes)
    (segment_dir / "config.json").write_text(json.dumps(config, indent=2) + "\n")
    manifest["segments"].append({
        "index": index, "job": job, "agent": agent, "account": account, "commit": commit,
        "started_at": now(), "pid": None, "checkpoint": None,
        "resumed_from": previous["checkpoint"],
        "notes": [str(path.relative_to(run.dir)) for path in kept_notes],
    })
    run.save(manifest)
    if dry_run:
        return index
    manifest["segments"][-1]["pid"] = launch(root, run, index, env, foreground)
    run.save(manifest)
    return index


def status(root: Path, run_id: str | None) -> list[dict[str, Any]]:
    runs = [Run(root, run_id)] if run_id else [Run(root, path.parent.name) for path in sorted((root / "runs").glob("*/run.json"))]
    rows = []
    for run in runs:
        manifest = run.load()
        segment = manifest["segments"][-1]
        rows.append({
            "run": run.id,
            "game": manifest["game"],
            "profile": manifest["profile"],
            "segments": len(manifest["segments"]),
            "running": bool(segment.get("pid") and alive(segment["pid"])),
            "checkpoint": segment.get("checkpoint"),
            "commit": segment.get("commit", manifest["commit"])[:10],
        })
    return rows
