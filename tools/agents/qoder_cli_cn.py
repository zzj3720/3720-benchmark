"""Harbor adapter for the authenticated Qoder CN CLI."""

import json
import shlex
from pathlib import Path
from typing import Any, override

from harbor.agents.installed.base import (
    BaseInstalledAgent,
    CliFlag,
    with_prompt_template,
)
from harbor.environments.base import BaseEnvironment
from harbor.models.agent.context import AgentContext
from harbor.models.trial.paths import EnvironmentPaths

from tools.agents.game_support.parabox import ParaboxResume


class QoderCliCn(BaseInstalledAgent):
    """Run Qoder CN in non-interactive mode and retain its raw JSON session."""

    SUPPORTS_ATIF = False
    SUPPORTS_RESUME = True
    _CLI_TOOLS = ("Bash", "Read", "Write", "Grep", "Glob")
    _CONFIG_DIR = "/logs/agent/sessions"
    CLI_FLAGS = [
        CliFlag(
            "reasoning_effort",
            cli="--reasoning-effort",
            type="enum",
            choices=["low", "medium", "high", "xhigh", "max"],
            default="max",
        ),
    ]

    def __init__(
        self,
        logs_dir: Path,
        auth_dir: str | None = None,
        stall_timeout_sec: int = 600,
        max_session_restarts: int = 8,
        *args,
        **kwargs,
    ):
        super().__init__(logs_dir, *args, **kwargs)
        self._auth_dir = Path(auth_dir or Path.home() / ".qoder-cn" / ".auth")
        if stall_timeout_sec < 60:
            raise ValueError("stall_timeout_sec must be at least 60")
        if max_session_restarts < 0:
            raise ValueError("max_session_restarts cannot be negative")
        self._stall_timeout_sec = stall_timeout_sec
        self._max_session_restarts = max_session_restarts

    @staticmethod
    @override
    def name() -> str:
        return "qoder-cli-cn"

    @override
    def get_version_command(self) -> str | None:
        return 'export PATH="$HOME/.local/bin:$PATH"; qoderclicn --version'

    @override
    async def install(self, environment: BaseEnvironment) -> None:
        await self.exec_as_root(
            environment,
            command=(
                "apt-get update && "
                "apt-get install -y --no-install-recommends ca-certificates curl"
            ),
            env={"DEBIAN_FRONTEND": "noninteractive"},
        )
        await self.exec_as_agent(
            environment,
            command=(
                "set -euo pipefail; "
                "curl -fsSL https://qoder.com.cn/install | bash; "
                'export PATH="$HOME/.local/bin:$PATH"; '
                "qoderclicn --version"
            ),
        )

    @override
    async def setup(self, environment: BaseEnvironment) -> None:
        await super().setup(environment)
        user_path = self._auth_dir / "user"
        machine_id_path = self._auth_dir / "machine_id"
        installation_id_path = self._auth_dir.parent / "installation_id"
        for path in (user_path, machine_id_path, installation_id_path):
            if not path.is_file():
                raise ValueError(f"Qoder CN login credential is missing: {path}")

        remote_secret_dir = "/installed-agent/qoder-cn-auth"
        await self.exec_as_root(
            environment,
            command=(
                f"mkdir -p {remote_secret_dir} {self._CONFIG_DIR}/.auth "
                f"&& chmod -R 777 {self._CONFIG_DIR}"
            ),
        )
        await environment.upload_file(
            user_path,
            f"{remote_secret_dir}/user",
        )
        await environment.upload_file(
            machine_id_path,
            f"{remote_secret_dir}/machine_id",
        )
        await environment.upload_file(
            installation_id_path,
            f"{remote_secret_dir}/installation_id",
        )
        await self.exec_as_agent(
            environment,
            command=(
                f"cp {remote_secret_dir}/user {self._CONFIG_DIR}/.auth/user; "
                f"cp {remote_secret_dir}/machine_id "
                f"{self._CONFIG_DIR}/.auth/machine_id; "
                f"cp {remote_secret_dir}/installation_id "
                f"{self._CONFIG_DIR}/installation_id; "
                f"chmod 600 {self._CONFIG_DIR}/.auth/user "
                f"{self._CONFIG_DIR}/.auth/machine_id "
                f"{self._CONFIG_DIR}/installation_id"
            ),
        )

    def _result_event(self) -> dict[str, Any] | None:
        output_path = self.logs_dir / "qoder-cn.jsonl"
        try:
            lines = output_path.read_text(encoding="utf-8").splitlines()
        except OSError:
            return None
        for line in reversed(lines):
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            if isinstance(event, dict) and event.get("type") == "result":
                return event
        return None

    @override
    def populate_context_post_run(self, context: AgentContext) -> None:
        result = self._result_event()
        context.metadata = {
            "session_format": "qoder-cn-stream-json",
            "token_accounting": "unavailable",
            "reported_model_usage": result.get("modelUsage") if result else None,
        }
        # Qoder CN 1.1.0 reports zero token counts for subscription models.
        # Leave Harbor token fields unset so the ranking code cannot mistake
        # unavailable accounting for a genuinely zero-token trial.

    @override
    @with_prompt_template
    async def run(
        self,
        instruction: str,
        environment: BaseEnvironment,
        context: AgentContext,
    ) -> None:
        model = shlex.quote(self.model_name or "Auto")
        cli_flags = self.build_cli_flags()
        tools = " ".join(self._CLI_TOOLS)
        resume_flag = "--continue " if self._resume else ""
        instruction_env = "HARBOR_QODER_CN_INSTRUCTION"
        continuation_env = "HARBOR_QODER_CN_CONTINUATION"
        await self.exec_as_agent(
            environment,
            command=(
                'set -uo pipefail; export PATH="$HOME/.local/bin:$PATH"; '
                "output=/logs/agent/qoder-cn.jsonl; "
                "errors=/logs/agent/qoder-cn.stderr; "
                "supervisor=/logs/agent/qoder-cn-supervisor.jsonl; "
                ': >"$output"; : >"$errors"; : >"$supervisor"; '
                "attempt=0; "
                f'resume_flag={shlex.quote(resume_flag.strip())}; '
                "while :; do "
                "attempt=$((attempt + 1)); "
                f'if [ "$attempt" -eq 1 ]; then prompt="${instruction_env}"; '
                f'else prompt="${continuation_env}"; resume_flag=--continue; fi; '
                'printf \'{"event":"start","attempt":%s,"timestamp":%s}\\n\' '
                '"$attempt" "$(date +%s)" >>"$supervisor"; '
                f"qoderclicn --config-dir {self._CONFIG_DIR} "
                f"--print --model {model} {cli_flags} "
                "--output-format stream-json "
                "--permission-mode bypass_permissions "
                f"--tools {tools} "
                '$resume_flag -- "$prompt" >>"$output" 2>>"$errors" & '
                "qoder_pid=$!; stalled=0; "
                "while kill -0 \"$qoder_pid\" 2>/dev/null; do "
                "sleep 15; now=$(date +%s); "
                'modified=$(stat -c %Y "$output" 2>/dev/null || echo "$now"); '
                f"if [ $((now - modified)) -ge {self._stall_timeout_sec} ]; then "
                "stalled=1; kill -TERM \"$qoder_pid\" 2>/dev/null || true; "
                "sleep 5; kill -KILL \"$qoder_pid\" 2>/dev/null || true; break; fi; "
                "done; "
                'wait "$qoder_pid"; code=$?; '
                'printf \'{"event":"exit","attempt":%s,"code":%s,'
                '"stalled":%s,"timestamp":%s}\\n\' '
                '"$attempt" "$code" "$stalled" "$(date +%s)" >>"$supervisor"; '
                'if [ "$stalled" -eq 0 ] && [ "$code" -eq 0 ]; then exit 0; fi; '
                'if grep -q \'"error":"billing_error"\' "$output"; then '
                'exit "${code:-1}"; fi; '
                f"if [ \"$attempt\" -gt {self._max_session_restarts} ]; then "
                'exit "${code:-1}"; fi; '
                "done"
            ),
            env={
                instruction_env: instruction,
                continuation_env: (
                    "The previous Qoder request stalled or failed because of a "
                    "transient runtime or network problem. Resume this same "
                    "session and continue the task from its current game state. "
                    "Do not repeat the interrupted response or stop at partial "
                    "progress."
                ),
            },
        )


class ResumeParaboxQoderCliCn(ParaboxResume, QoderCliCn):
    """Resume Qoder's native session and the exact private Parabox state."""

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
        self._resume_sessions_dir = Path(resume_sessions_dir)
        self._resume_workspace_dir = (
            Path(resume_workspace_dir) if resume_workspace_dir else None
        )
        if not self._resume_sessions_dir.is_dir():
            raise ValueError(
                f"resume_sessions_dir is not a directory: "
                f"{self._resume_sessions_dir}"
            )
        if (
            self._resume_workspace_dir is not None
            and not self._resume_workspace_dir.is_dir()
        ):
            raise ValueError(
                f"resume_workspace_dir is not a directory: "
                f"{self._resume_workspace_dir}"
            )
        self._configure_parabox_resume(
            resume_game_state_path=resume_game_state_path,
            resume_game_audit_path=resume_game_audit_path,
            resume_game_events_path=resume_game_events_path,
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
        await self._restore_parabox(environment)

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
