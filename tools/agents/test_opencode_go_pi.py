import asyncio
import json
from pathlib import Path

import pytest

from tools.agents.opencode_go_pi import OpenCodeGoCampaignGoalPi


def _credential(path: Path) -> None:
    path.write_text(
        json.dumps({"opencode-go": {"type": "api", "key": "test-key"}}),
        encoding="utf-8",
    )


def _agent(tmp_path: Path, **kwargs) -> OpenCodeGoCampaignGoalPi:
    auth = tmp_path / "auth.json"
    _credential(auth)
    return OpenCodeGoCampaignGoalPi(
        logs_dir=tmp_path / "logs",
        auth_path=str(auth),
        model_name="opencode-go/ox-alpha-free",
        reasoning_effort="max",
        api_version="parabox-api-v3",
        game_command="parabox",
        max_score=364,
        goal_objective="Solve the complete campaign.",
        **kwargs,
    )


def test_uses_private_go_credential_and_true_max_flag(tmp_path: Path) -> None:
    agent = _agent(tmp_path)

    assert agent._extra_env["OPENCODE_API_KEY"] == "test-key"
    assert agent.build_cli_flags() == "--thinking max"
    assert agent.version() == "0.83.0"


def test_model_catalog_exposes_only_the_pinned_free_model() -> None:
    catalog = json.loads(
        OpenCodeGoCampaignGoalPi._MODELS_PATH.read_text(encoding="utf-8")
    )
    provider = catalog["providers"]["opencode-go"]
    model = provider["models"][0]

    assert provider["apiKey"] == "$OPENCODE_API_KEY"
    assert [entry["id"] for entry in provider["models"]] == ["ox-alpha-free"]
    assert model["cost"] == {
        "input": 0,
        "output": 0,
        "cacheRead": 0,
        "cacheWrite": 0,
    }
    assert model["thinkingLevelMap"]["max"] == "max"


def test_rejects_any_other_model(tmp_path: Path) -> None:
    auth = tmp_path / "auth.json"
    _credential(auth)
    with pytest.raises(ValueError, match="opencode-go/ox-alpha-free"):
        OpenCodeGoCampaignGoalPi(
            logs_dir=tmp_path / "logs",
            auth_path=str(auth),
            model_name="opencode-go/deepseek-v4-flash",
            reasoning_effort="max",
            api_version="parabox-api-v3",
            game_command="parabox",
            max_score=364,
            goal_objective="Solve the complete campaign.",
        )


def test_install_places_catalog_in_the_runtime_users_home(
    monkeypatch, tmp_path: Path
) -> None:
    agent = _agent(tmp_path)
    commands: list[str] = []
    uploads: list[tuple[Path, str]] = []

    async def exec_as_root(_environment, **_kwargs) -> None:
        return None

    async def exec_as_agent(_environment, *, command: str, **_kwargs) -> None:
        commands.append(command)

    class Environment:
        async def upload_file(self, source: Path, destination: str) -> None:
            uploads.append((source, destination))

    monkeypatch.setattr(agent, "exec_as_root", exec_as_root)
    monkeypatch.setattr(agent, "exec_as_agent", exec_as_agent)

    asyncio.run(agent.install(Environment()))

    assert uploads == [
        (
            OpenCodeGoCampaignGoalPi._MODELS_PATH,
            "/tmp/opencode-go-ox-alpha-free.models.json",
        )
    ]
    assert "$HOME/.pi/agent/models.json" in commands[-1]
    assert "/home/agent/.pi/agent/models.json" not in "\n".join(commands)
