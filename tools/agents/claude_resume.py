"""Shared restoration for resumable Claude Code-backed agents."""

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
    ) -> None:
        self._resume_sessions_dir = Path(resume_sessions_dir)
        self._resume_workspace_dir = (
            Path(resume_workspace_dir) if resume_workspace_dir else None
        )
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

    async def setup(self, environment: BaseEnvironment) -> None:
        await super().setup(environment)
        sessions_dir = EnvironmentPaths.agent_dir / "sessions"
        await self.exec_as_root(
            environment,
            command=f"mkdir -p {sessions_dir} && chmod -R 777 /logs/agent",
        )
        await environment.upload_dir(
            self._resume_sessions_dir,
            sessions_dir.as_posix(),
        )
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
