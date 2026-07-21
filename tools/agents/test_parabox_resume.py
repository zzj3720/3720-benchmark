import asyncio
import json
from pathlib import Path
from types import SimpleNamespace

from tools.agents.parabox_resume import ParaboxResume


class _ResumeFixture(ParaboxResume):
    def __init__(self, state: Path):
        self._configure_parabox_resume(resume_game_state_path=str(state))


def test_resume_without_events_builds_matching_baseline(tmp_path):
    state = tmp_path / "state.txt"
    state.write_text(
        "\n".join(
            [
                "parabox-state-v4",
                "campaign parabox-complete-364-v11",
                "selected b3",
                "solved a1",
                "solved a2",
                "level a1",
            ]
        )
        + "\n"
    )

    record = json.loads(_ResumeFixture(state)._resume_event_baseline())

    assert record["schema"] == "parabox-events-v1"
    assert record["type"] == "sidecar_started"
    assert record["campaign"] == "parabox-complete-364-v11"
    assert record["score"] == 2
    assert record["selected"] == "b3"
    assert record["total"] == 364
    assert isinstance(record["timestamp_ms"], int)


def test_resume_baseline_accepts_selection_required_state(tmp_path):
    state = tmp_path / "state.txt"
    state.write_text(
        "\n".join(
            [
                "parabox-state-v4",
                "campaign parabox-complete-364-v11",
                "solved a1",
            ]
        )
        + "\n"
    )

    record = json.loads(_ResumeFixture(state)._resume_event_baseline())

    assert record["score"] == 1
    assert record["selected"] is None


def test_resume_events_accept_explicit_no_selection_state(tmp_path):
    state = tmp_path / "state.txt"
    state.write_text(
        "\n".join(
            [
                "parabox-state-v4",
                "campaign parabox-complete-364-v11",
                "selected -",
                "solved a1",
            ]
        )
        + "\n"
    )
    events = tmp_path / "events.jsonl"
    events.write_text(
        json.dumps(
            {
                "campaign": "parabox-complete-364-v11",
                "schema": "parabox-events-v1",
                "score": 1,
                "selected": None,
                "timestamp_ms": 10,
                "total": 364,
                "type": "sidecar_started",
            }
        )
        + "\n"
    )
    fixture = _ResumeFixture(state)
    fixture._resume_game_events_path = events

    normalized, provenance = fixture._normalized_resume_events()
    record = json.loads(normalized)

    assert record["score"] == 1
    assert record["selected"] is None
    assert provenance["mode"] == "exact_event_restore"


def test_resume_baseline_rejects_unknown_state_schema(tmp_path):
    state = tmp_path / "state.txt"
    state.write_text("parabox-state-v3\ncampaign old\n")

    try:
        _ResumeFixture(state)._resume_event_baseline()
    except ValueError as error:
        assert "unsupported Parabox state schema" in str(error)
    else:
        raise AssertionError("unknown state schema was accepted")


def test_resume_rebases_legacy_zero_startup_record(tmp_path):
    state = tmp_path / "state.txt"
    state.write_text(
        "\n".join(
            [
                "parabox-state-v4",
                "campaign parabox-complete-364-v11",
                "selected b3",
                "solved a1",
                "solved a2",
            ]
        )
        + "\n"
    )
    events = tmp_path / "events.jsonl"
    events.write_text(
        "\n".join(
            [
                json.dumps(
                    {
                        "campaign": "parabox-complete-364-v11",
                        "schema": "parabox-events-v1",
                        "score": 0,
                        "selected": "a1",
                        "timestamp_ms": 10,
                        "total": 364,
                        "type": "sidecar_started",
                    }
                ),
                json.dumps(
                    {
                        "argument_count": 0,
                        "campaign": "parabox-complete-364-v11",
                        "code": 0,
                        "command": "show",
                        "ok": True,
                        "schema": "parabox-events-v1",
                        "score": 2,
                        "score_before": 2,
                        "score_delta": 0,
                        "selected": "b3",
                        "selected_before": "b3",
                        "solved_levels": [],
                        "timestamp_ms": 20,
                        "total": 364,
                        "type": "request",
                    }
                ),
            ]
        )
        + "\n"
    )
    fixture = _ResumeFixture(state)
    fixture._resume_game_events_path = events

    normalized, provenance = fixture._normalized_resume_events()
    records = [json.loads(line) for line in normalized.splitlines()]

    assert records[0]["score"] == 2
    assert records[0]["selected"] == "b3"
    assert records[1]["score_before"] == 2
    assert provenance["mode"] == "legacy_startup_rebased"
    assert provenance["source_sha256"] != provenance["normalized_sha256"]


def test_resume_rejects_discontinuous_event_stream(tmp_path):
    state = tmp_path / "state.txt"
    state.write_text(
        "\n".join(
            [
                "parabox-state-v4",
                "campaign parabox-complete-364-v11",
                "selected a2",
                "solved a1",
            ]
        )
        + "\n"
    )
    events = tmp_path / "events.jsonl"
    events.write_text(
        "\n".join(
            [
                json.dumps(
                    {
                        "campaign": "parabox-complete-364-v11",
                        "schema": "parabox-events-v1",
                        "score": 0,
                        "selected": "a1",
                        "timestamp_ms": 10,
                        "total": 364,
                        "type": "sidecar_started",
                    }
                ),
                json.dumps(
                    {
                        "argument_count": 1,
                        "campaign": "parabox-complete-364-v11",
                        "code": 0,
                        "command": "move",
                        "ok": True,
                        "schema": "parabox-events-v1",
                        "score": 1,
                        "score_before": 0,
                        "score_delta": 0,
                        "selected": "a2",
                        "selected_before": "a1",
                        "solved_levels": ["a1"],
                        "timestamp_ms": 20,
                        "total": 364,
                        "type": "request",
                    }
                ),
            ]
        )
        + "\n"
    )
    fixture = _ResumeFixture(state)
    fixture._resume_game_events_path = events

    try:
        fixture._normalized_resume_events()
    except ValueError as error:
        assert "discontinuous Parabox resume event" in str(error)
    else:
        raise AssertionError("discontinuous event stream was accepted")


def test_restore_large_event_stream_uses_bounded_exec_chunks(tmp_path):
    state = tmp_path / "state.txt"
    state.write_text(
        "parabox-state-v4\ncampaign parabox-complete-364-v11\n"
    )
    fixture = _ResumeFixture(state)

    class Environment:
        def __init__(self):
            self.commands = []

        async def service_exec(self, command, **_kwargs):
            self.commands.append(command)
            return SimpleNamespace(return_code=0, stderr="")

    environment = Environment()
    asyncio.run(
        fixture._restore_game_bytes(
            environment,
            b"x" * (70 * 1024),
            "/var/lib/parabox/events.jsonl",
        )
    )

    assert len(environment.commands) == 5
    assert max(map(len, environment.commands)) < 45_000
    assert environment.commands[0].startswith("umask 077; : >")
    assert environment.commands[-1].startswith('test "$(wc -c <')
