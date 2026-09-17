import asyncio
import json
from pathlib import Path

from harbor.models.agent.context import AgentContext

from tools.agents.pi_goal import (
    EmergencyOperatorGoalPi,
    _campaign_completion_evidence,
    _last_pi_stop,
    _operator_completion_evidence,
    _sokoban_completion_evidence,
)


def _tool_result(status: str, *, complete: bool = False) -> dict:
    response = {
        "api_version": "emergency-operator-api-v1",
        "command": "submit",
        "data": {
            "complete": complete,
            "state": {"shift": {"status": status}},
        },
    }
    return {
        "type": "message",
        "message": {
            "role": "toolResult",
            "content": [{"type": "text", "text": json.dumps(response)}],
        },
    }


def _sokoban_tool_result(score: int, *, complete: bool) -> dict:
    response = {
        "api_version": "sokoban-api-v1",
        "ok": True,
        "command": "submit",
        "data": {
            "score": score,
            "max_score": 305,
            "complete": complete,
            "state": {
                "campaign": {
                    "score": score,
                    "max_score": 305,
                    "complete": complete,
                }
            },
        },
    }
    return {
        "type": "message",
        "message": {
            "role": "toolResult",
            "content": [{"type": "text", "text": json.dumps(response)}],
        },
    }


def _minesweeper_tool_result(score: int, *, complete: bool) -> dict:
    result = _sokoban_tool_result(score, complete=complete)
    response = json.loads(result["message"]["content"][0]["text"])
    response["api_version"] = "minesweeper-api-v2"
    response["data"]["max_score"] = 5
    response["data"]["state"]["campaign"]["max_score"] = 5
    result["message"]["content"][0]["text"] = json.dumps(response)
    return result


def _kitchen_tool_result(score: int, *, complete: bool) -> dict:
    response = {
        "api_version": "overcooked-api-v4",
        "ok": True,
        "command": "submit",
        "data": {
            "score": score,
            "complete": complete,
            "state": {
                "campaign": {"score": score},
                "shift": {"status": "complete" if complete else "running"},
            },
        },
    }
    return {
        "type": "message",
        "message": {
            "role": "toolResult",
            "content": [{"type": "text", "text": json.dumps(response)}],
        },
    }


def _write_session(logs_dir: Path, events: list[dict]) -> None:
    session = logs_dir / "pi" / "sessions" / "session.jsonl"
    session.parent.mkdir(parents=True, exist_ok=True)
    session.write_text(
        "\n".join(json.dumps(event) for event in events) + "\n",
        encoding="utf-8",
    )


def _assistant_stop(
    reason: str, error: str | None = None, *, event_id: str = "assistant"
) -> dict:
    return {
        "id": event_id,
        "type": "message",
        "message": {
            "role": "assistant",
            "content": [{"type": "text", "text": "done"}],
            "stopReason": reason,
            "errorMessage": error,
        },
    }


def test_completion_requires_authoritative_terminal_state(tmp_path):
    _write_session(tmp_path, [_tool_result("running", complete=False)])
    assert _operator_completion_evidence(tmp_path) is None

    _write_session(tmp_path, [_tool_result("complete", complete=True)])
    assert _operator_completion_evidence(tmp_path) == {
        "command": "submit",
        "complete": True,
        "shift_status": "complete",
        "source": "api_json",
    }


def test_sokoban_completion_requires_authoritative_full_campaign(tmp_path):
    _write_session(tmp_path, [_sokoban_tool_result(304, complete=False)])
    assert _sokoban_completion_evidence(tmp_path) is None

    _write_session(tmp_path, [_sokoban_tool_result(305, complete=True)])
    assert _sokoban_completion_evidence(tmp_path) == {
        "command": "submit",
        "complete": True,
        "score": 305,
        "max_score": 305,
        "source": "api_json",
    }


def test_minesweeper_completion_accepts_a_terminal_partial_score(tmp_path):
    _write_session(tmp_path, [_minesweeper_tool_result(2, complete=False)])
    assert (
        _campaign_completion_evidence(tmp_path, "minesweeper-api-v2", 5) is None
    )

    _write_session(tmp_path, [_minesweeper_tool_result(2, complete=True)])
    assert _campaign_completion_evidence(tmp_path, "minesweeper-api-v2", 5) == {
        "command": "submit",
        "complete": True,
        "score": 2,
        "max_score": 5,
        "source": "api_json",
    }


def test_kitchen_completion_uses_the_configured_score_ceiling(tmp_path):
    _write_session(tmp_path, [_kitchen_tool_result(24, complete=False)])
    assert _campaign_completion_evidence(tmp_path, "overcooked-api-v4", 260) is None

    _write_session(tmp_path, [_kitchen_tool_result(-6, complete=True)])
    assert _campaign_completion_evidence(tmp_path, "overcooked-api-v4", 260) == {
        "command": "submit",
        "complete": True,
        "score": -6,
        "max_score": 260,
        "source": "api_json",
    }


