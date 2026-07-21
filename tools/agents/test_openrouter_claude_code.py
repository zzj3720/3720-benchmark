import pytest

from openrouter_claude_code import OpenRouterClaudeCode


def test_openrouter_uses_anthropic_skin_limits(tmp_path):
    agent = OpenRouterClaudeCode(
        logs_dir=tmp_path,
        model_name="google/gemini-3.6-flash",
    )

    assert agent._BASE_URL == "https://openrouter.ai/api"
    assert agent._CONTEXT_TOKENS == "1048576"
    assert agent._MAX_OUTPUT_TOKENS == "65536"


def test_openrouter_requires_nonempty_credential_label(tmp_path):
    with pytest.raises(ValueError, match="credential_label must not be empty"):
        OpenRouterClaudeCode(
            logs_dir=tmp_path,
            model_name="google/gemini-3.6-flash",
            credential_label=" ",
        )


def test_openrouter_rejects_oversized_credential_label(tmp_path):
    with pytest.raises(ValueError, match="at most 80 characters"):
        OpenRouterClaudeCode(
            logs_dir=tmp_path,
            model_name="google/gemini-3.6-flash",
            credential_label="x" * 81,
        )
