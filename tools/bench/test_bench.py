import json
import subprocess
from pathlib import Path

import pytest

from tools.bench import runs, tasks
from tools.bench.specs import load_game, load_profile

ROOT = Path(__file__).resolve().parents[2]


def test_every_game_spec_and_profile_loads():
    for spec in (ROOT / "games").glob("*/run.toml"):
        game = load_game(ROOT, spec.parent.name)
        assert (ROOT / game.task / "task.toml").is_file()
        assert "{session}" in game.objective
    for profile in (ROOT / "tools/agents/profiles").glob("*.toml"):
        load_profile(ROOT, profile.stem)


def test_config_renders_objective_env_and_game_kwargs():
    game = load_game(ROOT, "sokoban")
    profile = load_profile(ROOT, "pi-deepseek-v4-flash")
    config = runs.job_config(ROOT, "job", game, profile, profile.start_agent("sokoban"), {}, [Path("/n.md")])
    agent = config["agents"][0]
    assert agent["name"] == "tools.agents.pi_goal:SokobanGoalPi"
    assert agent["kwargs"]["max_score"] == 305 and agent["kwargs"]["thinking"] == "xhigh"
    assert "same Pi session" in agent["kwargs"]["goal_objective"]
    assert agent["env"] == {"DEEPSEEK_API_KEY": "${DEEPSEEK_API_KEY}"}
    assert config["jobs_dir"] == str(ROOT / ".harbor/jobs")
    assert config["extra_instruction_paths"] == ["/n.md"]


def test_builtin_agents_get_no_objective():
    config = runs.job_config(ROOT, "job", load_game(ROOT, "sokoban"), load_profile(ROOT, "nop"), "nop", {}, [])
    assert config["agents"][0] == {"name": "nop", "kwargs": {}}


def test_resume_kwargs_follow_the_agent_signature(tmp_path):
    manifest = {"artifacts": {
        "agent_session": {"path": "sessions"}, "workspace": None,
        "game_state": {"path": "game/parabox-state.txt"}, "game_audit": {"path": "game/parabox-audit.tsv"},
    }}
    kwargs = runs.resume_kwargs("tools.agents.isolated_codex:ContinueParaboxGoalResumeIsolatedCodex", tmp_path, manifest)
    assert kwargs == {
        "resume_sessions_dir": str(tmp_path / "sessions"),
        "resume_game_state_path": str(tmp_path / "game/parabox-state.txt"),
        "resume_game_audit_path": str(tmp_path / "game/parabox-audit.tsv"),
    }
    with pytest.raises(SystemExit, match="resume_game_state_path"):
        runs.resume_kwargs("tools.agents.isolated_codex:ContinueParaboxGoalResumeIsolatedCodex", tmp_path,
                           {"artifacts": {"agent_session": {"path": "sessions"}}})
    assert runs.resume_kwargs("nop", tmp_path, manifest) == {}


def test_checkpoint_hashes_detect_tampering(tmp_path):
    (tmp_path / "game").mkdir()
    (tmp_path / "game/audit.jsonl").write_text("{}\n")
    manifest = {"schema": runs.CHECKPOINT_SCHEMA, "artifacts": {
        "game_audit": {"path": "game/audit.jsonl", "sha256": runs.tree_digest(tmp_path / "game/audit.jsonl")}}}
    (tmp_path / "manifest.json").write_text(json.dumps(manifest))
    runs.verify_checkpoint(tmp_path)
    (tmp_path / "game/audit.jsonl").write_text("{}\n{}\n")
    with pytest.raises(SystemExit, match="does not match"):
        runs.verify_checkpoint(tmp_path)


def test_dirty_tree_blocks_scored_runs(tmp_path):
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True)
    (tmp_path / "tools").mkdir()
    (tmp_path / "tools/x.py").write_text("x = 1\n")
    with pytest.raises(SystemExit, match="tools/x.py"):
        runs.require_clean(tmp_path)


def test_task_data_copies_match_their_game_sources():
    for game in tasks.games_with_tasks(ROOT):
        assert tasks.data_mismatches(ROOT, game) == []


def test_compact_only_touches_finished_agent_logs(tmp_path):
    trial = tmp_path / ".harbor/jobs/job-a/trial-1/agent"
    (trial / "sessions").mkdir(parents=True)
    (trial / "claude-code.txt").write_text("x" * (2 << 20))
    (trial / "small.txt").write_text("x")
    (trial / "sessions/session.jsonl").write_text("y" * (2 << 20))
    running = tmp_path / ".harbor/jobs/job-b/trial-1/agent"
    running.mkdir(parents=True)
    (running / "claude-code.txt").write_text("x" * (2 << 20))
    (tmp_path / ".harbor/jobs/job-a/result.json").write_text("{}")
    files, saved = runs.compact(tmp_path)
    assert files == 1 and saved > 0
    assert (trial / "claude-code.txt.zst").is_file() and not (trial / "claude-code.txt").exists()
    assert (trial / "small.txt").is_file() and (trial / "sessions/session.jsonl").is_file()
    assert (running / "claude-code.txt").is_file()
