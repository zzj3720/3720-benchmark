import hashlib
import json
from datetime import datetime, timedelta, timezone

from score_parabox_trial import score, score_chain


def write_result(
    tmp_path,
    *,
    levels=10,
    seconds=600,
    input_tokens=800_000,
    cache_tokens=500_000,
    output_tokens=200_000,
    exception=None,
    suffix="",
    task_checksum="checksum",
):
    started = datetime(2026, 7, 20, tzinfo=timezone.utc)
    result = {
        "id": f"trial-id{suffix}",
        "trial_name": f"trial-name{suffix}",
        "task_name": "3720/parabox-intro",
        "task_checksum": task_checksum,
        "exception_info": exception,
        "config": {
            "agent": {
                "name": "codex",
                "model_name": "openai/gpt-5.6-terra",
                "kwargs": {"reasoning_effort": "high"},
            }
        },
        "agent_execution": {
            "started_at": started.isoformat(),
            "finished_at": (started + timedelta(seconds=seconds)).isoformat(),
        },
        "agent_result": {
            "n_input_tokens": input_tokens,
            "n_cache_tokens": cache_tokens,
            "n_output_tokens": output_tokens,
        },
        "verifier_result": {"rewards": {"reward": float(levels)}},
    }
    path = tmp_path / f"result{suffix}.json"
    path.write_text(json.dumps(result))
    return path


def write_rollout(result_path, events):
    rollout = result_path.parent / "agent" / "sessions" / "rollout-test.jsonl"
    rollout.parent.mkdir(parents=True, exist_ok=True)
    rollout.write_text("\n".join(json.dumps(event) for event in events) + "\n")
    return rollout


def write_events(result_path, records):
    events = (
        result_path.parent
        / "artifacts"
        / "var"
        / "lib"
        / "parabox"
        / "parabox-events.jsonl"
    )
    events.parent.mkdir(parents=True, exist_ok=True)
    events.write_text("\n".join(json.dumps(record) for record in records) + "\n")
    return events


def file_sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write_recovery(result_path, levels):
    root = result_path.parent / "recovery"
    verifier = root / "verifier"
    verifier.mkdir(parents=True)
    state = root / "parabox-state.txt"
    state.write_text(
        "parabox-state-v4\ncampaign parabox-complete-364-v11\n"
        + "".join(f"solved a{index}\n" for index in range(1, levels + 1))
    )
    audit = root / "parabox-audit.tsv"
    audit.write_text("parabox-audit-v1\n")
    events = write_events(
        result_path,
        [
            {
                "schema": "parabox-events-v1",
                "type": "sidecar_started",
                "timestamp_ms": 0,
                "score": levels,
                "selected": None,
            }
        ],
    )
    recovered_events = root / "parabox-events.jsonl"
    recovered_events.write_bytes(events.read_bytes())
    source_manifest = root / "verifier-source-manifest.json"
    source_manifest.write_text("[]\n")
    stdout = root / "verifier-stdout.txt"
    stdout.write_text(f"score: {levels}\n")
    stderr = root / "verifier-stderr.txt"
    stderr.write_text("")
    ctrf = verifier / "ctrf.json"
    ctrf.write_text("{}\n")
    result = json.loads(result_path.read_text())
    recovery = {
        "schema": "parabox-verifier-recovery-v1",
        "status": "passed",
        "source": {
            "result_sha256": file_sha256(result_path),
            "task_checksum": result["task_checksum"],
            "original_verifier_reward": 0.0,
            "state_sha256": file_sha256(state),
            "audit_sha256": file_sha256(audit),
        },
        "verification": {
            "reward": levels,
            "state_score": levels,
            "normalized_events_sha256": file_sha256(recovered_events),
            "source_manifest_sha256": file_sha256(source_manifest),
            "stdout_sha256": file_sha256(stdout),
            "stderr_sha256": file_sha256(stderr),
            "ctrf_sha256": file_sha256(ctrf),
        },
    }
    attestation = root / "attestation.json"
    attestation.write_text(json.dumps(recovery))
    return attestation


def test_score_does_not_double_count_cache_and_keeps_time_diagnostic(tmp_path):
    result = score(write_result(tmp_path))

    assert result["score_valid"]
    assert result["ranking_eligible"]
    assert result["eligible"]
    assert result["raw"]["total_tokens"] == 1_000_000
    assert result["raw"]["diagnostic_agent_seconds"] == 600
    assert result["derived"]["score"] == 10
    assert result["derived"]["token_tiebreak"] == 1_000_000
    assert result["derived"]["ascending_rank_key"] == [-10, 1_000_000]


