import json

from isolated_codex import (
    ContinueSwarmGoalResumeIsolatedCodex,
    _goal_continuation_pending,
    _session_sync_start_command,
    _session_sync_stop_command,
)


def _write_session(tmp_path, events):
    session = tmp_path / "sessions" / "2026" / "07" / "20" / "rollout-test.jsonl"
    session.parent.mkdir(parents=True)
    session.write_text(
        "\n".join(json.dumps(event) for event in events) + "\n",
        encoding="utf-8",
    )
    return tmp_path


def test_detects_goal_turn_aborted_after_queued_continuation(tmp_path):
    logs_dir = _write_session(
        tmp_path,
        [
            {"type": "event_msg", "payload": {"type": "task_complete"}},
            {
                "type": "response_item",
                "payload": {
                    "type": "message",
                    "role": "user",
                    "content": [
                        {
                            "type": "input_text",
                            "text": ('<codex_internal_context source="goal">continue'),
                        }
                    ],
                },
            },
            {"type": "event_msg", "payload": {"type": "turn_aborted"}},
        ],
    )

    assert _goal_continuation_pending(logs_dir)


def test_detects_markerless_turn_started_after_completion(tmp_path):
    logs_dir = _write_session(
        tmp_path,
        [
            {"type": "event_msg", "payload": {"type": "task_complete"}},
            {"type": "event_msg", "payload": {"type": "task_started"}},
            {
                "type": "response_item",
                "payload": {
                    "type": "message",
                    "role": "user",
                    "content": [
                        {
                            "type": "input_text",
                            "text": "<turn_aborted>interrupted</turn_aborted>",
                        }
                    ],
                },
            },
            {"type": "event_msg", "payload": {"type": "turn_aborted"}},
        ],
    )

    assert _goal_continuation_pending(logs_dir)


def test_detects_queued_goal_turn_without_explicit_abort(tmp_path):
    logs_dir = _write_session(
        tmp_path,
        [
            {"type": "event_msg", "payload": {"type": "task_complete"}},
            {"type": "event_msg", "payload": {"type": "task_started"}},
            {"type": "turn_context", "payload": {}},
            {
                "type": "response_item",
                "payload": {
                    "type": "message",
                    "role": "user",
                    "content": [
                        {
                            "type": "input_text",
                            "text": ('<codex_internal_context source="goal">continue'),
                        }
                    ],
                },
            },
        ],
    )

    assert _goal_continuation_pending(logs_dir)


def test_ignores_ordinary_completed_turn(tmp_path):
    logs_dir = _write_session(
        tmp_path,
        [
            {"type": "event_msg", "payload": {"type": "task_complete"}},
            {
                "type": "response_item",
                "payload": {
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": "done"}],
                },
            },
        ],
    )

    assert not _goal_continuation_pending(logs_dir)


def test_requires_goal_marker_before_aborted_turn(tmp_path):
    logs_dir = _write_session(
        tmp_path,
        [
            {
                "type": "response_item",
                "payload": {
                    "type": "message",
                    "role": "user",
                    "content": [{"type": "input_text", "text": "ordinary prompt"}],
                },
            },
            {"type": "event_msg", "payload": {"type": "turn_aborted"}},
        ],
    )

    assert not _goal_continuation_pending(logs_dir)


def test_ignores_unfinished_non_goal_turn_without_abort(tmp_path):
    logs_dir = _write_session(
        tmp_path,
        [
            {"type": "event_msg", "payload": {"type": "task_complete"}},
            {"type": "event_msg", "payload": {"type": "task_started"}},
            {
                "type": "response_item",
                "payload": {
                    "type": "message",
                    "role": "user",
                    "content": [{"type": "input_text", "text": "ordinary prompt"}],
                },
            },
        ],
    )

    assert not _goal_continuation_pending(logs_dir)


def test_detects_active_goal_when_cli_cannot_queue_next_turn(tmp_path):
    logs_dir = _write_session(
        tmp_path,
        [
            {"type": "event_msg", "payload": {"type": "task_started"}},
            {
                "type": "response_item",
                "payload": {
                    "type": "custom_tool_call",
                    "name": "exec",
                    "input": (
                        'await tools.create_goal({objective:"do not call '
                        'update_goal yet"});'
                    ),
                },
            },
            *[
                {
                    "type": "response_item",
                    "payload": {"type": "reasoning", "encrypted_content": "x"},
                }
                for _ in range(40)
            ],
            {"type": "event_msg", "payload": {"type": "task_complete"}},
        ],
    )

    assert _goal_continuation_pending(logs_dir)


def test_goal_update_makes_completed_turn_terminal_without_queued_turn(tmp_path):
    logs_dir = _write_session(
        tmp_path,
        [
            {"type": "event_msg", "payload": {"type": "task_started"}},
            {
                "type": "response_item",
                "payload": {
                    "type": "custom_tool_call",
                    "name": "exec",
                    "input": 'await tools.create_goal({objective:"finish"});',
                },
            },
            {
                "type": "response_item",
                "payload": {
                    "type": "custom_tool_call",
                    "name": "exec",
                    "input": 'await tools.update_goal({status:"complete"});',
                },
            },
            {"type": "event_msg", "payload": {"type": "task_complete"}},
        ],
    )

    assert not _goal_continuation_pending(logs_dir)


def test_live_session_sync_checkpoints_codex_rollout_into_agent_logs():
    command = _session_sync_start_command()

    assert "/tmp/codex-home/sessions/." in command
    assert "/logs/agent/sessions/" in command
    assert "sleep 5" in command
    assert "nohup sh -c" in command


def test_live_session_sync_stop_is_best_effort():
    command = _session_sync_stop_command()

    assert "harbor-codex-session-sync.stop" in command
    assert 'kill "$(cat ' in command
    assert "|| true" in command


def test_swarm_resume_requires_an_existing_audit(tmp_path):
    missing = tmp_path / "missing-audit.jsonl"

    try:
        ContinueSwarmGoalResumeIsolatedCodex(
            model_name="test",
            logs_dir=tmp_path / "logs",
            resume_sessions_dir=str(tmp_path),
            resume_game_audit_path=str(missing),
            goal_objective="finish the Swarm scenario",
        )
    except ValueError as error:
        assert "Swarm resume artifact is missing" in str(error)
    else:
        raise AssertionError("missing Swarm audit was accepted")
