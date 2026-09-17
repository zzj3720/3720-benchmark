import json
from pathlib import Path

import pytest

from tools.agents.opencode_go import OpenCodeGo, ResumeParaboxGoalOpenCodeGo


def credential(path: Path) -> None:
    path.write_text(
        json.dumps({"opencode-go": {"type": "api", "key": "test-key"}}),
        encoding="utf-8",
    )


def test_reads_private_opencode_go_credential(tmp_path: Path) -> None:
    auth = tmp_path / "auth.json"
    credential(auth)
    agent = OpenCodeGo(logs_dir=tmp_path / "logs", auth_path=str(auth))
    assert agent._api_key() == "test-key"
    assert agent._extra_env["OPENCODE_API_KEY"] == "test-key"
    assert agent._extra_env["OPENCODE_EXPERIMENTAL_OUTPUT_TOKEN_MAX"] == "32000"
    assert agent._extra_env["XDG_DATA_HOME"] == "/logs/agent/opencode-data"


def test_rejects_non_go_model(tmp_path: Path) -> None:
    auth = tmp_path / "auth.json"
    credential(auth)
    agent = OpenCodeGo(
        logs_dir=tmp_path / "logs",
        auth_path=str(auth),
        model_name="deepseek/deepseek-v4-flash",
    )
    with pytest.raises(ValueError, match="opencode-go provider"):
        agent._model()


def test_config_preserves_prior_compaction_boundary() -> None:
    config = OpenCodeGo._config()
    assert config["compaction"] == {
        "auto": True,
        "prune": True,
        "reserved": 148_032,
    }
    assert config["permission"]["webfetch"] == "deny"
    assert config["permission"]["websearch"] == "deny"


def test_populates_usage_from_opencode_events(tmp_path: Path) -> None:
    auth = tmp_path / "auth.json"
    credential(auth)
    logs = tmp_path / "logs"
    logs.mkdir()
    (logs / "opencode.jsonl").write_text(
        json.dumps(
            {
                "type": "step_finish",
                "sessionID": "ses_test",
                "part": {
                    "tokens": {
                        "input": 100,
                        "output": 20,
                        "reasoning": 30,
                        "cache": {"read": 400},
                    },
                    "cost": 0.5,
                },
            }
        )
        + "\n",
        encoding="utf-8",
    )
    agent = OpenCodeGo(logs_dir=logs, auth_path=str(auth))

    class Context:
        pass

    context = Context()
    agent.populate_context_post_run(context)
    assert context.n_input_tokens == 500
    assert context.n_output_tokens == 50
    assert context.n_cache_tokens == 400
    assert context.cost_usd == 0.5
    assert context.metadata["session_ids"] == ["ses_test"]


def test_resume_requires_existing_native_data_directory(tmp_path: Path) -> None:
    state = tmp_path / "state.txt"
    state.write_text(
        "parabox-state-v4\ncampaign parabox-complete-364-v11\n",
        encoding="utf-8",
    )
    with pytest.raises(ValueError, match="resume_opencode_data_dir"):
        ResumeParaboxGoalOpenCodeGo(
            logs_dir=tmp_path / "logs",
            resume_opencode_data_dir=str(tmp_path / "missing"),
            resume_game_state_path=str(state),
        )
