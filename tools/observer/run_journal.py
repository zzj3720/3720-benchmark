from __future__ import annotations

import asyncio
import json
import os
import time
from pathlib import Path
from typing import Any


RUN_EVENT_SCHEMA = "benchmark-run-event-v1"


def _milliseconds(value: Any) -> int:
    if hasattr(value, "timestamp"):
        return int(value.timestamp() * 1000)
    return int(time.time() * 1000)


class RunJournalPlugin:
    """Thin Harbor adapter for the authoritative Rust run recorder."""

    def __init__(
        self,
        *,
        chain_id: str | None = None,
        journal_root: str | None = None,
        recorder_path: str | None = None,
    ) -> None:
        self.requested_chain_id = chain_id
        self.requested_journal_root = (
            Path(journal_root).expanduser() if journal_root else None
        )
        configured_recorder = recorder_path or os.environ.get(
            "BENCHMARK_RUN_RECORDER"
        )
        self.recorder_path = (
            Path(configured_recorder).expanduser()
            if configured_recorder
            else Path(__file__).parent
            / "runtime"
            / "target"
            / "release"
            / "run-recorder"
        )
        self._job: Any = None
        self._process: asyncio.subprocess.Process | None = None
        self._protocol_lock = asyncio.Lock()
        self._segment_id: str | None = None

    async def on_job_start(self, job: Any) -> None:
        if len(job) != 1:
            raise ValueError(
                "RunJournalPlugin requires one logical trial per Harbor job"
            )
        self._job = job
        chain_id = self.requested_chain_id or job.config.job_name
        journal_root = (
            self.requested_journal_root
            or job.job_dir.parent.parent / "run-journals"
        )
        if not self.recorder_path.is_file():
            raise FileNotFoundError(
                f"Rust run recorder is not built: {self.recorder_path}"
            )
        self._process = await asyncio.create_subprocess_exec(
            str(self.recorder_path),
            chain_id,
            str(journal_root),
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
        )
        try:
            await self._send({"command": "ping"})
        except BaseException:
            await self._stop_process()
            raise

        job.on_trial_started(self._on_hook)
        job.on_environment_started(self._on_hook)
        job.on_agent_started(self._on_hook)
        job.on_agent_ended(self._on_hook)
        job.on_verification_started(self._on_hook)
        job.on_trial_cancelled(self._on_hook)
        job.on_trial_ended(self._on_hook)

    async def on_job_end(self, _job_result: Any) -> None:
        try:
            if self._process and self._process.returncode is None:
                await self._send({"command": "shutdown"})
        finally:
            await self._stop_process()

    async def _on_hook(self, event: Any) -> None:
        segment_id = str(event.trial_id)
        timestamp_ms = _milliseconds(event.timestamp)
        if self._segment_id is None:
            self._segment_id = segment_id
            trial_dir = self._job.job_dir / event.trial_name
            observer_dir = (
                trial_dir / "artifacts" / "logs" / "artifacts" / "observer"
            )
            await self._send(
                {
                    "command": "register",
                    "segment_id": segment_id,
                    "created_at_ms": timestamp_ms,
                    "observer_dir": str(observer_dir),
                    "job_id": str(self._job.id),
                    "job_name": self._job.config.job_name,
                    "trial_id": segment_id,
                    "trial_name": event.trial_name,
                    "task": event.task_name,
                    "model": event.config.agent.model_name,
                    "agent": event.config.agent.name,
                    "effort": event.config.agent.kwargs.get("reasoning_effort"),
                    "agent_dir": str(trial_dir / "agent"),
                    "workspace_dir": str(trial_dir / "agent" / "workspace"),
                }
            )
        elif self._segment_id != segment_id:
            raise RuntimeError("run recorder received an unexpected second segment")

        event_name = str(getattr(event.event, "value", event.event))
        if event_name == "start":
            return
        request: dict[str, Any] = {
            "command": "lifecycle",
            "segment_id": segment_id,
            "event": event_name,
            "timestamp_ms": timestamp_ms,
        }
        if event_name == "end":
            result = event.result
            exception = result.exception_info
            request["exception_type"] = (
                exception.exception_type if exception else None
            )
            verifier = result.verifier_result
            request["rewards"] = verifier.rewards if verifier else None
        await self._send(request)

    async def _send(self, request: dict[str, Any]) -> None:
        async with self._protocol_lock:
            process = self._process
            if process is None or process.stdin is None or process.stdout is None:
                raise RuntimeError("Rust run recorder is not running")
            if process.returncode is not None:
                raise RuntimeError(
                    f"Rust run recorder exited with status {process.returncode}"
                )
            process.stdin.write(
                (json.dumps(request, ensure_ascii=False, separators=(",", ":")) + "\n").encode()
            )
            await process.stdin.drain()
            response_line = await process.stdout.readline()
            if not response_line:
                await process.wait()
                raise RuntimeError(
                    f"Rust run recorder exited with status {process.returncode}"
                )
            response = json.loads(response_line)
            if not response.get("ok"):
                raise RuntimeError(response.get("error") or "Rust run recorder failed")

    async def _stop_process(self) -> None:
        process, self._process = self._process, None
        if process is None:
            return
        if process.stdin:
            process.stdin.close()
        if process.returncode is None:
            try:
                await asyncio.wait_for(process.wait(), timeout=5)
            except TimeoutError:
                process.terminate()
                await process.wait()
