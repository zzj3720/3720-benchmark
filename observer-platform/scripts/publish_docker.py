#!/usr/bin/env python3
"""Stage and verify Docker services before switching; roll back on failure."""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import time
import urllib.request

PROJECT = Path(__file__).resolve().parent.parent
ROOT = PROJECT.parent
COMPOSE = PROJECT / "compose.yaml"
STATE = ROOT / ".harbor" / "live-deploy"
LEGACY_LABELS = ("org.3720.benchmark-live-site", "org.3720.benchmark-live-gateway")


def command(arguments: list[str], *, env: dict[str, str] | None = None, capture: bool = False) -> str:
    result = subprocess.run(arguments, cwd=ROOT, env=env, check=True, text=True, stdout=subprocess.PIPE if capture else None)
    return result.stdout or ""


def environment(release: str, candidate: bool) -> dict[str, str]:
    return {**os.environ, "LIVE_RELEASE": release, "LIVE_DATA_ROOT": str(ROOT / ".harbor"),
            "LIVE_WEB_BIND_PORT": "14000" if candidate else "3000", "LIVE_GATEWAY_BIND_PORT": "14740" if candidate else "3740"}


def compose(release: str, candidate: bool, *arguments: str, capture: bool = False) -> str:
    name = "benchmark-live-candidate" if candidate else "benchmark-live"
    return command(["docker", "compose", "-p", name, "-f", str(COMPOSE), *arguments], env=environment(release, candidate), capture=capture)


def read_json(url: str, timeout: int = 30) -> dict:
    with urllib.request.urlopen(url, timeout=timeout) as response:
        return json.load(response)


def compare_runs(before: dict, after: dict) -> None:
    expected = {run["id"]: (run["score"], bool(run.get("live"))) for run in before["runs"]}
    actual = {run["id"]: (run["score"], bool(run.get("live"))) for run in after["runs"]}
    if actual != expected:
        changed = sorted(key for key in expected.keys() | actual.keys() if expected.get(key) != actual.get(key))
        raise RuntimeError(f"candidate run identities/scores/liveness differ: {changed}")


def verify(web_port: int, gateway_port: int) -> dict:
    base = f"http://127.0.0.1:{web_port}"
    if not read_json(f"http://127.0.0.1:{gateway_port}/health").get("ready"):
        raise RuntimeError("gateway index is not ready")
    runs = read_json(base + "/api/live/v1/runs")
    if not isinstance(runs.get("runs"), list):
        raise RuntimeError("missing run list")
    with urllib.request.urlopen(base, timeout=15) as response:
        html = response.read().decode()
    assets = sorted(set(re.findall(r'(?:src|href)="(/assets/[^"?]+)', html)))
    if not assets:
        raise RuntimeError("rendered page has no application assets")
    for asset in assets:
        with urllib.request.urlopen(base + asset, timeout=15) as response:
            if response.status != 200:
                raise RuntimeError("application asset is unavailable")
    with urllib.request.urlopen(base + "/api/live/v1/subscribe?protocol=2", timeout=20) as response:
        while True:
            line = response.readline()
            if not line:
                raise RuntimeError("subscription ended before its initial snapshot")
            if line.startswith(b"data:"):
                event = json.loads(line[5:])
                if event.get("schema") != "benchmark-live-subscription-v2" or not event.get("reset"):
                    raise RuntimeError("invalid initial subscription snapshot")
                break
    return runs


def native_services() -> list[str]:
    if os.uname().sysname != "Darwin":
        return []
    return [label for label in LEGACY_LABELS if subprocess.run(
        ["launchctl", "print", f"gui/{os.getuid()}/{label}"], capture_output=True).returncode == 0]


def stop_native(labels: list[str]) -> None:
    for label in labels:
        source = Path.home() / "Library/LaunchAgents" / f"{label}.plist"
        if source.exists():
            (STATE / "legacy").mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, STATE / "legacy" / source.name)
        command(["launchctl", "disable", f"gui/{os.getuid()}/{label}"])
        command(["launchctl", "bootout", f"gui/{os.getuid()}/{label}"])


def restore_native(labels: list[str]) -> None:
    for label in labels:
        command(["launchctl", "enable", f"gui/{os.getuid()}/{label}"])
        if label not in native_services():
            command(["launchctl", "bootstrap", f"gui/{os.getuid()}", str(Path.home() / "Library/LaunchAgents" / f"{label}.plist")])


def save_state(value: dict) -> None:
    STATE.mkdir(parents=True, exist_ok=True)
    temporary = STATE / "current.json.tmp"
    with temporary.open("w") as output:
        json.dump(value, output, indent=2)
        output.flush()
        os.fsync(output.fileno())
    temporary.replace(STATE / "current.json")


def publish(release: str, skip_build: bool, preview: bool) -> None:
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]{0,100}", release):
        raise ValueError("invalid release tag")
    if not skip_build:
        compose(release, True, "build")
    # The host Harbor recorder must provide the cross-container writer lease.
    command(["cargo", "build", "--release", "--locked", "--manifest-path", str(ROOT / "tools/observer/runtime/Cargo.toml"), "--bins"])
    compose(release, True, "up", "-d", "--no-build", "--wait", "--wait-timeout", "240")
    candidate = verify(14000, 14740)
    try:
        baseline = read_json("http://127.0.0.1:3740/v1/runs")
    except (OSError, ValueError):
        baseline = None
    if baseline is not None:
        compare_runs(baseline, candidate)
    # During the first move, preserve only the native release's public chunks.
    old_assets = Path.home() / ".local/share/3720-benchmark-live/current/dist/client/assets"
    if old_assets.is_dir():
        container = compose(release, True, "ps", "-q", "web", capture=True).strip()
        command(["docker", "cp", str(old_assets) + "/.", f"{container}:/app/dist/client/assets/"])
    if preview:
        print(json.dumps({"release": release, "preview": "http://127.0.0.1:14000", "runs": len(candidate["runs"])}))
        return
    previous = json.loads((STATE / "current.json").read_text()) if (STATE / "current.json").exists() else None
    native = native_services()
    switched = False
    try:
        switched = True
        stop_native(native)
        compose(release, False, "up", "-d", "--no-build", "--wait", "--wait-timeout", "240")
        compare_runs(candidate, verify(3000, 3740))
        save_state({"release": release, "previous": previous["release"] if previous else None,
                    "legacy_services": native, "published_at": int(time.time()), "runs": len(candidate["runs"])})
    except BaseException:
        if switched:
            try:
                compose(release, False, "down")
            finally:
                if previous:
                    compose(previous["release"], False, "up", "-d", "--no-build", "--wait", "--wait-timeout", "240")
                elif native:
                    restore_native(native)
        raise
    finally:
        compose(release, True, "down")
    print(json.dumps({"release": release, "published": True, "runs": len(candidate["runs"])}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--release", default=time.strftime("%Y%m%dT%H%M%SZ", time.gmtime()))
    parser.add_argument("--skip-build", action="store_true")
    parser.add_argument("--preview", action="store_true")
    options = parser.parse_args()
    publish(options.release, options.skip_build, options.preview)
