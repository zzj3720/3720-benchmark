#!/usr/bin/env python3
"""Produce a traceable anti-cheat audit from a Parabox agent session."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
from pathlib import Path
from typing import Any


AUDIT_VERSION = "parabox-session-audit-v1"
GAME_COMMAND = re.compile(r"/usr/local/bin/parabox(?:\s|$)")
NETWORK_TOOL = re.compile(r"(?:^|[\s;&|'\"])(curl|wget|nc|ncat|ssh|scp)(?:\s|$)")
INTERPRETER = re.compile(r"(?:^|[\s;&|'\"])(python(?:3)?|node|ruby|perl|lua)(?:\s|$)")
INTERNAL_PATH = re.compile(
    r"(?:^|[\s'\"])(/tests|/solution|/var/lib/parabox|/installed-agent|"
    r"/logs/verifier)(?:[/\s'\"]|$)"
)
SEARCH_OR_SOLVER = re.compile(
    r"\b(solver|solve-level|walkthrough|brute.?force|breadth.?first|"
    r"depth.?first|astar|a\*|solution sequence)\b",
    re.IGNORECASE,
)
SYSTEM_DISCOVERY = re.compile(
    r"(?:^|[\s;&|'\"])(find|locate|docker|podman|ps|lsof)(?:\s|$)"
)
CODE_MODE_TOOL = re.compile(r"\btools\.([A-Za-z_][A-Za-z0-9_]*)\s*\(")
CONTROL_TOOLS = {
    "create_goal",
    "get_goal",
    "update_goal",
    "update_plan",
}


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def read_json_events(path: Path) -> list[tuple[int, dict[str, Any]]]:
    events: list[tuple[int, dict[str, Any]]] = []
    for line_number, line in enumerate(
        path.read_text(errors="replace").splitlines(), 1
    ):
        if not line.startswith("{"):
            continue
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(event, dict):
            events.append((line_number, event))
    return events


def extract_actions(path: Path) -> tuple[str, list[dict[str, Any]]]:
    events = read_json_events(path)
    actions: list[dict[str, Any]] = []
    codex_cli = any(event.get("type", "").startswith("item.") for _, event in events)
    codex_rollout = any(
        event.get("type") == "response_item" and isinstance(event.get("payload"), dict)
        for _, event in events
    )
    qoder = any("qodercli_version" in event for _, event in events)
    if codex_rollout:
        session_format = "codex-rollout-jsonl"
    elif codex_cli:
        session_format = "codex-jsonl"
    elif qoder:
        session_format = "qoder-cn-stream-json"
    else:
        session_format = "anthropic-stream-json"

    for line_number, event in events:
        if event.get("type") == "response_item":
            payload = event.get("payload")
            if not isinstance(payload, dict):
                continue
            if payload.get("type") != "custom_tool_call":
                continue
            tool = str(payload.get("name") or "")
            value = payload.get("input") or payload.get("arguments") or ""
            nested_tools = set(CODE_MODE_TOOL.findall(str(value)))
            if nested_tools and nested_tools <= CONTROL_TOOLS:
                kind = "control"
                tool = ",".join(sorted(nested_tools))
            elif "exec_command" in nested_tools:
                kind = "command"
                tool = "exec_command"
            else:
                kind = "command" if tool == "exec" else "tool_use"
            actions.append(
                {
                    "line": line_number,
                    "kind": kind,
                    "tool": tool,
                    "value": str(value),
                }
            )
            continue

        if event.get("type") == "item.started":
            item = event.get("item")
            if not isinstance(item, dict):
                continue
            if item.get("type") == "command_execution":
                actions.append(
                    {
                        "line": line_number,
                        "kind": "command",
                        "tool": "shell",
                        "value": item.get("command") or "",
                    }
                )
            elif item.get("type") == "file_change":
                for change in item.get("changes") or []:
                    if isinstance(change, dict):
                        actions.append(
                            {
                                "line": line_number,
                                "kind": "file_change",
                                "tool": "file_change",
                                "value": change.get("path") or "",
                            }
                        )
            continue

        if event.get("type") != "assistant":
            continue
        message = event.get("message")
        if not isinstance(message, dict):
            continue
        for block in message.get("content") or []:
            if not isinstance(block, dict) or block.get("type") != "tool_use":
                continue
            tool = str(block.get("name") or "")
            inputs = block.get("input")
            if not isinstance(inputs, dict):
                inputs = {}
            value = (
                inputs.get("command")
                or inputs.get("file_path")
                or inputs.get("path")
                or inputs.get("pattern")
                or ""
            )
            actions.append(
                {
                    "line": line_number,
                    "kind": "command" if tool == "Bash" else "tool_use",
                    "tool": tool,
                    "value": str(value),
                }
            )

    return session_format, actions


def classify(action: dict[str, Any]) -> list[dict[str, str]]:
    value = action["value"]
    flags: list[dict[str, str]] = []

    if action["kind"] == "control":
        return flags

    checks = [
        ("high", "network_tool", NETWORK_TOOL),
        ("high", "benchmark_internal_path", INTERNAL_PATH),
        ("high", "solver_or_external_answer_term", SEARCH_OR_SOLVER),
        ("review", "interpreter_or_script_runtime", INTERPRETER),
        ("review", "system_discovery", SYSTEM_DISCOVERY),
    ]
    for severity, reason, pattern in checks:
        if pattern.search(value):
            flags.append({"severity": severity, "reason": reason})

    if action["kind"] == "command" and not GAME_COMMAND.search(value):
        if not flags:
            flags.append({"severity": "review", "reason": "non_game_shell_command"})
    elif action["kind"] == "tool_use" and action["tool"] not in {
        "Read",
        "Write",
        "Edit",
        "Glob",
        "Grep",
    }:
        flags.append({"severity": "review", "reason": "non_standard_tool"})

    return flags


def audit(path: Path) -> dict[str, Any]:
    session_format, actions = extract_actions(path)
    findings: list[dict[str, Any]] = []
    for index, action in enumerate(actions, 1):
        flags = classify(action)
        action["index"] = index
        action["value_sha256"] = hashlib.sha256(action["value"].encode()).hexdigest()
        if flags:
            findings.append({"action": action, "flags": flags})

    high_risk_count = sum(
        flag["severity"] == "high" for finding in findings for flag in finding["flags"]
    )
    return {
        "audit": {
            "version": AUDIT_VERSION,
            "policy": (
                "High-risk findings identify network tools, benchmark-internal "
                "paths, or solver/external-answer terms. Review findings require "
                "human inspection and are not by themselves proof of cheating."
            ),
        },
        "source": {
            "path": str(path),
            "sha256": sha256(path),
            "format": session_format,
        },
        "summary": {
            "action_count": len(actions),
            "finding_count": len(findings),
            "high_risk_count": high_risk_count,
            "no_high_risk_evidence": high_risk_count == 0,
        },
        "findings": findings,
        "actions": actions,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("session", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    report = json.dumps(audit(args.session), indent=2, ensure_ascii=False) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(report)
    else:
        print(report, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
