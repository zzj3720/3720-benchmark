"""Recognize a completed deterministic campaign (Sokoban, Minesweeper, ...) in a Pi session log."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any


def campaign_completion_evidence(
    logs_dir: Path, api_version: str, max_score: int
) -> dict[str, Any] | None:
    """Read a completed deterministic campaign response already observed by Pi."""

    sessions_dir = logs_dir / "pi" / "sessions"
    try:
        sessions = sorted(
            sessions_dir.glob("*.jsonl"),
            key=lambda path: path.stat().st_mtime_ns,
            reverse=True,
        )
    except OSError:
        return None

    for session in sessions:
        try:
            lines = session.read_text(encoding="utf-8").splitlines()
        except OSError:
            continue
        for line in reversed(lines):
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            message = event.get("message")
            if not isinstance(message, dict) or message.get("role") != "toolResult":
                continue
            if message.get("isError") is True:
                continue
            content = message.get("content")
            if not isinstance(content, list):
                continue
            for part in content:
                if not isinstance(part, dict) or not isinstance(part.get("text"), str):
                    continue
                try:
                    response = json.loads(part["text"])
                except json.JSONDecodeError:
                    continue
                if (
                    not isinstance(response, dict)
                    or response.get("api_version") != api_version
                    or response.get("ok") is not True
                ):
                    continue
                data = response.get("data")
                if not isinstance(data, dict):
                    continue
                state = data.get("state", data)
                campaign = state.get("campaign") if isinstance(state, dict) else None
                score = (
                    data.get("score")
                    if isinstance(data.get("score"), int)
                    else campaign.get("score")
                    if isinstance(campaign, dict)
                    else None
                )
                reported_max = (
                    data.get("max_score")
                    if isinstance(data.get("max_score"), int)
                    else campaign.get("max_score")
                    if isinstance(campaign, dict)
                    else None
                )
                complete = data.get("complete") is True or (
                    isinstance(campaign, dict) and campaign.get("complete") is True
                )
                if (
                    complete
                    and isinstance(score, int)
                    and score <= max_score
                    and (reported_max is None or reported_max == max_score)
                ):
                    return {
                        "command": response.get("command"),
                        "complete": True,
                        "score": score,
                        "max_score": max_score,
                        "source": "api_json",
                    }
    return None


def sokoban_completion_evidence(logs_dir: Path) -> dict[str, Any] | None:
    return campaign_completion_evidence(logs_dir, "sokoban-api-v1", 305)
