"""Recognize a completed Emergency Operator shift in a Pi session log."""

from __future__ import annotations

import json
import re
from pathlib import Path
from typing import Any


_SHIFT_COMPLETE = re.compile(r"(?im)^shift status:\s*complete\s*$")
_SUBMIT_COMPLETE = re.compile(r"(?im)^complete:\s*true\s*$")


def _completion_from_tool_text(text: str) -> dict[str, Any] | None:
    """Recognize either raw API JSON or a concise projection printed by Pi."""

    try:
        response = json.loads(text)
    except json.JSONDecodeError:
        response = None

    if isinstance(response, dict):
        if response.get("api_version") != "emergency-operator-api-v1":
            return None
        data = response.get("data")
        if not isinstance(data, dict):
            return None
        state = data.get("state", data)
        shift = state.get("shift") if isinstance(state, dict) else None
        status = shift.get("status") if isinstance(shift, dict) else None
        if data.get("complete") is True or status == "complete":
            return {
                "command": response.get("command"),
                "complete": data.get("complete") is True,
                "shift_status": status,
                "source": "api_json",
            }
        return None

    shift_complete = _SHIFT_COMPLETE.search(text) is not None
    submit_complete = _SUBMIT_COMPLETE.search(text) is not None
    if not shift_complete and not submit_complete:
        return None
    return {
        "command": "submit" if submit_complete else "show",
        "complete": submit_complete,
        "shift_status": "complete" if shift_complete else None,
        "source": "tool_projection",
    }


def operator_completion_evidence(logs_dir: Path) -> dict[str, Any] | None:
    """Read terminal game evidence already observed by the model."""

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
                evidence = _completion_from_tool_text(part["text"])
                if evidence is not None:
                    return evidence
    return None
