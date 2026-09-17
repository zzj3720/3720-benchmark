"""OpenCode Go agents with private credentials and durable native sessions."""

from __future__ import annotations

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

from tools.agents.parabox_resume import ParaboxResume


class OpenCodeGo(BaseInstalledAgent):
    """Run OpenCode against the subscription-only OpenCode Go provider."""

    SUPPORTS_RESUME = True
    _DEFAULT_VERSION = "1.18.11"
    _DATA_HOME = "/logs/agent/opencode-data"
    _OUTPUT_PATH = Path("opencode.jsonl")
    _CONFIG_PATH = "/installed-agent/opencode.json"
    CLI_FLAGS = [
        CliFlag(
            "reasoning_effort",
            cli="--variant",
            type="enum",
            choices=["high", "max"],
            default="max",
        ),
    ]

    def __init__(
        self,
        logs_dir: Path,
        auth_path: str | None = None,
        version: str | None = None,
        *args: Any,
        **kwargs: Any,
    ) -> None:
        super().__init__(
            logs_dir,
            *args,
            version=version or self._DEFAULT_VERSION,
            **kwargs,
        )
        self._auth_path = Path(
            auth_path or Path.home() / ".local/share/opencode/auth.json"
        )
        self._extra_env.update(
            {
                "OPENCODE_API_KEY": self._api_key(),
                "OPENCODE_CONFIG_CONTENT": json.dumps(
                    self._config(), separators=(",", ":")
                ),
                "OPENCODE_EXPERIMENTAL_OUTPUT_TOKEN_MAX": "32000",
                "XDG_DATA_HOME": self._DATA_HOME,
            }
        )

    @staticmethod
    @override
    def name() -> str:
        return "opencode-go"

    @override
    def get_version_command(self) -> str | None:
        return "opencode --version"

    def _api_key(self) -> str:
        configured = self._get_env("OPENCODE_API_KEY")
        if configured:
            return configured
        try:
            credentials = json.loads(self._auth_path.read_text(encoding="utf-8"))
            credential = credentials["opencode-go"]
            key = credential["key"]
        except (OSError, KeyError, TypeError, json.JSONDecodeError) as error:
            raise ValueError(
                "OpenCode Go credential is missing; run "
                "`opencode auth login -p opencode-go` first"
            ) from error
        if credential.get("type") != "api" or not isinstance(key, str) or not key:
            raise ValueError("OpenCode Go credential is not a non-empty API key")
        return key

    @staticmethod
    def _config() -> dict[str, Any]:
        # The Go catalog advertises a 1,000,000-token context. A 148,032-token
        # reserve keeps the automatic boundary at 851,968, matching the prior
        # DeepSeek continuation while leaving output and compaction headroom.
        return {
            "$schema": "https://opencode.ai/config.json",
            "autoupdate": False,
            "share": "disabled",
            "compaction": {
                "auto": True,
                "prune": True,
                "reserved": 148_032,
            },
            "permission": {
                "question": "deny",
                "task": "deny",
                "webfetch": "deny",
                "websearch": "deny",
            },
            "mcp": {},
            "plugin": [],
        }

    @override
    async def install(self, environment: BaseEnvironment) -> None:
        version = shlex.quote(self.version() or self._DEFAULT_VERSION)
        await self.exec_as_root(
            environment,
            command=(
                "set -euo pipefail; "
                "if ! command -v curl >/dev/null 2>&1; then "
                "apt-get update; "
                "DEBIAN_FRONTEND=noninteractive apt-get install -y "
                "--no-install-recommends ca-certificates curl; "
                "fi; "
                f"version={version}; "
                'case "$(uname -m)" in '
                "aarch64|arm64) package=opencode-linux-arm64 ;; "
                "x86_64|amd64) package=opencode-linux-x64-baseline ;; "
                "*) echo 'unsupported OpenCode architecture' >&2; exit 1 ;; "
                "esac; "
                "directory=$(mktemp -d); "
                "archive=$directory/opencode.tgz; "
                'curl -fsSL "https://registry.npmjs.org/$package/-/'
                '$package-$version.tgz" -o "$archive"; '
                'tar -xzf "$archive" -C "$directory"; '
                'install -m 755 "$directory/package/bin/opencode" '
                "/usr/local/bin/opencode; "
                "opencode --version"
            ),
        )

    @override
    async def setup(self, environment: BaseEnvironment) -> None:
        self._api_key()
        await super().setup(environment)
        config = shlex.quote(json.dumps(self._config(), separators=(",", ":")))
        await self.exec_as_root(
            environment,
            command=(
                f"mkdir -p {self._DATA_HOME} /installed-agent; "
                f"printf %s {config} > {self._CONFIG_PATH}; "
                f"chmod 777 {self._DATA_HOME}; "
                f"chmod 644 {self._CONFIG_PATH}"
            ),
        )

    def _model(self) -> str:
        model = self.model_name or "opencode-go/deepseek-v4-flash"
        if not model.startswith("opencode-go/"):
            raise ValueError("OpenCode Go model must use the opencode-go provider")
        return model

    @override
    @with_prompt_template
    async def run(
        self,
        instruction: str,
        environment: BaseEnvironment,
        context: AgentContext,
    ) -> None:
        model = shlex.quote(self._model())
        flags = self.build_cli_flags()
        first_prompt = "HARBOR_OPENCODE_INSTRUCTION"
        next_prompt = "HARBOR_OPENCODE_CONTINUATION"
        resume = "--continue" if self._resume else ""
        await self.exec_as_agent(
            environment,
            cwd="/app",
            command=(
                "set -uo pipefail; "
                'test -n "${OPENCODE_API_KEY:-}"; '
                "output=/logs/agent/opencode.jsonl; "
                "errors=/logs/agent/opencode.stderr; "
                "supervisor=/logs/agent/opencode-supervisor.jsonl; "
                'touch "$output" "$errors" "$supervisor"; '
                "attempt=0; "
                f"resume_flag={shlex.quote(resume)}; "
                "while :; do "
                "attempt=$((attempt + 1)); "
                f'if [ "$attempt" -eq 1 ]; then prompt="${first_prompt}"; '
                f'else prompt="${next_prompt}"; resume_flag=--continue; fi; '
                'printf \'{"event":"start","attempt":%s,'
                '"timestamp":%s}\\n\' "$attempt" "$(date +%s)" '
                '>>"$supervisor"; '
                "opencode run --print-logs --log-level DEBUG --pure --auto "
                f"--model {model} {flags} --format json --thinking "
                '$resume_flag -- "$prompt" '
                '>>"$output" 2>>"$errors"; code=$?; '
                'printf \'{"event":"exit","attempt":%s,'
                '"code":%s,"timestamp":%s}\\n\' '
                '"$attempt" "$code" "$(date +%s)" >>"$supervisor"; '
                "status=$(/usr/local/bin/parabox status 2>/dev/null || true); "
                'if printf %s "$status" | grep -q \'"complete":true\'; then '
                "exit 0; fi; "
                'if [ "$code" -ne 0 ]; then exit "$code"; fi; '
                "sleep 1; "
                "done"
            ),
            env={
                first_prompt: instruction,
                next_prompt: (
                    "Continue the same benchmark task from the current game "
                    "state and workspace notes. Do not stop at partial "
                    "progress. Keep reasoning and acting until the game "
                    "reports authoritative completion or runtime ends."
                ),
            },
        )

    @override
    def populate_context_post_run(self, context: AgentContext) -> None:
        output = self.logs_dir / self._OUTPUT_PATH
        if not output.is_file():
            return
        input_tokens = 0
        output_tokens = 0
        cache_tokens = 0
        cost = 0.0
        session_ids: set[str] = set()
        for line in output.read_text(encoding="utf-8").splitlines():
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            session_id = event.get("sessionID")
            if isinstance(session_id, str):
                session_ids.add(session_id)
            if event.get("type") != "step_finish":
                continue
            part = event.get("part") or {}
            tokens = part.get("tokens") or {}
            cache = tokens.get("cache") or {}
            input_tokens += int(tokens.get("input") or 0)
            output_tokens += int(tokens.get("output") or 0)
            output_tokens += int(tokens.get("reasoning") or 0)
            cache_tokens += int(cache.get("read") or 0)
            cost += float(part.get("cost") or 0)
        context.n_input_tokens = input_tokens + cache_tokens
        context.n_output_tokens = output_tokens
        context.n_cache_tokens = cache_tokens
        context.cost_usd = cost if cost else None
        context.metadata = {
            "provider": "opencode-go",
            "session_format": "opencode-jsonl-and-sqlite",
            "session_ids": sorted(session_ids),
            "variant": self._resolved_flags.get("reasoning_effort"),
        }


