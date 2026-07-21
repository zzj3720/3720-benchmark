"""Claude Code adapters backed by the Kimi Coding Plan API."""

import json
import os
from typing import override

from harbor.agents.installed.claude_code import ClaudeCode
from harbor.environments.base import BaseEnvironment
from harbor.models.agent.context import AgentContext

from tools.agents.claude_resume import ClaudeSessionResume
from tools.agents.claude_goal import NativeClaudeGoal
from tools.agents.parabox_resume import ParaboxResume


class KimiClaudeCode(ClaudeCode):
    """Run Kimi K3 through the official Anthropic-compatible endpoint."""

    _BASE_URL = "https://api.kimi.com/coding/"
    _CONTEXT_TOKENS = "1048576"
    _MAX_OUTPUT_TOKENS = "131072"

    @override
    async def install(self, environment: BaseEnvironment) -> None:
        if await self._installed_claude_satisfies_version(environment):
            return

        await self.exec_as_root(
            environment,
            command=(
                "apt-get update && apt-get install -y --no-install-recommends "
                "ca-certificates nodejs npm procps"
            ),
            env={"DEBIAN_FRONTEND": "noninteractive"},
        )
        version = f"@{self._version}" if self._version else ""
        await self.exec_as_agent(
            environment,
            command=(
                "set -euo pipefail; "
                "npm install -g --prefix \"$HOME/.local\" "
                f"@anthropic-ai/claude-code{version}; "
                "export PATH=\"$HOME/.local/bin:$PATH\"; "
                "claude --version"
            ),
        )

    def __init__(
        self,
        *args,
        credential_label: str = "unlabeled-local-key",
        **kwargs,
    ):
        super().__init__(*args, **kwargs)
        self._credential_label = credential_label.strip()
        if not self._credential_label:
            raise ValueError("credential_label must not be empty")
        if len(self._credential_label) > 80:
            raise ValueError("credential_label must be at most 80 characters")

    async def run(
        self,
        instruction: str,
        environment: BaseEnvironment,
        context: AgentContext,
    ) -> None:
        api_key = self._get_env("KIMI_API_KEY")
        if not api_key:
            raise RuntimeError("KIMI_API_KEY is required for KimiClaudeCode")

        self.logs_dir.mkdir(parents=True, exist_ok=True)
        (self.logs_dir / "credential-provenance.json").write_text(
            json.dumps(
                {
                    "schema": "local-credential-provenance-v1",
                    "environment_variable": "KIMI_API_KEY",
                    "credential_label": self._credential_label,
                    "endpoint": self._BASE_URL,
                    "secret_value_recorded": False,
                },
                indent=2,
            )
            + "\n",
            encoding="utf-8",
        )

        original_extra_env = self._extra_env
        original_base_url = os.environ.get("ANTHROPIC_BASE_URL")
        model = self.model_name or "k3[1m]"
        self._extra_env = {
            **original_extra_env,
            "ANTHROPIC_API_KEY": api_key,
            "ANTHROPIC_AUTH_TOKEN": api_key,
            "ANTHROPIC_BASE_URL": self._BASE_URL,
            "ANTHROPIC_MODEL": model,
            "ANTHROPIC_DEFAULT_FABLE_MODEL": model,
            "ANTHROPIC_DEFAULT_OPUS_MODEL": model,
            "ANTHROPIC_DEFAULT_SONNET_MODEL": model,
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": model,
            "CLAUDE_CODE_SUBAGENT_MODEL": model,
            "CLAUDE_CODE_AUTO_COMPACT_WINDOW": self._CONTEXT_TOKENS,
            "CLAUDE_CODE_MAX_CONTEXT_TOKENS": self._CONTEXT_TOKENS,
            "CLAUDE_CODE_MAX_OUTPUT_TOKENS": self._MAX_OUTPUT_TOKENS,
        }
        # Harbor reads this routing value from the host before launching Claude.
        os.environ["ANTHROPIC_BASE_URL"] = self._BASE_URL
        try:
            await super().run(instruction, environment, context)
        finally:
            self._extra_env = original_extra_env
            if original_base_url is None:
                os.environ.pop("ANTHROPIC_BASE_URL", None)
            else:
                os.environ["ANTHROPIC_BASE_URL"] = original_base_url


class GoalKimiClaudeCode(NativeClaudeGoal, KimiClaudeCode):
    """Run Kimi through Claude Code's native persistent `/goal`."""


class ResumeParaboxGoalKimiClaudeCode(
    ParaboxResume,
    NativeClaudeGoal,
    ClaudeSessionResume,
    KimiClaudeCode,
):
    """Resume Kimi's native Claude session and private Parabox state."""

    def __init__(
        self,
        *args,
        resume_sessions_dir: str,
        resume_workspace_dir: str | None = None,
        resume_game_state_path: str,
        resume_game_audit_path: str | None = None,
        resume_game_events_path: str | None = None,
        **kwargs,
    ):
        super().__init__(*args, **kwargs)
        self._configure_claude_resume(
            resume_sessions_dir=resume_sessions_dir,
            resume_workspace_dir=resume_workspace_dir,
        )
        self._configure_parabox_resume(
            resume_game_state_path=resume_game_state_path,
            resume_game_audit_path=resume_game_audit_path,
            resume_game_events_path=resume_game_events_path,
        )

    async def setup(self, environment: BaseEnvironment) -> None:
        await super().setup(environment)
        await self._restore_parabox(environment)
