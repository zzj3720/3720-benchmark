#!/usr/bin/env python3
"""Re-run the isolated Parabox verifier after a proven event-header defect."""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import subprocess
import tempfile
from pathlib import Path

from tools.agents.parabox_resume import ParaboxResume


SCHEMA = "parabox-verifier-recovery-v1"


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def source_manifest(root: Path) -> list[dict[str, str]]:
    return [
        {"path": str(path.relative_to(root)), "sha256": sha256(path)}
        for path in sorted(root.rglob("*"))
        if path.is_file()
        and "__pycache__" not in path.parts
        and path.suffix != ".pyc"
    ]


class LocalResume(ParaboxResume):
    def __init__(self, state: Path, audit: Path, events: Path):
        self._configure_parabox_resume(
            resume_game_state_path=str(state),
            resume_game_audit_path=str(audit),
            resume_game_events_path=str(events),
        )


def run(
    result_path: Path,
    output_dir: Path,
    tests_dir: Path,
    image: str | None,
) -> dict[str, object]:
    result_path = result_path.resolve()
    output_dir = output_dir.resolve()
    tests_dir = tests_dir.resolve()
    if output_dir.exists():
        raise ValueError(f"recovery output already exists: {output_dir}")

    result = json.loads(result_path.read_text(encoding="utf-8"))
    artifacts = result_path.parent / "artifacts" / "var" / "lib" / "parabox"
    state = artifacts / "parabox-state.txt"
    audit = artifacts / "parabox-audit.tsv"
    events = artifacts / "parabox-events.jsonl"
    for path in (state, audit, events):
        if not path.is_file():
            raise ValueError(f"missing source artifact: {path}")

    normalized, provenance = LocalResume(
        state, audit, events
    )._normalized_resume_events()
    if provenance["mode"] != "legacy_startup_rebased":
        raise ValueError(
            "recovery is permitted only for the proven legacy startup-header defect"
        )

    output_dir.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(
        prefix=f".{output_dir.name}-", dir=output_dir.parent
    ) as temporary:
        work = Path(temporary)
        shutil.copy2(state, work / state.name)
        shutil.copy2(audit, work / audit.name)
        (work / events.name).write_bytes(normalized)
        manifest = source_manifest(tests_dir)
        (work / "verifier-source-manifest.json").write_text(
            json.dumps(manifest, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )

        if image is None:
            built = subprocess.run(
                [
                    "docker",
                    "build",
                    "--quiet",
                    "--file",
                    str(tests_dir / "Dockerfile"),
                    str(tests_dir),
                ],
                check=True,
                capture_output=True,
                text=True,
            )
            image = built.stdout.strip().splitlines()[-1]

        logs = work / "verifier"
        logs.mkdir()
        verified = subprocess.run(
            [
                "docker",
                "run",
                "--rm",
                "--mount",
                f"type=bind,src={work / state.name},"
                "dst=/var/lib/parabox/parabox-state.txt,readonly",
                "--mount",
                f"type=bind,src={work / audit.name},"
                "dst=/var/lib/parabox/parabox-audit.tsv,readonly",
                "--mount",
                f"type=bind,src={work / events.name},"
                "dst=/var/lib/parabox/parabox-events.jsonl,readonly",
                "--mount",
                f"type=bind,src={logs},dst=/logs/verifier",
                image,
                "/tests/test.sh",
            ],
            check=True,
            capture_output=True,
            text=True,
        )
        (work / "verifier-stdout.txt").write_text(
            verified.stdout, encoding="utf-8"
        )
        (work / "verifier-stderr.txt").write_text(
            verified.stderr, encoding="utf-8"
        )
        reward = int((logs / "reward.txt").read_text().strip())
        state_score = sum(
            line.startswith("solved ")
            for line in (work / state.name).read_text().splitlines()
        )
        if reward != state_score:
            raise ValueError(
                f"isolated verifier returned {reward}, state contains {state_score}"
            )

        original_reward = result["verifier_result"]["rewards"]["reward"]
        attestation = {
            "schema": SCHEMA,
            "status": "passed",
            "defect": "legacy_resume_startup_header_zeroed",
            "source": {
                "result_sha256": sha256(result_path),
                "task_checksum": result["task_checksum"],
                "original_verifier_reward": original_reward,
                "state_sha256": sha256(work / state.name),
                "audit_sha256": sha256(work / audit.name),
                "events_sha256": sha256(events),
            },
            "normalization": provenance,
            "verification": {
                "image": image,
                "source_manifest_sha256": sha256(
                    work / "verifier-source-manifest.json"
                ),
                "normalized_events_sha256": sha256(work / events.name),
                "reward": reward,
                "state_score": state_score,
                "stdout_sha256": sha256(work / "verifier-stdout.txt"),
                "stderr_sha256": sha256(work / "verifier-stderr.txt"),
                "ctrf_sha256": sha256(logs / "ctrf.json"),
            },
        }
        (work / "attestation.json").write_text(
            json.dumps(attestation, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        Path(temporary).rename(output_dir)
    return attestation


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("result", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument(
        "--tests-dir", type=Path, default=Path("tasks/parabox-intro/tests")
    )
    parser.add_argument("--image")
    args = parser.parse_args()
    attestation = run(args.result, args.output, args.tests_dir, args.image)
    print(json.dumps(attestation, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
