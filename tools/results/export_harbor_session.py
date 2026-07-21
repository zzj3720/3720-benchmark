#!/usr/bin/env python3
"""Export a reviewable Harbor session without local credentials or install logs."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
from pathlib import Path
from typing import Any

from audit_parabox_session import audit
from audit_parabox_trajectory import audit as audit_trajectory


SENSITIVE_KEYS = {
    "access_token",
    "api_key",
    "authorization",
    "id_token",
    "password",
    "refresh_token",
    "secret",
}
SECRET_PATTERNS = [
    (re.compile(r"sk-[A-Za-z0-9_-]{16,}"), "<redacted-api-key>"),
    (
        re.compile(r"(?i)(authorization\s*:\s*bearer\s+)[^\s\"']+"),
        r"\1<redacted-token>",
    ),
]


def sanitize_text(value: str) -> str:
    value = value.replace(str(Path.home()), "$HOME")
    for pattern, replacement in SECRET_PATTERNS:
        value = pattern.sub(replacement, value)
    return value


def sanitize_json(value: Any, key: str | None = None) -> Any:
    if key is not None and key.lower() in SENSITIVE_KEYS:
        return "<redacted>"
    if isinstance(value, dict):
        return {name: sanitize_json(item, name) for name, item in value.items()}
    if isinstance(value, list):
        return [sanitize_json(item) for item in value]
    if isinstance(value, str):
        return sanitize_text(value)
    return value


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write_sanitized(source: Path, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    if source.suffix == ".json":
        value = sanitize_json(json.loads(source.read_text()))
        destination.write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n")
    else:
        destination.write_text(sanitize_text(source.read_text()))


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("job", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument(
        "--trial",
        help="trial directory name when the job contains more than one trial",
    )
    parser.add_argument(
        "--oracle",
        type=Path,
        help="optional canonical trace file for exact-history diagnostics",
    )
    args = parser.parse_args()

    if args.trial:
        trial_dirs = [args.job / args.trial]
    else:
        trial_dirs = sorted(
            path
            for path in args.job.iterdir()
            if path.is_dir() and (path / "result.json").is_file()
        )
    if len(trial_dirs) != 1:
        raise ValueError(
            f"expected one trial under {args.job}, found {len(trial_dirs)}"
        )
    trial = trial_dirs[0]
    if not (trial / "result.json").is_file():
        raise ValueError(f"trial has no result: {trial}")

    state_candidates = [
        trial / "artifacts" / "var" / "lib" / "parabox" / "parabox-state.txt",
        trial / "artifacts" / "app" / "parabox-state.txt",
    ]
    state_artifact = next((path for path in state_candidates if path.is_file()), None)
    audit_artifact = (
        trial / "artifacts" / "var" / "lib" / "parabox" / "parabox-audit.tsv"
    )
    event_artifact = (
        trial / "artifacts" / "var" / "lib" / "parabox" / "parabox-events.jsonl"
    )

    sources = {
        "config.json": args.job / "config.json",
        "job-result.json": args.job / "result.json",
        "trial-config.json": trial / "config.json",
        "trial-result.json": trial / "result.json",
        "trajectory.json": trial / "agent" / "trajectory.json",
        "artifact-manifest.json": trial / "artifacts" / "manifest.json",
        "verifier/ctrf.json": trial / "verifier" / "ctrf.json",
        "verifier/reward.txt": trial / "verifier" / "reward.txt",
        "exception.txt": trial / "exception.txt",
    }
    if state_artifact is not None:
        sources["artifacts/parabox-state.txt"] = state_artifact
    if audit_artifact.is_file():
        sources["artifacts/parabox-audit.tsv"] = audit_artifact
    if event_artifact.is_file():
        sources["artifacts/parabox-events.jsonl"] = event_artifact

    if args.output.exists():
        shutil.rmtree(args.output)
    args.output.mkdir(parents=True)

    exported = []
    for relative, source in sources.items():
        if not source.is_file():
            continue
        destination = args.output / relative
        write_sanitized(source, destination)
        exported.append(
            {
                "path": relative,
                "source_sha256": digest(source),
                "export_sha256": digest(destination),
            }
        )

    native_sessions = [
        trial / "agent" / name
        for name in ("codex.txt", "claude-code.txt", "qoder-cn.jsonl")
        if (trial / "agent" / name).is_file()
    ]
    if len(native_sessions) > 1:
        raise ValueError(
            f"expected at most one native agent session, found {len(native_sessions)}"
        )
    if native_sessions:
        source = native_sessions[0]
        destination = args.output / "session-audit.json"
        destination.write_text(
            json.dumps(
                sanitize_json(audit(source)),
                indent=2,
                ensure_ascii=False,
            )
            + "\n"
        )
        exported.append(
            {
                "path": "session-audit.json",
                "source_sha256": digest(source),
                "export_sha256": digest(destination),
            }
        )

    trajectory = trial / "agent" / "trajectory.json"
    if trajectory.is_file():
        destination = args.output / "audit-report.json"
        destination.write_text(
            json.dumps(
                sanitize_json([audit_trajectory(trajectory, args.oracle)]),
                indent=2,
                ensure_ascii=False,
            )
            + "\n"
        )
        exported.append(
            {
                "path": "audit-report.json",
                "source_sha256": digest(trajectory),
                "export_sha256": digest(destination),
            }
        )

    manifest = {
        "schema_version": "2",
        "source_job": args.job.name,
        "source_trial": trial.name,
        "excluded": [
            "job.log",
            "trial.log",
            "raw native agent logs (represented by session-audit.json)",
            "lock files",
            "credentials",
        ],
        "files": exported,
    }
    (args.output / "session-manifest.json").write_text(
        json.dumps(manifest, indent=2) + "\n"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
