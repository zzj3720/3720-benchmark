import pytest

from kimi_claude_code import KimiClaudeCode


def test_kimi_uses_safe_fallback_label_for_ad_hoc_runs(tmp_path):
    agent = KimiClaudeCode(
        logs_dir=tmp_path,
        model_name="k3[1m]",
    )

    assert agent._credential_label == "unlabeled-local-key"


def test_kimi_requires_nonempty_credential_label(tmp_path):
    with pytest.raises(ValueError, match="credential_label must not be empty"):
        KimiClaudeCode(
            logs_dir=tmp_path,
            model_name="k3[1m]",
            credential_label=" ",
        )


def test_kimi_rejects_oversized_credential_label(tmp_path):
    with pytest.raises(ValueError, match="at most 80 characters"):
        KimiClaudeCode(
            logs_dir=tmp_path,
            model_name="k3[1m]",
            credential_label="x" * 81,
        )


def test_kimi_allows_long_reasoning_output():
    assert KimiClaudeCode._MAX_OUTPUT_TOKENS == "131072"
