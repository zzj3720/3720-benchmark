"""Claude Code adapters backed by DeepSeek's Anthropic-compatible API."""

import os

from harbor.agents.installed.claude_code import ClaudeCode
from harbor.environments.base import BaseEnvironment
from harbor.models.agent.context import AgentContext

from tools.agents.claude_resume import ClaudeSessionResume
from tools.agents.claude_goal import NativeClaudeGoal
from tools.agents.parabox_resume import ParaboxResume


class DeepSeekClaudeCode(ClaudeCode):
    """Use the local ``DEEPSEEK_API_KEY`` without serializing it into a job."""

    _BASE_URL = "https://api.deepseek.com/anthropic"

    async def run(
        self,
        instruction: str,
        environment: BaseEnvironment,
        context: AgentContext,
    ) -> None:
        api_key = self._get_env("DEEPSEEK_API_KEY")
        if not api_key:
            raise RuntimeError("DEEPSEEK_API_KEY is required for DeepSeekClaudeCode")

        original_extra_env = self._extra_env
        original_base_url = os.environ.get("ANTHROPIC_BASE_URL")
        model = self.model_name or "deepseek-v4-flash"
        self._extra_env = {
            **original_extra_env,
            "ANTHROPIC_API_KEY": api_key,
            "ANTHROPIC_AUTH_TOKEN": api_key,
            "ANTHROPIC_BASE_URL": self._BASE_URL,
            "ANTHROPIC_DEFAULT_OPUS_MODEL": model,
            "ANTHROPIC_DEFAULT_SONNET_MODEL": model,
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": model,
            "CLAUDE_CODE_SUBAGENT_MODEL": model,
        }
        # Harbor's Claude Code adapter consults the host process for this one
        # routing value before it executes in the trial container.
        os.environ["ANTHROPIC_BASE_URL"] = self._BASE_URL
        try:
            await super().run(instruction, environment, context)
        finally:
            self._extra_env = original_extra_env
            if original_base_url is None:
                os.environ.pop("ANTHROPIC_BASE_URL", None)
            else:
                os.environ["ANTHROPIC_BASE_URL"] = original_base_url


class GoalDeepSeekClaudeCode(NativeClaudeGoal, DeepSeekClaudeCode):
    """Run DeepSeek through Claude Code's native persistent `/goal`."""


class ResumeParaboxGoalDeepSeekClaudeCode(
    ParaboxResume,
    NativeClaudeGoal,
    ClaudeSessionResume,
    DeepSeekClaudeCode,
):
    """Resume DeepSeek's Claude session and private Parabox state."""

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