def test_an_extra_level_always_beats_any_token_advantage(tmp_path):
    lower = score(
        write_result(
            tmp_path,
            levels=10,
            input_tokens=0,
            cache_tokens=0,
            output_tokens=0,
        )
    )
    higher = score(
        write_result(
            tmp_path,
            levels=11,
            input_tokens=10**12,
            cache_tokens=0,
            output_tokens=0,
        )
    )

    assert (
        higher["derived"]["ascending_rank_key"] < lower["derived"]["ascending_rank_key"]
    )


def test_fewer_tokens_breaks_an_equal_score_tie(tmp_path):
    efficient = score(
        write_result(
            tmp_path,
            levels=10,
            input_tokens=100,
            cache_tokens=0,
            output_tokens=100,
        )
    )
    expensive = score(
        write_result(
            tmp_path,
            levels=10,
            input_tokens=1_000,
            cache_tokens=0,
            output_tokens=1_000,
        )
    )

    assert efficient["derived"]["score"] == expensive["derived"]["score"] == 10
    assert (
        efficient["derived"]["ascending_rank_key"]
        < expensive["derived"]["ascending_rank_key"]
    )


def test_zero_progress_has_zero_integer_score(tmp_path):
    result = score(
        write_result(
            tmp_path,
            levels=0,
            input_tokens=0,
            cache_tokens=0,
            output_tokens=0,
        )
    )

    assert result["derived"]["score"] == 0


def test_cache_accounting_is_optional_because_it_is_not_added(tmp_path):
    result = score(write_result(tmp_path, cache_tokens=None))

    assert result["score_valid"]
    assert result["ranking_eligible"]
    assert result["eligible"]
    assert result["raw"]["cache_tokens"] is None
    assert result["raw"]["total_tokens"] == 1_000_000


def test_missing_tokens_keeps_score_but_cannot_break_ties(tmp_path):
    missing = score(write_result(tmp_path, input_tokens=None))

    assert missing["score_valid"]
    assert not missing["ranking_eligible"]
    assert not missing["eligible"]
    assert "input_tokens" in missing["ineligible_reason"]
    assert missing["raw"]["solved_levels"] == 10
    assert missing["raw"]["total_tokens"] is None
    assert missing["derived"]["score"] == 10
    assert missing["derived"]["token_tiebreak"] is None
    assert missing["derived"]["ascending_rank_key"] is None


def test_infrastructure_error_invalidates_score(tmp_path):
    failed = score(
        write_result(
            tmp_path,
            exception={"exception_type": "AgentError"},
        )
    )

    assert not failed["score_valid"]
    assert not failed["ranking_eligible"]
    assert not failed["eligible"]
    assert "infrastructure" in failed["ineligible_reason"]
    assert failed["termination"] == {
        "status": "error",
        "exception_type": "AgentError",
    }
    assert failed["raw"] is None
    assert failed["derived"] is None


def test_agent_timeout_is_an_expected_cutoff_with_a_verified_score(tmp_path):
    cutoff = score(
        write_result(
            tmp_path,
            levels=16,
            exception={
                "exception_type": "AgentTimeoutError",
                "exception_message": "Agent execution timed out after 7200.0 seconds",
            },
        )
    )

    assert cutoff["score_valid"]
    assert cutoff["ranking_eligible"]
    assert cutoff["eligible"]
    assert cutoff["termination"] == {
        "status": "expected_agent_timeout",
        "exception_type": "AgentTimeoutError",
    }
    assert cutoff["derived"]["score"] == 16


def test_verified_recovery_replaces_only_matching_broken_verifier_result(tmp_path):
    path = write_result(tmp_path, levels=0)
    recovery = write_recovery(path, 3)

    recovered = score(path, [recovery])

    assert recovered["score_valid"]
    assert recovered["raw"]["verified_final_solved_levels"] == 3
    assert recovered["derived"]["score"] == 3
    assert recovered["source"]["verifier_recovery"]["attestation"] == str(
        recovery
    )


def test_sidecar_excludes_scores_recorded_after_agent_cutoff(tmp_path):
    path = write_result(
        tmp_path,
        levels=3,
        seconds=60,
        exception={
            "exception_type": "AgentTimeoutError",
            "exception_message": "Agent execution timed out after 60.0 seconds",
        },
    )
    result = json.loads(path.read_text())
    result["agent_execution"]["finished_at"] = (
        datetime(2026, 7, 20, tzinfo=timezone.utc) + timedelta(seconds=75)
    ).isoformat()
    path.write_text(json.dumps(result))
    started_ms = int(datetime(2026, 7, 20, tzinfo=timezone.utc).timestamp() * 1_000)
    write_events(
        path,
        [
            {
                "schema": "parabox-events-v1",
                "type": "sidecar_started",
                "timestamp_ms": started_ms - 1_000,
                "score": 0,
                "selected": "a1",
            },
            {
                "schema": "parabox-events-v1",
                "type": "request",
                "timestamp_ms": started_ms + 50_000,
                "score": 2,
                "selected": "a3",
                "command": "move",
            },
            {
                "schema": "parabox-events-v1",
                "type": "request",
                "timestamp_ms": started_ms + 65_000,
                "score": 3,
                "selected": "a4",
                "command": "move",
            },
        ],
    )

    cutoff = score(path)

    assert cutoff["score_valid"]
    assert cutoff["raw"]["verified_final_solved_levels"] == 3
    assert cutoff["raw"]["solved_levels"] == 2
    assert cutoff["raw"]["sidecar_cutoff"]["post_agent_score_delta"] == 1
    assert cutoff["derived"]["score"] == 2


