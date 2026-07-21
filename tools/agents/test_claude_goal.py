import json

from claude_goal import _latest_goal_stop_was_rejected


def _write_session(tmp_path, events):
    session = tmp_path / "sessions" / "projects" / "-app" / "session.jsonl"
    session.parent.mkdir(parents=True)
    session.write_text(
        "\n".join(json.dumps(event) for event in events) + "\n",
        encoding="utf-8",
    )
    return tmp_path


def test_detects_goal_stop_rejection_after_invalid_hook_json(tmp_path):
    logs_dir = _write_session(
        tmp_path,
        [
            {
                "attachment": {
                    "type": "hook_non_blocking_error",
                    "hookName": "Stop",
                    "stderr": "JSON validation failed",
                    "stdout": (
                        "The stop condition has **not** been satisfied.\n"
                        '{"ok": false, "reason": "still incomplete"}'
                    ),
                }
            }
        ],
    )

    assert _latest_goal_stop_was_rejected(logs_dir)


def test_ignores_unrelated_stop_hook_error(tmp_path):
    logs_dir = _write_session(
        tmp_path,
        [
            {
                "attachment": {
                    "type": "hook_non_blocking_error",
                    "hookName": "Stop",
                    "stdout": "temporary hook failure",
                }
            }
        ],
    )

    assert not _latest_goal_stop_was_rejected(logs_dir)


def test_uses_latest_session(tmp_path):
    older = _write_session(
        tmp_path,
        [
            {
                "attachment": {
                    "type": "hook_non_blocking_error",
                    "hookName": "Stop",
                    "stdout": '{"ok": false}',
                }
            }
        ],
    )
    newer = older / "sessions" / "projects" / "-other" / "session.jsonl"
    newer.parent.mkdir(parents=True)
    newer.write_text('{"type":"assistant"}\n', encoding="utf-8")

    assert not _latest_goal_stop_was_rejected(tmp_path)
