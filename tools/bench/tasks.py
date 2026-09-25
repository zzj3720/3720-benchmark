"""Packaging and pre-commit checks for the Harbor tasks generated from games/."""

from __future__ import annotations

import hashlib
import os
import subprocess
import sys
import tomllib
from pathlib import Path


def games_with_tasks(root: Path) -> list[str]:
    return sorted(path.parent.parent.name for path in (root / "games").glob("*/scripts/package_task.sh"))


def package(root: Path, games: list[str]) -> None:
    """Rebuild each game's task package, then refresh dataset digests."""
    for game in games or games_with_tasks(root):
        script = root / "games" / game / "scripts" / "package_task.sh"
        if not script.is_file():
            raise SystemExit(f"{game} has no scripts/package_task.sh")
        print(f"== packaging {game}", flush=True)
        subprocess.run(["bash", str(script)], cwd=root, check=True)
    subprocess.run(["harbor", "sync", str(root / "dataset.toml")], cwd=root, check=True)


def data_mismatches(root: Path, game: str) -> list[str]:
    """Task files that copy games/<game>/data/<path> but differ from it.

    A task file is a copy when its path ends with the data-relative path, for
    example tasks/sokoban/tests/campaign/sokoban.json for data/campaign/sokoban.json.
    """
    data = root / "games" / game / "data"
    task = root / "tasks" / game
    if not data.is_dir() or not task.is_dir():
        return []
    sources = {path.relative_to(data).as_posix(): path for path in data.rglob("*") if path.is_file()}
    problems = []
    for copy in (path for path in task.rglob("*") if path.is_file()):
        relative = copy.relative_to(task).as_posix()
        for suffix, source in sources.items():
            if "/" in suffix and relative.endswith("/" + suffix):
                if hashlib.sha256(copy.read_bytes()).digest() != hashlib.sha256(source.read_bytes()).digest():
                    problems.append(f"{copy.relative_to(root)} differs from {source.relative_to(root)}")
    return problems


def unbuilt(root: Path) -> list[str]:
    """Packaged tasks whose prebuilt binaries (untracked) are missing."""
    return [game for game in games_with_tasks(root)
            if (root / "tasks" / game / "environment").is_dir()
            and not any((root / "tasks" / game).glob("**/bin/*/*"))]


def stale_digests(root: Path) -> list[str]:
    """Tasks whose dataset.toml digest `harbor sync` would change. Leaves the file untouched."""
    manifest = root / "dataset.toml"
    original = manifest.read_bytes()
    try:
        subprocess.run(["harbor", "sync", str(manifest)], cwd=root, check=True, capture_output=True)
        synced = tomllib.loads(manifest.read_text())
    finally:
        manifest.write_bytes(original)
    before = {task["name"]: task["digest"] for task in tomllib.loads(original.decode()).get("tasks", [])}
    return sorted(task["name"] for task in synced.get("tasks", []) if before.get(task["name"]) != task["digest"])


def check(root: Path, tasks: list[str]) -> bool:
    names = tasks or sorted(path.name for path in (root / "tasks").iterdir() if (path / "task.toml").is_file())
    ok = True
    # The checks need Python 3.11+ (tomllib); prefer the interpreter bench runs on.
    environment = {**os.environ, "PATH": f"{Path(sys.executable).parent}{os.pathsep}{os.environ.get('PATH', '')}"}
    for name in names:
        task = root / "tasks" / name
        failures = []
        for script in sorted((root / "ci_checks").glob("check-*.sh")):
            result = subprocess.run(["bash", str(script), str(task)], cwd=root, env=environment, capture_output=True, text=True)
            if result.returncode != 0:
                failures.append(f"{script.name}: {(result.stdout + result.stderr).strip().splitlines()[-1:] or ['failed']}")
        failures += data_mismatches(root, name)
        status = "ok" if not failures else "FAIL"
        print(f"{status:4} {name}")
        for failure in failures:
            print(f"     {failure}")
        ok &= not failures
    missing = unbuilt(root)
    if missing:
        print(f"SKIP dataset digests: binaries not built for {', '.join(missing)}; run `bench package`")
    else:
        stale = stale_digests(root)
        if stale:
            print(f"FAIL dataset.toml digests are stale for {', '.join(stale)}; run `bench package`")
            ok = False
    return ok


def audit(root: Path, run_id: str) -> Path:
    """Write results/audits/<run>.json from the run's journal with run-audit."""
    output = root / "results" / "audits" / f"{run_id}.json"
    output.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(
        ["cargo", "run", "--release", "--locked", "--manifest-path", str(root / "observer" / "runtime" / "Cargo.toml"),
         "--bin", "run-audit", "--", str(root / ".harbor" / "run-journals" / run_id), str(output)],
        cwd=root, check=True,
    )
    return output
