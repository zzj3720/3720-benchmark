"""Persistent Pi benchmark agent for the OpenCode Go free model."""

from __future__ import annotations

import json
import shlex
from pathlib import Path
from typing import Any

from harbor.agents.installed.base import CliFlag
from harbor.agents.installed.node_install import nvm_node_install_snippet
from harbor.environments.base import BaseEnvironment

from tools.agents.pi_goal import CampaignGoalPi


class OpenCodeGoCampaignGoalPi(CampaignGoalPi):
    """Run one persistent Pi session against Ox Alpha Free at true max thinking."""

    _DEFAULT_VERSION = "0.83.0"
    _MODEL = "opencode-go/ox-alpha-free"
    _MODELS_PATH = Path(__file__).with_name(
        "opencode_go_ox_alpha_free.models.json"
    )
    CLI_FLAGS = [
        CliFlag(
            "reasoning_effort",
            cli="--thinking",
            type="enum",
            choices=["low", "high", "max"],
            default="max",
        )
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
        if self.model_name != self._MODEL:
            raise ValueError(f"model_name must be {self._MODEL}")
        self._auth_path = Path(
            auth_path or Path.home() / ".local/share/opencode/auth.json"
        )
        self._extra_env["OPENCODE_API_KEY"] = self._api_key()

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

    async def install(self, environment: BaseEnvironment) -> None:
        await self.exec_as_root(
            environment,
            command="apt-get update && apt-get install -y curl",
            env={"DEBIAN_FRONTEND": "noninteractive"},
        )
        package = shlex.quote(
            f"@earendil-works/pi-coding-agent@{self.version()}"
        )
        await self.exec_as_agent(
            environment,
            command=(
                "set -euo pipefail; "
                f"{nvm_node_install_snippet()} && "
                f"npm install -g {package} && "
                "mkdir -p $HOME/.pi/agent && "
                "printf '%s\\n' "
                "'{\"compaction\":{\"enabled\":true,\"reserveTokens\":65536,"
                "\"keepRecentTokens\":20000}}' "
                "> $HOME/.pi/agent/settings.json"
            ),
        )
        remote_models = "/tmp/opencode-go-ox-alpha-free.models.json"
        await environment.upload_file(self._MODELS_PATH, remote_models)
        await self.exec_as_agent(
            environment,
            command=(
                "set -euo pipefail; "
                f"install -m 600 {shlex.quote(remote_models)} "
                "$HOME/.pi/agent/models.json; "
                ". ~/.nvm/nvm.sh; "
                "pi --version; "
                "pi --list-models ox-alpha-free | grep -F ox-alpha-free"
            ),
        )