def test_stop_reason_must_come_from_the_current_segment(tmp_path):
    prior = _assistant_stop("stop")
    prior["id"] = "previous-segment"
    _write_session(tmp_path, [prior])

    assert _last_pi_stop(tmp_path)["reason"] == "stop"
    assert _last_pi_stop(tmp_path, after_id="previous-segment") == {
        "id": "previous-segment",
        "reason": None,
        "error": None,
    }


class _FakeGoalPi(EmergencyOperatorGoalPi):
    def __init__(self, logs_dir: Path):
        self.logs_dir = logs_dir
        self._goal_objective = "Play until the shift is complete."
        self._premature_finals = 0
        self._interrupted_segments = 0
        self._completion_evidence = None
        self._resume = False
        self.calls: list[tuple[str, bool]] = []

    async def _run_pi_segment(self, instruction, environment, context):
        self.calls.append((instruction, self._resume))
        complete = len(self.calls) == 2
        _write_session(
            self.logs_dir,
            [
                _tool_result("complete" if complete else "running", complete=complete),
                _assistant_stop("stop", event_id=f"assistant-{len(self.calls)}"),
            ],
        )
        usage = {
            "input": 10,
            "output": 2,
            "cacheRead": 20,
            "cacheWrite": 0,
            "cost": {"total": 0.01},
        }
        (self.logs_dir / self._OUTPUT_FILENAME).write_text(
            json.dumps(
                {
                    "type": "message_end",
                    "message": {"role": "assistant", "usage": usage},
                }
            )
            + "\n",
            encoding="utf-8",
        )


def test_premature_final_continues_same_session_and_remains_visible(tmp_path):
    agent = _FakeGoalPi(tmp_path)
    context = AgentContext()

    asyncio.run(agent.run("Task specification.", object(), context))
    agent.populate_context_post_run(context)

    assert len(agent.calls) == 2
    assert agent.calls[0][1] is False
    assert agent.calls[1][1] is True
    assert "premature stop has been recorded" in agent.calls[1][0]
    assert context.n_input_tokens == 60
    assert context.n_cache_tokens == 40
    assert context.n_output_tokens == 4
    assert context.cost_usd == 0.02
    assert context.metadata == {
        "goal_pi": {
            "completed": True,
            "premature_finals": 1,
            "interrupted_segments": 0,
            "completion_evidence": {
                "command": "submit",
                "complete": True,
                "shift_status": "complete",
                "source": "api_json",
            },
        }
    }

    records = [
        json.loads(line)
        for line in (tmp_path / agent._EVENTS_FILENAME).read_text().splitlines()
    ]
    assert [record["kind"] for record in records] == [
        "premature_final",
        "goal_complete",
    ]


def test_interrupted_segment_is_not_counted_as_a_premature_final(tmp_path):
    class InterruptedThenComplete(_FakeGoalPi):
        async def _run_pi_segment(self, instruction, environment, context):
            self.calls.append((instruction, self._resume))
            complete = len(self.calls) == 2
            _write_session(
                self.logs_dir,
                [
                    _tool_result(
                        "complete" if complete else "running", complete=complete
                    ),
                    _assistant_stop(
                        "stop" if complete else "error",
                        None if complete else "provider context limit",
                        event_id=f"assistant-{len(self.calls)}",
                    ),
                ],
            )

    agent = InterruptedThenComplete(tmp_path)
    context = AgentContext()
    asyncio.run(agent.run("Task specification.", object(), context))
    agent.populate_context_post_run(context)

    assert agent._premature_finals == 0
    assert agent._interrupted_segments == 1
    assert "segment was interrupted" in agent.calls[1][0]
    records = [
        json.loads(line)
        for line in (tmp_path / agent._EVENTS_FILENAME).read_text().splitlines()
    ]
    assert [record["kind"] for record in records] == [
        "segment_interrupted",
        "goal_complete",
    ]


def test_completion_accepts_successful_concise_operator_projection(tmp_path):
    _write_session(
        tmp_path,
        [
            {
                "type": "message",
                "message": {
                    "role": "toolResult",
                    "toolName": "bash",
                    "content": [
                        {
                            "type": "text",
                            "text": (
                                "Shift status: complete\nScore: 9588\nRemaining ms: 0\n"
                            ),
                        }
                    ],
                    "isError": False,
                },
            }
        ],
    )
    assert _operator_completion_evidence(tmp_path) == {
        "command": "show",
        "complete": False,
        "shift_status": "complete",
        "source": "tool_projection",
    }


def test_completion_rejects_failed_or_nonterminal_projection(tmp_path):
    event = {
        "type": "message",
        "message": {
            "role": "toolResult",
            "toolName": "bash",
            "content": [{"type": "text", "text": "Shift status: complete\n"}],
            "isError": True,
        },
    }
    _write_session(tmp_path, [event])
    assert _operator_completion_evidence(tmp_path) is None

    event["message"]["isError"] = False
    event["message"]["content"][0]["text"] = "Complete: False\n"
    _write_session(tmp_path, [event])
    assert _operator_completion_evidence(tmp_path) is None
