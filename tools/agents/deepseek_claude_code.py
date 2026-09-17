"""Claude Code adapters backed by DeepSeek's Anthropic-compatible API."""

import asyncio
import os

from harbor.agents.installed.base import NetworkConnectionError
from harbor.agents.installed.claude_code import ClaudeCode
from harbor.environments.base import BaseEnvironment
from harbor.models.agent.context import AgentContext

from tools.agents.claude_resume import ClaudeSessionResume
from tools.agents.claude_goal import NativeClaudeGoal
from tools.agents.parabox_resume import ParaboxResume


class DeepSeekClaudeCode(ClaudeCode):
    """Use the local ``DEEPSEEK_API_KEY`` without serializing it into a job."""

    _BASE_URL = "https://api.deepseek.com/anthropic"
    _CONTEXT_TOKENS = "1048576"
    # Xhigh adaptive thinking can reserve 131,072 completion tokens. Compact
    # early enough to leave that reservation plus a 65,536-token safety margin.
    _AUTO_COMPACT_TOKENS = "851968"

    def __init__(self, *args, max_output_tokens: int | None = None, **kwargs):
        super().__init__(*args, **kwargs)
        if max_output_tokens is not None and max_output_tokens <= 0:
            raise ValueError("max_output_tokens must be positive")
        self._max_output_tokens = max_output_tokens
        self._resolved_env_vars.update(
            {
                "CLAUDE_CODE_AUTO_COMPACT_WINDOW": self._AUTO_COMPACT_TOKENS,
                "CLAUDE_CODE_MAX_CONTEXT_TOKENS": self._CONTEXT_TOKENS,
            }
        )

    async def install(self, environment: BaseEnvironment) -> None:
        for attempt in range(3):
            try:
                await super().install(environment)
                return
            except NetworkConnectionError:
                if attempt == 2:
                    raise
                await asyncio.sleep(2 ** attempt)

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
        original_max_output_tokens = os.environ.get(
            "CLAUDE_CODE_MAX_OUTPUT_TOKENS"
        )
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
        if self._max_output_tokens is not None:
            self._extra_env["CLAUDE_CODE_MAX_OUTPUT_TOKENS"] = str(
                self._max_output_tokens
            )
        # Harbor's Claude Code adapter consults the host process for this one
        # routing value and the output limit before it executes in the trial
        # container.
        os.environ["ANTHROPIC_BASE_URL"] = self._BASE_URL
        if self._max_output_tokens is not None:
            os.environ["CLAUDE_CODE_MAX_OUTPUT_TOKENS"] = str(
                self._max_output_tokens
            )
        try:
            await super().run(instruction, environment, context)
        finally:
            self._extra_env = original_extra_env
            if original_base_url is None:
                os.environ.pop("ANTHROPIC_BASE_URL", None)
            else:
                os.environ["ANTHROPIC_BASE_URL"] = original_base_url
            if original_max_output_tokens is None:
                os.environ.pop("CLAUDE_CODE_MAX_OUTPUT_TOKENS", None)
            else:
                os.environ["CLAUDE_CODE_MAX_OUTPUT_TOKENS"] = (
                    original_max_output_tokens
                )


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
        resume_from_last_compact: bool = False,
        resume_game_state_path: str,
        resume_game_audit_path: str | None = None,
        resume_game_events_path: str | None = None,
        compact_before_resume: bool = False,
        **kwargs,
    ):
        super().__init__(*args, **kwargs)
        self._compact_before_resume = compact_before_resume
        self._configure_claude_resume(
            resume_sessions_dir=resume_sessions_dir,
            resume_workspace_dir=resume_workspace_dir,
            resume_from_last_compact=resume_from_last_compact,
        )
        self._configure_parabox_resume(
            resume_game_state_path=resume_game_state_path,
            resume_game_audit_path=resume_game_audit_path,
            resume_game_events_path=resume_game_events_path,
        )

    async def setup(self, environment: BaseEnvironment) -> None:
        await super().setup(environment)
        await self._restore_parabox(environment)

    async def run(
        self,
        instruction: str,
        environment: BaseEnvironment,
        context: AgentContext,
    ) -> None:
        if self._compact_before_resume:
            self._resume = True
            original_effort = self._resolved_flags.get("reasoning_effort")
            original_disable_adaptive = self._resolved_env_vars.get(
                "CLAUDE_CODE_DISABLE_ADAPTIVE_THINKING"
            )
            original_thinking_tokens = self._resolved_env_vars.get(
                "MAX_THINKING_TOKENS"
            )
            self._resolved_flags["reasoning_effort"] = "low"
            self._resolved_env_vars["CLAUDE_CODE_DISABLE_ADAPTIVE_THINKING"] = "1"
            self._resolved_env_vars["MAX_THINKING_TOKENS"] = "8192"
            try:
                await DeepSeekClaudeCode.run(
                    self,
                    "/compact",
                    environment,
                    context,
                )
            finally:
                if original_effort is None:
                    self._resolved_flags.pop("reasoning_effort", None)
                else:
                    self._resolved_flags["reasoning_effort"] = original_effort
                if original_disable_adaptive is None:
                    self._resolved_env_vars.pop(
                        "CLAUDE_CODE_DISABLE_ADAPTIVE_THINKING", None
                    )
                else:
                    self._resolved_env_vars[
                        "CLAUDE_CODE_DISABLE_ADAPTIVE_THINKING"
                    ] = original_disable_adaptive
                if original_thinking_tokens is None:
                    self._resolved_env_vars.pop("MAX_THINKING_TOKENS", None)
                else:
                    self._resolved_env_vars["MAX_THINKING_TOKENS"] = (
                        original_thinking_tokens
                    )
                self._resume = False
        await super().run(instruction, environment, context)
