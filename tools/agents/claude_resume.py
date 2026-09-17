"""Shared restoration for resumable Claude Code-backed agents."""

import json
import shutil
import tempfile
from pathlib import Path

from harbor.environments.base import BaseEnvironment
from harbor.models.agent.context import AgentContext
from harbor.models.trial.paths import EnvironmentPaths


class ClaudeSessionResume:
    """Restore the native session and optional `/app` working files."""

    def _configure_claude_resume(
        self,
        *,
        resume_sessions_dir: str,
        resume_workspace_dir: str | None = None,
        resume_from_last_compact: bool = False,
    ) -> None:
        self._resume_sessions_dir = Path(resume_sessions_dir)
        self._resume_workspace_dir = (
            Path(resume_workspace_dir) if resume_workspace_dir else None
        )
        self._resume_from_last_compact = resume_from_last_compact
        if not self._resume_sessions_dir.is_dir():
            raise ValueError(
                f"resume_sessions_dir is not a directory: {self._resume_sessions_dir}"
            )
        if (
            self._resume_workspace_dir is not None
            and not self._resume_workspace_dir.is_dir()
        ):
            raise ValueError(
                f"resume_workspace_dir is not a directory: "
                f"{self._resume_workspace_dir}"
            )

    @staticmethod
    def _trim_after_last_compact(session: Path) -> None:
        lines = session.read_text(encoding="utf-8").splitlines()
        boundary_uuid = None
        boundary_index = None
        for index, line in enumerate(lines):
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            if event.get("type") == "system" and event.get("subtype") == (
                "compact_boundary"
            ):
                boundary_uuid = event.get("uuid")
                boundary_index = index
        if not isinstance(boundary_uuid, str) or boundary_index is None:
            raise ValueError(f"Claude session has no compact boundary: {session}")
        summary_index = next(
            (
                index
                for index in range(boundary_index + 1, len(lines))
                if json.loads(lines[index]).get("parentUuid") == boundary_uuid
            ),
            None,
        )
        if summary_index is None:
            raise ValueError(f"Claude compact boundary has no summary: {session}")
        session.write_text(
            "\n".join(lines[: summary_index + 1]) + "\n",
            encoding="utf-8",
        )

    async def setup(self, environment: BaseEnvironment) -> None:
        await super().setup(environment)
        sessions_dir = EnvironmentPaths.agent_dir / "sessions"
        await self.exec_as_root(
            environment,
            command=f"mkdir -p {sessions_dir} && chmod -R 777 /logs/agent",
        )
        upload_source = self._resume_sessions_dir
        temporary_root = None
        if self._resume_from_last_compact:
            temporary_root = Path(tempfile.mkdtemp(prefix="claude-resume-"))
            upload_source = temporary_root / "sessions"
            shutil.copytree(self._resume_sessions_dir, upload_source)
            for session in upload_source.rglob("*.jsonl"):
                self._trim_after_last_compact(session)
        try:
            await environment.upload_dir(upload_source, sessions_dir.as_posix())
        finally:
            if temporary_root is not None:
                shutil.rmtree(temporary_root)
        if self._resume_workspace_dir is not None:
            await environment.upload_dir(self._resume_workspace_dir, "/app")

    async def run(
        self,
        instruction: str,
        environment: BaseEnvironment,
        context: AgentContext,
    ) -> None:
        self._resume = True
        try:
            await super().run(instruction, environment, context)
        finally:
            self._resume = False
