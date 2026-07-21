from pathlib import Path

import pytest

from tools.agents.qoder_cli_cn import QoderCliCn, ResumeParaboxQoderCliCn


def test_rejects_too_short_stall_timeout(tmp_path: Path) -> None:
    with pytest.raises(ValueError, match="at least 60"):
        QoderCliCn(logs_dir=tmp_path, stall_timeout_sec=59)


def test_rejects_negative_session_restart_count(tmp_path: Path) -> None:
    with pytest.raises(ValueError, match="cannot be negative"):
        QoderCliCn(logs_dir=tmp_path, max_session_restarts=-1)


def test_resume_requires_existing_session_directory(tmp_path: Path) -> None:
    state = tmp_path / "state.txt"
    state.write_text("parabox-state-v4\n", encoding="utf-8")
    with pytest.raises(ValueError, match="resume_sessions_dir"):
        ResumeParaboxQoderCliCn(
            logs_dir=tmp_path / "logs",
            resume_sessions_dir=str(tmp_path / "missing"),
            resume_game_state_path=str(state),
        )


def test_resume_requires_existing_workspace_directory(tmp_path: Path) -> None:
    sessions = tmp_path / "sessions"
    sessions.mkdir()
    state = tmp_path / "state.txt"
    state.write_text(
        "parabox-state-v4\ncampaign parabox-complete-364-v11\n",
        encoding="utf-8",
    )
    with pytest.raises(ValueError, match="resume_workspace_dir"):
        ResumeParaboxQoderCliCn(
            logs_dir=tmp_path / "logs",
            resume_sessions_dir=str(sessions),
            resume_workspace_dir=str(tmp_path / "missing"),
            resume_game_state_path=str(state),
        )