def test_sidecar_final_score_must_match_isolated_verifier(tmp_path):
    path = write_result(tmp_path, levels=3)
    started_ms = int(datetime(2026, 7, 20, tzinfo=timezone.utc).timestamp() * 1_000)
    write_events(
        path,
        [
            {
                "schema": "parabox-events-v1",
                "type": "sidecar_started",
                "timestamp_ms": started_ms,
                "score": 2,
                "selected": "a3",
            }
        ],
    )

    invalid = score(path)

    assert not invalid["score_valid"]
    assert "does not match" in invalid["ineligible_reason"]


def test_continuation_chain_uses_final_score_and_sums_tokens(tmp_path):
    chain = score_chain(
        [
            write_result(
                tmp_path,
                suffix="-1",
                levels=15,
                input_tokens=100,
                cache_tokens=50,
                output_tokens=10,
            ),
            write_result(
                tmp_path,
                suffix="-2",
                levels=20,
                input_tokens=200,
                cache_tokens=150,
                output_tokens=20,
                exception={"exception_type": "AgentTimeoutError"},
            ),
        ]
    )

    assert chain["score_valid"]
    assert chain["raw"]["segment_scores"] == [15, 20]
    assert chain["raw"]["total_tokens"] == 330
    assert chain["derived"]["score"] == 20
    assert chain["derived"]["ascending_rank_key"] == [-20, 330]
    assert chain["termination"]["segments"][-1]["status"] == "expected_agent_timeout"


def test_continuation_chain_rejects_score_regression(tmp_path):
    chain = score_chain(
        [
            write_result(tmp_path, suffix="-1", levels=20),
            write_result(tmp_path, suffix="-2", levels=19),
        ]
    )

    assert not chain["score_valid"]
    assert "monotonically" in chain["ineligible_reason"]


def test_continuation_chain_rejects_identity_mismatch(tmp_path):
    chain = score_chain(
        [
            write_result(tmp_path, suffix="-1", levels=15),
            write_result(
                tmp_path,
                suffix="-2",
                levels=20,
                task_checksum="different",
            ),
        ]
    )

    assert not chain["score_valid"]
    assert "task_checksum" in chain["ineligible_reason"]


def test_continuation_chain_uses_exact_rollout_runtime_cutoff(tmp_path):
    first = write_result(tmp_path, suffix="-1", levels=10, seconds=6_000)
    second = write_result(tmp_path, suffix="-2", levels=15, seconds=2_000)
    started = datetime(2026, 7, 20, tzinfo=timezone.utc)
    write_rollout(
        second,
        [
            {
                "timestamp": (started + timedelta(seconds=1_000)).isoformat(),
                "type": "response_item",
                "payload": {
                    "type": "custom_tool_call_output",
                    "output": [{"type": "input_text", "text": '{"score":14}'}],
                },
            },
            {
                "timestamp": (started + timedelta(seconds=1_100)).isoformat(),
                "type": "event_msg",
                "payload": {
                    "type": "token_count",
                    "info": {
                        "total_token_usage": {
                            "input_tokens": 900,
                            "cached_input_tokens": 700,
                            "output_tokens": 100,
                        }
                    },
                },
            },
            {
                "timestamp": (started + timedelta(seconds=1_300)).isoformat(),
                "type": "response_item",
                "payload": {
                    "type": "custom_tool_call_output",
                    "output": [{"type": "input_text", "text": '{"score":15}'}],
                },
            },
        ],
    )

    chain = score_chain([first, second], runtime_budget_seconds=7_200)

    assert chain["score_valid"]
    assert chain["termination"]["status"] == "runtime_budget_cutoff"
    assert chain["raw"]["segment_scores"] == [10, 15]
    assert chain["raw"]["runtime_cutoff"]["segment_index"] == 1
    assert chain["raw"]["runtime_cutoff"]["seconds_into_segment"] == 1_200
    assert chain["derived"]["score"] == 14
    assert chain["derived"]["token_tiebreak"] == 1_000
    assert chain["derived"]["ascending_rank_key"] == [-14, 1_000]
