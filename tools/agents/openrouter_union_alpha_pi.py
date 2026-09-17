"""Persistent Pi benchmark agent for OpenRouter's stealth/union-alpha model."""

from __future__ import annotations

import shlex
from pathlib import Path
from typing import Any

from harbor.agents.installed.node_install import nvm_node_install_snippet
from harbor.environments.base import BaseEnvironment

from tools.agents.pi_goal import CampaignGoalPi


class OpenRouterUnionAlphaCampaignGoalPi(CampaignGoalPi):
    """Run one persistent Pi session against OpenRouter stealth/union-alpha.

    Union Alpha advertises no reasoning-effort control: the endpoint accepts a
    ``reasoning`` parameter but reports zero reasoning tokens and returns no
    reasoning content, so no ``--thinking`` level is declared here.
    """

    _DEFAULT_VERSION = "0.83.0"
    _MODEL = "openrouter/stealth/union-alpha"
    _MODELS_PATH = Path(__file__).with_name(
        "openrouter_union_alpha.models.json"
    )

    def __init__(
        self,
        logs_dir: Path,
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
        self._extra_env["OPENROUTER_API_KEY"] = self._api_key()

    def _api_key(self) -> str:
        key = self._get_env("OPENROUTER_API_KEY")
        if not key:
            raise ValueError(
                "OPENROUTER_API_KEY is required for "
                "OpenRouterUnionAlphaCampaignGoalPi"
            )
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
        remote_models = "/tmp/openrouter-union-alpha.models.json"
        await environment.upload_file(self._MODELS_PATH, remote_models)
        await self.exec_as_agent(
            environment,
            command=(
                "set -euo pipefail; "
                f"install -m 600 {shlex.quote(remote_models)} "
                "$HOME/.pi/agent/models.json; "
                ". ~/.nvm/nvm.sh; "
                "pi --version; "
                "pi --list-models union-alpha | grep -F union-alpha"
            ),
        )
