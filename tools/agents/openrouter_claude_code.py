"""Claude Code adapters backed by OpenRouter's Anthropic-compatible API."""

import json
import os
import shlex

from harbor.agents.installed.claude_code import ClaudeCode
from harbor.environments.base import BaseEnvironment
from harbor.models.agent.context import AgentContext

from tools.agents.claude_goal import NativeClaudeGoal
from tools.agents.claude_resume import ClaudeSessionResume
from tools.agents.parabox_resume import ParaboxResume


class OpenRouterClaudeCode(ClaudeCode):
    """Run a selected OpenRouter model through Claude Code."""

    _BASE_URL = "https://openrouter.ai/api"
    _CONTEXT_TOKENS = "1048576"
    _MAX_OUTPUT_TOKENS = "65536"

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
        api_key = self._get_env("OPENROUTER_API_KEY")
        if not api_key:
            raise RuntimeError(
                "OPENROUTER_API_KEY is required for OpenRouterClaudeCode"
            )

        self.logs_dir.mkdir(parents=True, exist_ok=True)
        (self.logs_dir / "credential-provenance.json").write_text(
            json.dumps(
                {
                    "schema": "local-credential-provenance-v1",
                    "environment_variable": "OPENROUTER_API_KEY",
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
        model = self.model_name or "openrouter/auto"
        self._extra_env = {
            **original_extra_env,
            "ANTHROPIC_API_KEY": "",
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
        os.environ["ANTHROPIC_BASE_URL"] = self._BASE_URL
        try:
            await super().run(instruction, environment, context)
        finally:
            self._extra_env = original_extra_env
            if original_base_url is None:
                os.environ.pop("ANTHROPIC_BASE_URL", None)
            else:
                os.environ["ANTHROPIC_BASE_URL"] = original_base_url


class GoalOpenRouterClaudeCode(NativeClaudeGoal, OpenRouterClaudeCode):
    """Run an OpenRouter model through Claude Code's persistent goal."""

    async def setup(self, environment: BaseEnvironment) -> None:
        await super().setup(environment)
        destination = "/logs/agent/workspace"
        temporary = f"{destination}.tmp"
        loop = (
            "while :; do "
            f"rm -rf {shlex.quote(temporary)}; mkdir -p {shlex.quote(temporary)}; "
            f"cp -R /app/. {shlex.quote(temporary)}/ 2>/dev/null || true; "
            f"rm -rf {shlex.quote(destination)}; "
            f"mv {shlex.quote(temporary)} {shlex.quote(destination)}; "
            "sleep 30; done"
        )
        checkpoint = await environment.service_exec(
            (
                "mkdir -p /logs/agent; "
                f"nohup sh -c {shlex.quote(loop)} "
                "</dev/null >/dev/null 2>&1 &"
            ),
            service="main",
            user=0,
        )
        if checkpoint.return_code != 0:
            raise RuntimeError(
                f"failed to start OpenRouter workspace checkpoint: {checkpoint.stderr}"
            )


class ResumeParaboxGoalOpenRouterClaudeCode(
    ParaboxResume,
    NativeClaudeGoal,
    ClaudeSessionResume,
    OpenRouterClaudeCode,
):
    """Resume an OpenRouter Claude session and private Parabox state."""

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