class ResumeParaboxGoalOpenCodeGo(ParaboxResume, OpenCodeGo):
    """Start or resume OpenCode from saved notes and exact private game state."""

    def __init__(
        self,
        *args: Any,
        resume_workspace_dir: str | None = None,
        resume_opencode_data_dir: str | None = None,
        handoff_instruction: str | None = None,
        resume_game_state_path: str,
        resume_game_audit_path: str | None = None,
        resume_game_events_path: str | None = None,
        **kwargs: Any,
    ) -> None:
        super().__init__(*args, **kwargs)
        self._resume_workspace_dir = (
            Path(resume_workspace_dir) if resume_workspace_dir else None
        )
        self._resume_opencode_data_dir = (
            Path(resume_opencode_data_dir) if resume_opencode_data_dir else None
        )
        self._handoff_instruction = (handoff_instruction or "").strip()
        for label, path in (
            ("resume_workspace_dir", self._resume_workspace_dir),
            ("resume_opencode_data_dir", self._resume_opencode_data_dir),
        ):
            if path is not None and not path.is_dir():
                raise ValueError(f"{label} is not a directory: {path}")
        self._configure_parabox_resume(
            resume_game_state_path=resume_game_state_path,
            resume_game_audit_path=resume_game_audit_path,
            resume_game_events_path=resume_game_events_path,
        )

    @override
    async def setup(self, environment: BaseEnvironment) -> None:
        await super().setup(environment)
        if self._resume_opencode_data_dir is not None:
            await environment.upload_dir(
                self._resume_opencode_data_dir,
                self._DATA_HOME,
            )
        if self._resume_workspace_dir is not None:
            await environment.upload_dir(self._resume_workspace_dir, "/app")
        await self._restore_parabox(environment)

    @override
    async def run(
        self,
        instruction: str,
        environment: BaseEnvironment,
        context: AgentContext,
    ) -> None:
        self._resume = self._resume_opencode_data_dir is not None
        if self._handoff_instruction:
            instruction = f"{self._handoff_instruction}\n\n{instruction}"
        try:
            await super().run(instruction, environment, context)
        finally:
            self._resume = False
