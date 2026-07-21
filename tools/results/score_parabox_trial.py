#!/usr/bin/env python3
"""Compute a traceable integer Parabox score with a token-use tie-break."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
from datetime import datetime, timedelta
from pathlib import Path
from typing import Any


SCORING_VERSION = "parabox-integer-score-token-tiebreak-v5"
CHAIN_SCORING_VERSION = "parabox-continuation-chain-v2"
LEVEL_COUNT = 364
EXPECTED_CUTOFF_EXCEPTIONS = {"AgentTimeoutError"}
RECOVERY_SCHEMA = "parabox-verifier-recovery-v1"
FORMULA = (
    "score = solved_levels at agent_execution.finished_at; rank by score "
    "descending, then total_tokens ascending"
)
SCORE_PATTERN = re.compile(r'"score":\s*(\d+)')
TIMEOUT_PATTERN = re.compile(r"timed out after ([0-9]+(?:\.[0-9]+)?) seconds")


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def integer(value: Any, field: str) -> int:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise ValueError(f"{field} is missing")
    if value < 0 or int(value) != value:
        raise ValueError(f"{field} must be a non-negative integer")
    return int(value)


def agent_seconds(result: dict[str, Any]) -> float | None:
    try:
        started = datetime.fromisoformat(
            result["agent_execution"]["started_at"].replace("Z", "+00:00")
        )
        finished = datetime.fromisoformat(
            result["agent_execution"]["finished_at"].replace("Z", "+00:00")
        )
        seconds = (finished - started).total_seconds()
        return seconds if seconds >= 0 else None
    except (AttributeError, KeyError, TypeError, ValueError):
        return None


def parse_time(value: str) -> datetime:
    return datetime.fromisoformat(value.replace("Z", "+00:00"))


def agent_cutoff(result: dict[str, Any]) -> datetime:
    started = parse_time(result["agent_execution"]["started_at"])
    finished = parse_time(result["agent_execution"]["finished_at"])
    exception = result.get("exception_info") or {}
    if exception.get("exception_type") == "AgentTimeoutError":
        match = TIMEOUT_PATTERN.search(exception.get("exception_message", ""))
        if match:
            return min(finished, started + timedelta(seconds=float(match.group(1))))
    return finished


def text_values(value: Any):
    if isinstance(value, str):
        yield value
    elif isinstance(value, list):
        for item in value:
            yield from text_values(item)
    elif isinstance(value, dict):
        for item in value.values():
            yield from text_values(item)


def rollout_cutoff_evidence(path: Path, cutoff: datetime) -> dict[str, Any]:
    """Read exact score and cumulative token evidence at an absolute cutoff."""

    sessions_dir = path.parent / "agent" / "sessions"
    rollouts = sorted(sessions_dir.rglob("rollout-*.jsonl"))
    if not rollouts:
        raise ValueError(f"no raw rollout found for runtime cutoff: {path}")

    latest_score = None
    latest_tokens = None
    for rollout in rollouts:
        for line_number, line in enumerate(
            rollout.read_text(encoding="utf-8").splitlines(), 1
        ):
            try:
                event = json.loads(line)
                event_time = parse_time(event["timestamp"])
            except (json.JSONDecodeError, KeyError, TypeError, ValueError):
                continue
            if event_time > cutoff:
                continue
            payload = event.get("payload")
            if not isinstance(payload, dict):
                continue
            if (
                event.get("type") == "response_item"
                and payload.get("type") == "custom_tool_call_output"
            ):
                scores = [
                    int(match.group(1))
                    for text in text_values(payload.get("output"))
                    for match in SCORE_PATTERN.finditer(text)
                ]
                if scores:
                    latest_score = {
                        "score": scores[-1],
                        "timestamp": event["timestamp"],
                        "rollout_jsonl": str(rollout),
                        "rollout_sha256": sha256(rollout),
                        "line": line_number,
                    }
            if (
                event.get("type") == "event_msg"
                and payload.get("type") == "token_count"
            ):
                usage = payload.get("info", {}).get("total_token_usage")
                if isinstance(usage, dict):
                    latest_tokens = {
                        "input_tokens": integer(
                            usage.get("input_tokens"), "input_tokens"
                        ),
                        "cache_tokens": integer(
                            usage.get("cached_input_tokens"), "cache_tokens"
                        ),
                        "output_tokens": integer(
                            usage.get("output_tokens"), "output_tokens"
                        ),
                        "timestamp": event["timestamp"],
                        "rollout_jsonl": str(rollout),
                        "rollout_sha256": sha256(rollout),
                        "line": line_number,
                    }
    if latest_score is None:
        raise ValueError("raw rollout has no score evidence at runtime cutoff")
    return {
        "cutoff_timestamp": cutoff.isoformat(),
        "score": latest_score,
        "tokens": latest_tokens,
    }


def sidecar_cutoff_evidence(
    path: Path,
    result: dict[str, Any],
    verified_final_score: int,
    event_path: Path | None = None,
) -> dict[str, Any] | None:
    event_path = event_path or (
        path.parent / "artifacts" / "var" / "lib" / "parabox" / "parabox-events.jsonl"
    )
    if not event_path.is_file():
        return None
    cutoff = agent_cutoff(result)
    cutoff_ms = int(cutoff.timestamp() * 1_000)
    latest_at_cutoff = None
    latest_record = None
    previous_timestamp = None
    previous_score = None
    for line_number, line in enumerate(event_path.read_text().splitlines(), 1):
        try:
            record = json.loads(line)
            timestamp = integer(record["timestamp_ms"], "timestamp_ms")
            score = integer(record["score"], "score")
        except (json.JSONDecodeError, KeyError, TypeError, ValueError) as error:
            raise ValueError(
                f"invalid sidecar event line {line_number}: {error}"
            ) from error
        if record.get("schema") != "parabox-events-v1":
            raise ValueError(f"invalid sidecar event schema at line {line_number}")
        if previous_timestamp is not None and timestamp < previous_timestamp:
            raise ValueError("sidecar event timestamps are not monotonic")
        if previous_score is not None and score < previous_score:
            raise ValueError("sidecar event scores are not monotonic")
        latest_record = {
            "line": line_number,
            "timestamp_ms": timestamp,
            "score": score,
            "selected": record.get("selected"),
            "type": record.get("type"),
            "command": record.get("command"),
        }
        if timestamp <= cutoff_ms:
            latest_at_cutoff = latest_record
        previous_timestamp = timestamp
        previous_score = score
    if latest_record is None:
        raise ValueError("sidecar event log is empty")
    if latest_record["score"] != verified_final_score:
        raise ValueError(
            "sidecar final score does not match the isolated verifier result"
        )
    if latest_at_cutoff is None:
        raise ValueError("sidecar has no score evidence at the agent cutoff")
    return {
        "path": str(event_path),
        "sha256": sha256(event_path),
        "cutoff_timestamp": cutoff.isoformat(),
        "cutoff_timestamp_ms": cutoff_ms,
        "score_at_cutoff": latest_at_cutoff,
        "final_record": latest_record,
        "post_agent_score_delta": latest_record["score"] - latest_at_cutoff["score"],
    }


def termination(result: dict[str, Any]) -> dict[str, Any]:
    exception = result.get("exception_info")
    if exception is None:
        return {"status": "completed", "exception_type": None}
    exception_type = exception.get("exception_type")
    return {
        "status": (
            "expected_agent_timeout"
            if exception_type in EXPECTED_CUTOFF_EXCEPTIONS
            else "error"
        ),
        "exception_type": exception_type,
    }


def recovery_evidence(
    path: Path,
    result: dict[str, Any],
    recovery_paths: list[Path],
) -> tuple[int, Path, dict[str, Any]] | None:
    result_hash = sha256(path)
    for recovery_path in recovery_paths:
        recovery = json.loads(recovery_path.read_text())
        if recovery.get("source", {}).get("result_sha256") != result_hash:
            continue
        if (
            recovery.get("schema") != RECOVERY_SCHEMA
            or recovery.get("status") != "passed"
            or recovery["source"].get("task_checksum") != result.get("task_checksum")
            or recovery["source"].get("original_verifier_reward")
            != result["verifier_result"]["rewards"]["reward"]
        ):
            raise ValueError("invalid Parabox verifier recovery attestation")
        root = recovery_path.parent
        checks = {
            "state_sha256": root / "parabox-state.txt",
            "audit_sha256": root / "parabox-audit.tsv",
            "normalized_events_sha256": root / "parabox-events.jsonl",
            "source_manifest_sha256": root / "verifier-source-manifest.json",
            "stdout_sha256": root / "verifier-stdout.txt",
            "stderr_sha256": root / "verifier-stderr.txt",
            "ctrf_sha256": root / "verifier" / "ctrf.json",
        }
        for field, artifact in checks.items():
            expected = (
                recovery["source"].get(field)
                or recovery["verification"].get(field)
            )
            if not artifact.is_file() or expected != sha256(artifact):
                raise ValueError(
                    f"Parabox verifier recovery artifact mismatch: {field}"
                )
        verified = integer(recovery["verification"].get("reward"), "recovery reward")
        if verified != integer(
            recovery["verification"].get("state_score"), "recovery state_score"
        ):
            raise ValueError("Parabox verifier recovery score mismatch")
        return verified, root / "parabox-events.jsonl", {
            "attestation": str(recovery_path),
            "attestation_sha256": sha256(recovery_path),
            "schema": RECOVERY_SCHEMA,
        }
    return None


def score(path: Path, recovery_paths: list[Path] | None = None) -> dict[str, Any]:
    recovery_paths = recovery_paths or []
    result = json.loads(path.read_text())
    source = {
        "result_json": str(path),
        "result_sha256": sha256(path),
        "field_paths": {
            "solved_levels": "verifier_result.rewards.reward",
            "input_tokens": "agent_result.n_input_tokens",
            "cache_tokens": "agent_result.n_cache_tokens",
            "output_tokens": "agent_result.n_output_tokens",
            "termination": "exception_info.exception_type",
            "diagnostic_agent_started_at": "agent_execution.started_at",
            "diagnostic_agent_finished_at": "agent_execution.finished_at",
        },
    }
    agent = result.get("config", {}).get("agent", {})
    identity = {
        "harbor_trial_id": result.get("id"),
        "trial_name": result.get("trial_name"),
        "task_name": result.get("task_name"),
        "task_checksum": result.get("task_checksum"),
        "agent": agent.get("name"),
        "model": agent.get("model_name"),
        "reasoning_effort": agent.get("kwargs", {}).get("reasoning_effort"),
    }
    trial_termination = termination(result)

    try:
        if trial_termination["status"] == "error":
            raise ValueError("trial has an infrastructure exception")
        recovery = recovery_evidence(path, result, recovery_paths)
        if recovery is None:
            verified_final_score = integer(
                result["verifier_result"]["rewards"]["reward"], "solved_levels"
            )
            recovered_event_path = None
            recovery_source = None
        else:
            verified_final_score, recovered_event_path, recovery_source = recovery
            source["verifier_recovery"] = recovery_source
        if verified_final_score > LEVEL_COUNT:
            raise ValueError(f"solved_levels exceeds {LEVEL_COUNT}")
        cutoff_evidence = sidecar_cutoff_evidence(
            path, result, verified_final_score, recovered_event_path
        )
        solved = (
            cutoff_evidence["score_at_cutoff"]["score"]
            if cutoff_evidence is not None
            else verified_final_score
        )
    except (KeyError, TypeError, ValueError) as error:
        return {
            "source": source,
            "identity": identity,
            "score_valid": False,
            "ranking_eligible": False,
            "eligible": False,
            "ineligible_reason": str(error),
            "termination": trial_termination,
            "raw": None,
            "derived": None,
        }

    try:
        input_tokens = integer(result["agent_result"]["n_input_tokens"], "input_tokens")
        raw_cache_tokens = result["agent_result"].get("n_cache_tokens")
        cache_tokens = (
            None
            if raw_cache_tokens is None
            else integer(raw_cache_tokens, "cache_tokens")
        )
        output_tokens = integer(
            result["agent_result"]["n_output_tokens"], "output_tokens"
        )
        if cache_tokens is not None and cache_tokens > input_tokens:
            raise ValueError("cache_tokens exceeds input_tokens")
    except (KeyError, TypeError, ValueError) as error:
        return {
            "source": source,
            "identity": identity,
            "score_valid": True,
            "ranking_eligible": False,
            "eligible": False,
            "ineligible_reason": f"token ranking unavailable: {error}",
            "termination": trial_termination,
            "raw": {
                "solved_levels": solved,
                "verified_final_solved_levels": verified_final_score,
                "sidecar_cutoff": cutoff_evidence,
                "input_tokens": None,
                "cache_tokens": None,
                "output_tokens": None,
                "total_tokens": None,
                "diagnostic_agent_seconds": agent_seconds(result),
                "token_accounting": "unavailable",
            },
            "derived": {
                "score": solved,
                "token_tiebreak": None,
                "ascending_rank_key": None,
            },
        }

    total_tokens = input_tokens + output_tokens

    return {
        "source": source,
        "identity": identity,
        "score_valid": True,
        "ranking_eligible": True,
        "eligible": True,
        "ineligible_reason": None,
        "termination": trial_termination,
        "raw": {
            "solved_levels": solved,
            "verified_final_solved_levels": verified_final_score,
            "sidecar_cutoff": cutoff_evidence,
            "input_tokens": input_tokens,
            "cache_tokens": cache_tokens,
            "output_tokens": output_tokens,
            "total_tokens": total_tokens,
            "diagnostic_agent_seconds": agent_seconds(result),
            "token_accounting": (
                "input_tokens + output_tokens; cache_tokens is a subset of "
                "input_tokens and is not added again"
            ),
        },
        "derived": {
            "score": solved,
            "token_tiebreak": total_tokens,
            "ascending_rank_key": [-solved, total_tokens],
        },
    }


def score_chain(
    paths: list[Path],
    runtime_budget_seconds: float | None = None,
    recovery_paths: list[Path] | None = None,
) -> dict[str, Any]:
    """Aggregate ordered continuation segments without double-counting score."""

    if len(paths) < 2:
        raise ValueError("a continuation chain requires at least two results")
    segments = [score(path, recovery_paths) for path in paths]
    identity_fields = ("task_name", "task_checksum", "model", "reasoning_effort")
    first_identity = segments[0]["identity"]
    invalid_reason = next(
        (
            f"segment {index + 1}: {segment['ineligible_reason']}"
            for index, segment in enumerate(segments)
            if not segment["score_valid"]
        ),
        None,
    )
    if invalid_reason is None:
        for index, segment in enumerate(segments[1:], 2):
            mismatches = [
                field
                for field in identity_fields
                if segment["identity"][field] != first_identity[field]
            ]
            if mismatches:
                invalid_reason = (
                    f"segment {index} identity mismatch: {', '.join(mismatches)}"
                )
                break
    solved = [
        segment["derived"]["score"] for segment in segments if segment["score_valid"]
    ]
    if invalid_reason is None and solved != sorted(solved):
        invalid_reason = "continuation scores are not monotonically non-decreasing"
    cutoff = None
    if invalid_reason is None and runtime_budget_seconds is not None:
        if runtime_budget_seconds <= 0:
            invalid_reason = "runtime budget must be positive"
        else:
            elapsed = 0.0
            for index, path in enumerate(paths):
                result = json.loads(path.read_text())
                seconds = agent_seconds(result)
                if seconds is None:
                    invalid_reason = (
                        f"segment {index + 1}: agent execution duration is missing"
                    )
                    break
                if elapsed + seconds >= runtime_budget_seconds:
                    remaining = runtime_budget_seconds - elapsed
                    started = parse_time(result["agent_execution"]["started_at"])
                    try:
                        evidence = rollout_cutoff_evidence(
                            path, started + timedelta(seconds=remaining)
                        )
                    except ValueError as error:
                        invalid_reason = f"segment {index + 1}: {error}"
                        break
                    cutoff = {
                        "segment_index": index,
                        "elapsed_before_segment_seconds": elapsed,
                        "seconds_into_segment": remaining,
                        "evidence": evidence,
                    }
                    previous_score = solved[index - 1] if index else 0
                    if (
                        not previous_score
                        <= evidence["score"]["score"]
                        <= solved[index]
                    ):
                        invalid_reason = (
                            "runtime cutoff score is outside the segment score bounds"
                        )
                    break
                elapsed += seconds

    identity = {
        **first_identity,
        "agent": "continuation-chain",
        "segment_trial_ids": [
            segment["identity"]["harbor_trial_id"] for segment in segments
        ],
    }
    source = {
        "chain_scoring_version": CHAIN_SCORING_VERSION,
        "runtime_budget_seconds": runtime_budget_seconds,
        "segments": [
            {
                "result_json": segment["source"]["result_json"],
                "result_sha256": segment["source"]["result_sha256"],
            }
            for segment in segments
        ],
    }
    termination_chain = [segment["termination"] for segment in segments]
    if invalid_reason is not None:
        return {
            "source": source,
            "identity": identity,
            "score_valid": False,
            "ranking_eligible": False,
            "eligible": False,
            "ineligible_reason": invalid_reason,
            "termination": {"status": "invalid_chain", "segments": termination_chain},
            "raw": None,
            "derived": None,
            "segments": segments,
        }

    final_score = cutoff["evidence"]["score"]["score"] if cutoff else solved[-1]
    cutoff_tokens = cutoff["evidence"]["tokens"] if cutoff else None
    ranking_eligible = (
        cutoff_tokens is not None
        if cutoff
        else all(segment["ranking_eligible"] for segment in segments)
    )
    input_tokens = (
        cutoff_tokens["input_tokens"]
        if cutoff_tokens
        else (
            sum(segment["raw"]["input_tokens"] for segment in segments)
            if ranking_eligible
            else None
        )
    )
    cache_tokens = (
        cutoff_tokens["cache_tokens"]
        if cutoff_tokens
        else (
            sum(segment["raw"]["cache_tokens"] for segment in segments)
            if ranking_eligible
            and all(segment["raw"]["cache_tokens"] is not None for segment in segments)
            else None
        )
    )
    output_tokens = (
        cutoff_tokens["output_tokens"]
        if cutoff_tokens
        else (
            sum(segment["raw"]["output_tokens"] for segment in segments)
            if ranking_eligible
            else None
        )
    )
    total_tokens = input_tokens + output_tokens if ranking_eligible else None
    return {
        "source": source,
        "identity": identity,
        "score_valid": True,
        "ranking_eligible": ranking_eligible,
        "eligible": ranking_eligible,
        "ineligible_reason": (
            None
            if ranking_eligible
            else "token ranking unavailable in one or more continuation segments"
        ),
        "termination": {
            "status": ("runtime_budget_cutoff" if cutoff else "continuation_chain"),
            "segments": termination_chain,
        },
        "raw": {
            "solved_levels": final_score,
            "input_tokens": input_tokens,
            "cache_tokens": cache_tokens,
            "output_tokens": output_tokens,
            "total_tokens": total_tokens,
            "segment_count": len(segments),
            "segment_scores": solved,
            "runtime_cutoff": cutoff,
            "token_accounting": (
                (
                    "use the raw rollout's cumulative input_tokens + "
                    "output_tokens at the exact runtime cutoff"
                    if cutoff
                    else "sum each segment's input_tokens + output_tokens; "
                    "use only the final cumulative solved_levels as score"
                )
                if ranking_eligible
                else "unavailable"
            ),
        },
        "derived": {
            "score": final_score,
            "token_tiebreak": total_tokens,
            "ascending_rank_key": (
                [-final_score, total_tokens] if ranking_eligible else None
            ),
        },
        "segments": segments,
    }


def report(
    paths: list[Path],
    chain: bool = False,
    runtime_budget_seconds: float | None = None,
    recovery_paths: list[Path] | None = None,
) -> dict[str, Any]:
    return {
        "scoring": {
            "version": SCORING_VERSION,
            "chain_version": CHAIN_SCORING_VERSION,
            "formula": FORMULA,
            "level_count": LEVEL_COUNT,
            "time_policy": (
                "Elapsed duration is not a score factor. The configured Agent "
                "cutoff only excludes sidecar actions recorded after "
                "agent_execution.finished_at."
            ),
            "ordering_guarantee": (
                "Each solved puzzle contributes exactly one point. Token use "
                "only breaks ties between trials with the same integer score."
            ),
            "eligibility_policy": (
                "A valid verifier result retains its integer score even when "
                "token accounting is unavailable. Such a trial is excluded "
                "only from token-based tie ranking. Reaching the configured "
                "Agent timeout is an expected benchmark cutoff. The isolated "
                "verifier establishes final-state integrity, while the "
                "sidecar event stream removes any score gained after the "
                "recorded Agent cutoff. Other infrastructure exceptions "
                "invalidate the score itself."
            ),
            "continuation_policy": (
                "Ordered continuation segments must share task, checksum, "
                "model, and reasoning effort, and cumulative scores may not "
                "decrease. The chain score is the final cumulative score; "
                "tokens are summed across every segment. When a cumulative "
                "runtime budget cuts through a segment, score and cumulative "
                "tokens come from the last raw rollout evidence at that exact "
                "cutoff."
            ),
        },
        "trials": (
            [score_chain(paths, runtime_budget_seconds, recovery_paths)]
            if chain
            else [score(path, recovery_paths) for path in paths]
        ),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("result", type=Path, nargs="+")
    parser.add_argument("--output", type=Path)
    parser.add_argument(
        "--chain",
        action="store_true",
        help="treat ordered result files as one continuation chain",
    )
    parser.add_argument(
        "--runtime-budget-seconds",
        type=float,
        help="apply one cumulative agent-execution budget to a continuation chain",
    )
    parser.add_argument(
        "--verifier-recovery",
        action="append",
        type=Path,
        default=[],
        help="verified recovery attestation for a matching result SHA-256",
    )
    args = parser.parse_args()
    if args.runtime_budget_seconds is not None and not args.chain:
        parser.error("--runtime-budget-seconds requires --chain")
    output = (
        json.dumps(
            report(
                args.result,
                chain=args.chain,
                runtime_budget_seconds=args.runtime_budget_seconds,
                recovery_paths=args.verifier_recovery,
            ),
            indent=2,
            ensure_ascii=False,
        )
        + "\n"
    )
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(output)
    else:
        print(output, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
