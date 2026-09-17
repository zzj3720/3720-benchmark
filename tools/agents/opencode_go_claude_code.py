"""Claude Code backed by DeepSeek V4 Flash from an OpenCode Go plan."""

import json
import os
import shlex
from pathlib import Path

from harbor.agents.installed.claude_code import ClaudeCode
from harbor.environments.base import BaseEnvironment
from harbor.models.agent.context import AgentContext

from tools.agents.claude_goal import NativeClaudeGoal
from tools.agents.claude_resume import ClaudeSessionResume
from tools.agents.deepseek_claude_code import DeepSeekClaudeCode
from tools.agents.parabox_resume import ParaboxResume


class OpenCodeGoClaudeCode(DeepSeekClaudeCode):
    """Run native Claude Code through CLIProxyAPI and OpenCode Go."""

    _BASE_URL = "http://127.0.0.1:3456"
    _LOCAL_AUTH_TOKEN = "harbor-local-claude-code"
    _CONFIG_PLACEHOLDER = "__OPENCODE_GO_API_KEY__"
    _REMOTE_PROXY = "/tmp/cliproxyapi"
    _REMOTE_CONFIG_TEMPLATE = "/tmp/cliproxyapi-template.yaml"
    _REMOTE_CONFIG = "/tmp/cliproxyapi.yaml"
    _REMOTE_AUTH = "/tmp/opencode-auth.json"

    def __init__(
        self,
        *args,
        proxy_binary_path: str,
        proxy_config_path: str,
        opencode_auth_path: str,
        **kwargs,
    ):
        super().__init__(*args, **kwargs)
        self._proxy_binary_path = Path(proxy_binary_path)
        self._proxy_config_path = Path(proxy_config_path)
        self._opencode_auth_path = Path(opencode_auth_path)
        for label, path in (
            ("proxy_binary_path", self._proxy_binary_path),
            ("proxy_config_path", self._proxy_config_path),
            ("opencode_auth_path", self._opencode_auth_path),
        ):
            if not path.is_file():
                raise ValueError(f"{label} is not a file: {path}")
        auth = json.loads(self._opencode_auth_path.read_text(encoding="utf-8"))
        key = auth.get("opencode-go", {}).get("key")
        if not isinstance(key, str) or not key:
            raise ValueError("opencode_auth_path has no OpenCode Go API key")

    async def setup(self, environment: BaseEnvironment) -> None:
        await super().setup(environment)
        await environment.upload_file(
            self._proxy_binary_path, self._REMOTE_PROXY
        )
        await environment.upload_file(
            self._proxy_config_path, self._REMOTE_CONFIG_TEMPLATE
        )
        await environment.upload_file(self._opencode_auth_path, self._REMOTE_AUTH)
        await self.exec_as_root(
            environment,
            command=(
                f"chmod 700 {self._REMOTE_PROXY} && "
                f"chmod 600 {self._REMOTE_CONFIG_TEMPLATE} {self._REMOTE_AUTH}"
            ),
        )

    async def _start_proxy(self, environment: BaseEnvironment) -> None:
        render_config = shlex.quote(
            "import json,os,sys;"
            "auth=json.load(open(sys.argv[1],encoding='utf-8'));"
            "key=auth['opencode-go']['key'];"
            "assert isinstance(key,str) and key;"
            "template=open(sys.argv[2],encoding='utf-8').read();"
            f"assert template.count('{self._CONFIG_PLACEHOLDER}')==1;"
            "fd=os.open(sys.argv[3],os.O_WRONLY|os.O_CREAT|os.O_TRUNC,0o600);"
            "os.write(fd,template.replace("
            f"'{self._CONFIG_PLACEHOLDER}',key).encode());"
            "os.close(fd)"
        )
        await self.exec_as_agent(
            environment,
            command=(
                "set -euo pipefail; umask 077; "
                "if [ -s /logs/agent/cliproxyapi.pid ] && "
                "kill -0 \"$(cat /logs/agent/cliproxyapi.pid)\" 2>/dev/null; "
                "then exit 0; fi; "
                f"python3 -c {render_config} {self._REMOTE_AUTH} "
                f"{self._REMOTE_CONFIG_TEMPLATE} {self._REMOTE_CONFIG}; "
                "mkdir -p /tmp/cliproxyapi-auth; "
                f"nohup {self._REMOTE_PROXY} -local-model "
                f"-config {self._REMOTE_CONFIG} "
                ">/logs/agent/cliproxyapi.log 2>&1 & "
                "echo $! >/logs/agent/cliproxyapi.pid; "
                "for attempt in $(seq 1 30); do "
                f"curl -fsS -H 'Authorization: Bearer {self._LOCAL_AUTH_TOKEN}' "
                "http://127.0.0.1:3456/v1/models >/dev/null && exit 0; "
                "sleep 1; done; "
                "tail -n 50 /logs/agent/cliproxyapi.log >&2; exit 1"
            ),
        )

    async def run(
        self,
        instruction: str,
        environment: BaseEnvironment,
        context: AgentContext,
    ) -> None:
        await self._start_proxy(environment)
        original_extra_env = self._extra_env
        original_base_url = os.environ.get("ANTHROPIC_BASE_URL")
        original_max_output_tokens = os.environ.get(
            "CLAUDE_CODE_MAX_OUTPUT_TOKENS"
        )
        model = self.model_name or "deepseek-v4-flash"
        self._extra_env = {
            **original_extra_env,
            "ANTHROPIC_API_KEY": self._LOCAL_AUTH_TOKEN,
            "ANTHROPIC_AUTH_TOKEN": self._LOCAL_AUTH_TOKEN,
            "ANTHROPIC_BASE_URL": self._BASE_URL,
            "ANTHROPIC_DEFAULT_OPUS_MODEL": model,
            "ANTHROPIC_DEFAULT_SONNET_MODEL": model,
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": model,
            "CLAUDE_CODE_SUBAGENT_MODEL": model,
            "NO_PROXY": "127.0.0.1,localhost",
            "no_proxy": "127.0.0.1,localhost",
        }
        if self._max_output_tokens is not None:
            self._extra_env["CLAUDE_CODE_MAX_OUTPUT_TOKENS"] = str(
                self._max_output_tokens
            )
        os.environ["ANTHROPIC_BASE_URL"] = self._BASE_URL
        if self._max_output_tokens is not None:
            os.environ["CLAUDE_CODE_MAX_OUTPUT_TOKENS"] = str(
                self._max_output_tokens
            )
        try:
            await ClaudeCode.run(self, instruction, environment, context)
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


class GoalOpenCodeGoClaudeCode(NativeClaudeGoal, OpenCodeGoClaudeCode):
    """Use Claude Code's native persistent goal with the OpenCode Go backend."""


class ResumeParaboxGoalOpenCodeGoClaudeCode(
    ParaboxResume,
    NativeClaudeGoal,
    ClaudeSessionResume,
    OpenCodeGoClaudeCode,
):
    """Resume the CC session and private Parabox state on OpenCode Go."""

    def __init__(
        self,
        *args,
        resume_sessions_dir: str,
        resume_workspace_dir: str | None = None,
        resume_from_last_compact: bool = False,
        resume_game_state_path: str,
        resume_game_audit_path: str | None = None,
        resume_game_events_path: str | None = None,
        **kwargs,
    ):
        super().__init__(*args, **kwargs)
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
