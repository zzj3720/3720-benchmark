"""Shared restoration helpers for resumable Parabox agent trials."""

import base64
import hashlib
import json
import shlex
import time
from pathlib import Path

from harbor.environments.base import BaseEnvironment


class ParaboxResume:
    """Restore private sidecar state before a continued trial starts."""

    _EVENT_SCHEMA = "parabox-events-v1"
    _STATE_SCHEMA = "parabox-state-v4"
    _TOTAL_PUZZLES = 364
    _WORKSPACE_CHECKPOINT = "/logs/agent/workspace"

    def _configure_parabox_resume(
        self,
        *,
        resume_game_state_path: str,
        resume_game_audit_path: str | None = None,
        resume_game_events_path: str | None = None,
    ) -> None:
        self._resume_game_state_path = Path(resume_game_state_path)
        self._resume_game_audit_path = (
            Path(resume_game_audit_path) if resume_game_audit_path else None
        )
        self._resume_game_events_path = (
            Path(resume_game_events_path) if resume_game_events_path else None
        )
        for path in (
            self._resume_game_state_path,
            self._resume_game_audit_path,
            self._resume_game_events_path,
        ):
            if path is not None and not path.is_file():
                raise ValueError(f"Parabox resume artifact is missing: {path}")

    def _resume_state_summary(self) -> dict[str, object]:
        lines = self._resume_game_state_path.read_text(encoding="utf-8").splitlines()
        if not lines or lines[0] != self._STATE_SCHEMA:
            raise ValueError(
                f"unsupported Parabox state schema in "
                f"{self._resume_game_state_path}"
            )
        campaign = next(
            (
                line.removeprefix("campaign ")
                for line in lines
                if line.startswith("campaign ")
            ),
            None,
        )
        if campaign is None:
            raise ValueError(
                f"Parabox state has no campaign in {self._resume_game_state_path}"
            )
        selected_token = next(
            (
                line.removeprefix("selected ")
                for line in lines
                if line.startswith("selected ")
            ),
            None,
        )
        selected = None if selected_token in (None, "-") else selected_token
        return {
            "campaign": campaign,
            "score": sum(line.startswith("solved ") for line in lines),
            "selected": selected,
        }

    def _resume_event_baseline(self) -> bytes:
        summary = self._resume_state_summary()
        record = {
            **summary,
            "schema": self._EVENT_SCHEMA,
            "timestamp_ms": time.time_ns() // 1_000_000,
            "total": self._TOTAL_PUZZLES,
            "type": "sidecar_started",
        }
        return (
            json.dumps(record, separators=(",", ":"), sort_keys=True) + "\n"
        ).encode()

    def _normalized_resume_events(self) -> tuple[bytes, dict[str, object]]:
        if self._resume_game_events_path is None:
            normalized = self._resume_event_baseline()
            return normalized, {
                "mode": "synthesized_baseline",
                "source_sha256": None,
                "normalized_sha256": hashlib.sha256(normalized).hexdigest(),
            }

        source = self._resume_game_events_path.read_bytes()
        try:
            records = [
                json.loads(line)
                for line in source.decode("utf-8").splitlines()
                if line
            ]
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise ValueError(
                f"invalid Parabox resume event stream: {error}"
            ) from error
        if not records or records[0].get("type") != "sidecar_started":
            raise ValueError("Parabox resume event stream has no startup record")
        if any(record.get("schema") != self._EVENT_SCHEMA for record in records):
            raise ValueError("Parabox resume event stream has an unknown schema")

        transformed = False
        first_request = next(
            (record for record in records[1:] if record.get("type") == "request"),
            None,
        )
        if (
            first_request is not None
            and (
                records[0].get("score") != first_request.get("score_before")
                or records[0].get("selected")
                != first_request.get("selected_before")
            )
        ):
            # Early continuation adapters restored State after the sidecar had
            # already emitted its fresh zero-score startup record. The first
            # request contains the exact authoritative pre-request baseline,
            # so rebase only that header and retain every request unchanged.
            records[0]["score"] = first_request.get("score_before")
            records[0]["selected"] = first_request.get("selected_before")
            transformed = True

        previous_score = records[0].get("score")
        previous_selected = records[0].get("selected")
        previous_timestamp = records[0].get("timestamp_ms")
        for line_number, record in enumerate(records[1:], 2):
            timestamp = record.get("timestamp_ms")
            score = record.get("score")
            if (
                not isinstance(timestamp, int)
                or not isinstance(score, int)
                or not isinstance(previous_timestamp, int)
                or not isinstance(previous_score, int)
                or timestamp < previous_timestamp
                or score < previous_score
            ):
                raise ValueError(
                    f"invalid Parabox resume event ordering at line {line_number}"
                )
            if record.get("type") == "request" and (
                record.get("score_before") != previous_score
                or record.get("selected_before") != previous_selected
                or record.get("score_delta") != score - previous_score
            ):
                raise ValueError(
                    f"discontinuous Parabox resume event at line {line_number}"
                )
            previous_timestamp = timestamp
            previous_score = score
            previous_selected = record.get("selected")

        summary = self._resume_state_summary()
        if (
            previous_score != summary["score"]
            or previous_selected != summary["selected"]
        ):
            raise ValueError(
                "Parabox resume event stream does not match the restored state"
            )
        normalized = (
            "\n".join(
                json.dumps(record, separators=(",", ":"), sort_keys=True)
                for record in records
            )
            + "\n"
        ).encode()
        return normalized, {
            "mode": (
                "legacy_startup_rebased" if transformed else "exact_event_restore"
            ),
            "source_sha256": hashlib.sha256(source).hexdigest(),
            "normalized_sha256": hashlib.sha256(normalized).hexdigest(),
        }

    async def _restore_game_bytes(
        self,
        environment: BaseEnvironment,
        content: bytes,
        target: str,
    ) -> None:
        quoted_target = shlex.quote(target)
        initialized = await environment.service_exec(
            f"umask 077; : > {quoted_target}",
            service="game",
            user=0,
        )
        if initialized.return_code != 0:
            raise RuntimeError(
                f"failed to restore Parabox sidecar artifact to {target}: "
                f"{initialized.stderr}"
            )
        # Docker Compose passes exec commands through an argv boundary that is
        # smaller than long continuation event streams. Append bounded chunks
        # so restored traces remain exact without relying on host mounts.
        for offset in range(0, len(content), 32 * 1024):
            encoded = base64.b64encode(content[offset : offset + 32 * 1024]).decode(
                "ascii"
            )
            appended = await environment.service_exec(
                (
                    f"printf %s {shlex.quote(encoded)} | "
                    f"base64 -d >> {quoted_target}"
                ),
                service="game",
                user=0,
            )
            if appended.return_code != 0:
                raise RuntimeError(
                    f"failed to restore Parabox sidecar artifact to {target}: "
                    f"{appended.stderr}"
                )
        checked = await environment.service_exec(
            f'test "$(wc -c < {quoted_target})" -eq {len(content)}',
            service="game",
            user=0,
        )
        if checked.return_code != 0:
            raise RuntimeError(
                f"restored Parabox sidecar artifact has the wrong size: {target}"
            )

    async def _restore_game_file(
        self,
        environment: BaseEnvironment,
        source: Path,
        target: str,
    ) -> None:
        await self._restore_game_bytes(environment, source.read_bytes(), target)

    async def _restore_parabox(self, environment: BaseEnvironment) -> None:
        await self._restore_game_file(
            environment,
            self._resume_game_state_path,
            "/var/lib/parabox/parabox-state.txt",
        )
        if self._resume_game_audit_path is not None:
            await self._restore_game_file(
                environment,
                self._resume_game_audit_path,
                "/var/lib/parabox/parabox-audit.tsv",
            )
        events, provenance = self._normalized_resume_events()
        await self._restore_game_bytes(
            environment,
            events,
            "/var/lib/parabox/parabox-events.jsonl",
        )
        logs_dir = getattr(self, "logs_dir", None)
        if isinstance(logs_dir, Path):
            logs_dir.mkdir(parents=True, exist_ok=True)
            (logs_dir / "parabox-resume-provenance.json").write_text(
                json.dumps(provenance, indent=2, sort_keys=True) + "\n",
                encoding="utf-8",
            )
        destination = shlex.quote(self._WORKSPACE_CHECKPOINT)
        temporary = shlex.quote(f"{self._WORKSPACE_CHECKPOINT}.tmp")
        loop = (
            "while :; do "
            f"rm -rf {temporary}; mkdir -p {temporary}; "
            f"cp -R /app/. {temporary}/ 2>/dev/null || true; "
            f"rm -rf {destination}; mv {temporary} {destination}; "
            "sleep 30; done"
        )
        checkpoint = await environment.service_exec(
            (
                f"mkdir -p {shlex.quote(str(Path(self._WORKSPACE_CHECKPOINT).parent))}; "
                f"nohup sh -c {shlex.quote(loop)} "
                "</dev/null >/dev/null 2>&1 &"
            ),
            service="main",
            user=0,
        )
        if checkpoint.return_code != 0:
            raise RuntimeError(
                f"failed to start Parabox workspace checkpoint: "
                f"{checkpoint.stderr}"
            )
