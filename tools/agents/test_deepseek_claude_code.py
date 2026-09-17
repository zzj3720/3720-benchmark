import os

import pytest
from harbor.agents.installed.claude_code import ClaudeCode

from deepseek_claude_code import DeepSeekClaudeCode


@pytest.mark.asyncio
async def test_max_output_tokens_reaches_claude_host_environment(
    monkeypatch, tmp_path
):
    observed = {}

    async def inspect_environment(self, instruction, environment, context):
        observed["base_url"] = os.environ.get("ANTHROPIC_BASE_URL")
        observed["max_output_tokens"] = os.environ.get(
            "CLAUDE_CODE_MAX_OUTPUT_TOKENS"
        )
        observed["auto_compact_tokens"] = self._resolved_env_vars.get(
            "CLAUDE_CODE_AUTO_COMPACT_WINDOW"
        )
        observed["context_tokens"] = self._resolved_env_vars.get(
            "CLAUDE_CODE_MAX_CONTEXT_TOKENS"
        )

    monkeypatch.setattr(ClaudeCode, "run", inspect_environment)
    monkeypatch.delenv("ANTHROPIC_BASE_URL", raising=False)
    monkeypatch.delenv("CLAUDE_CODE_MAX_OUTPUT_TOKENS", raising=False)
    agent = DeepSeekClaudeCode(
        logs_dir=tmp_path,
        model_name="deepseek-v4-flash",
        max_output_tokens=12_000,
        extra_env={"DEEPSEEK_API_KEY": "test-key"},
    )

    await agent.run("continue", None, None)

    assert observed == {
        "base_url": DeepSeekClaudeCode._BASE_URL,
        "max_output_tokens": "12000",
        "auto_compact_tokens": DeepSeekClaudeCode._AUTO_COMPACT_TOKENS,
        "context_tokens": DeepSeekClaudeCode._CONTEXT_TOKENS,
    }
    assert "ANTHROPIC_BASE_URL" not in os.environ
    assert "CLAUDE_CODE_MAX_OUTPUT_TOKENS" not in os.environ
