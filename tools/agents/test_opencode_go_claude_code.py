import asyncio
import json
import os

from harbor.agents.installed.claude_code import ClaudeCode

from tools.agents.opencode_go_claude_code import OpenCodeGoClaudeCode


def _files(tmp_path):
    binary = tmp_path / "proxy"
    config = tmp_path / "config.yaml"
    auth = tmp_path / "auth.json"
    binary.write_bytes(b"proxy")
    config.write_text(
        "api-key: __OPENCODE_GO_API_KEY__\n", encoding="utf-8"
    )
    auth.write_text(
        json.dumps({"opencode-go": {"key": "private-test-key"}}),
        encoding="utf-8",
    )
    return binary, config, auth


def _agent(tmp_path):
    binary, config, auth = _files(tmp_path)
    return OpenCodeGoClaudeCode(
        logs_dir=tmp_path / "logs",
        model_name="deepseek-v4-flash",
        max_output_tokens=32_000,
        reasoning_effort="max",
        proxy_binary_path=str(binary),
        proxy_config_path=str(config),
        opencode_auth_path=str(auth),
    )


def test_proxy_start_command_reads_key_inside_container(tmp_path, monkeypatch):
    agent = _agent(tmp_path)
    observed = {}

    async def capture(environment, command, **kwargs):
        observed["command"] = command

    monkeypatch.setattr(agent, "exec_as_agent", capture)
    asyncio.run(agent._start_proxy(None))

    assert "private-test-key" not in observed["command"]
    assert agent._REMOTE_AUTH in observed["command"]
    assert "python3 -c" in observed["command"]
    assert "ROUTATIC" not in observed["command"]
    assert "cliproxyapi -local-model" in observed["command"]
    assert "http://127.0.0.1:3456/v1/models" in observed["command"]


def test_claude_run_uses_local_proxy_and_restores_host_env(
    tmp_path, monkeypatch
):
    agent = _agent(tmp_path)
    observed = {}

    async def proxy_started(environment):
        observed["proxy_started"] = True

    async def capture(self, instruction, environment, context):
        observed["instruction"] = instruction
        observed["base_url"] = os.environ.get("ANTHROPIC_BASE_URL")
        observed["output_tokens"] = os.environ.get(
            "CLAUDE_CODE_MAX_OUTPUT_TOKENS"
        )
        observed["extra_env"] = dict(self._extra_env)

    monkeypatch.setattr(agent, "_start_proxy", proxy_started)
    monkeypatch.setattr(ClaudeCode, "run", capture)
    monkeypatch.delenv("ANTHROPIC_BASE_URL", raising=False)
    monkeypatch.delenv("CLAUDE_CODE_MAX_OUTPUT_TOKENS", raising=False)

    asyncio.run(agent.run("continue", None, None))

    assert observed["proxy_started"]
    assert observed["instruction"] == "continue"
    assert observed["base_url"] == "http://127.0.0.1:3456"
    assert observed["output_tokens"] == "32000"
    assert observed["extra_env"]["ANTHROPIC_API_KEY"] == (
        "harbor-local-claude-code"
    )
    assert observed["extra_env"]["ANTHROPIC_BASE_URL"] == (
        "http://127.0.0.1:3456"
    )
    assert observed["extra_env"]["NO_PROXY"] == "127.0.0.1,localhost"
    assert agent._resolved_env_vars["CLAUDE_CODE_MAX_CONTEXT_TOKENS"] == (
        "1048576"
    )
    assert agent._resolved_env_vars["CLAUDE_CODE_AUTO_COMPACT_WINDOW"] == (
        "851968"
    )
    assert "ANTHROPIC_BASE_URL" not in os.environ
    assert "CLAUDE_CODE_MAX_OUTPUT_TOKENS" not in os.environ
