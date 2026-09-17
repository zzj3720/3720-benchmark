"""Persistent Pi lifecycles for benchmarks with authoritative terminal states."""

from __future__ import annotations

import json
import re
import shutil
import time
from pathlib import Path
from typing import Any

from harbor.agents.installed.pi import Pi
from harbor.environments.base import BaseEnvironment
from harbor.models.agent.context import AgentContext

from tools.agents.parabox_resume import ParaboxResume


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


def _operator_completion_evidence(logs_dir: Path) -> dict[str, Any] | None:
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


def _campaign_completion_evidence(
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


def _sokoban_completion_evidence(logs_dir: Path) -> dict[str, Any] | None:
    return _campaign_completion_evidence(logs_dir, "sokoban-api-v1", 305)


def _last_pi_stop(logs_dir: Path, after_id: str | None = None) -> dict[str, Any]:
    sessions_dir = logs_dir / "pi" / "sessions"
    try:
        sessions = sorted(
            sessions_dir.glob("*.jsonl"),
            key=lambda path: path.stat().st_mtime_ns,
            reverse=True,
        )
    except OSError:
        return {"reason": None, "error": None}
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
            if isinstance(message, dict) and message.get("role") == "assistant":
                event_id = (
                    message.get("id") or event.get("id") or event.get("timestamp")
                )
                if event_id == after_id:
                    return {"id": event_id, "reason": None, "error": None}
                return {
                    "id": event_id,
                    "reason": message.get("stopReason"),
                    "error": message.get("errorMessage"),
                }
    return {"id": None, "reason": None, "error": None}


class EmergencyOperatorGoalPi(Pi):
    """Keep one Pi session active until the authoritative shift is complete."""

    _AGGREGATE_FILENAME = "goal-pi-transcript.jsonl"
    _EVENTS_FILENAME = "goal-pi-events.jsonl"
    _TERMINAL_REQUIREMENT = (
        "an authoritative operator tool response reports that the shift status "
        "is `complete` (or `operator submit` reports `complete: true`)"
    )
    _CONTINUE_INSTRUCTION = (
        "Keep making your own timely decisions until a normal operator tool "
        "response shows `shift.status` equal to `complete` or `operator submit` "
        "returns `complete: true`; only then may you give a final answer."
    )

    async def install(self, environment: BaseEnvironment) -> None:
        await super().install(environment)
        await self.exec_as_agent(
            environment,
            command=(
                "mkdir -p $HOME/.pi/agent && "
                "printf '%s\\n' "
                """'{"compaction":{"enabled":true,"reserveTokens":65536,"keepRecentTokens":20000}}' """
                "> $HOME/.pi/agent/settings.json"
            ),
        )

    def __init__(self, *args, goal_objective: str, **kwargs):
        super().__init__(*args, **kwargs)
        self._goal_objective = goal_objective.strip()
        if not self._goal_objective:
            raise ValueError("goal_objective must not be empty")
        if len(self._goal_objective) > 4_000:
            raise ValueError("goal_objective must be at most 4,000 characters")
        self._premature_finals = 0
        self._interrupted_segments = 0
        self._completion_evidence: dict[str, Any] | None = None

    async def _run_pi_segment(
        self,
        instruction: str,
        environment: BaseEnvironment,
        context: AgentContext,
    ) -> None:
        await Pi.run(self, instruction, environment, context)

    def _record_segment(self) -> None:
        output = self.logs_dir / self._OUTPUT_FILENAME
        if not output.is_file():
            return
        data = output.read_bytes()
        if data and not data.endswith(b"\n"):
            data += b"\n"
        with (self.logs_dir / self._AGGREGATE_FILENAME).open("ab") as aggregate:
            aggregate.write(data)

    def _record_event(self, kind: str, **details: Any) -> None:
        record = {
            "schema": "goal-pi-event-v1",
            "kind": kind,
            "timestamp_ms": int(time.time() * 1_000),
            **details,
        }
        with (self.logs_dir / self._EVENTS_FILENAME).open(
            "a", encoding="utf-8"
        ) as events:
            events.write(json.dumps(record, sort_keys=True) + "\n")

    def _restore_aggregate_output(self) -> None:
        aggregate = self.logs_dir / self._AGGREGATE_FILENAME
        if aggregate.is_file():
            shutil.copyfile(aggregate, self.logs_dir / self._OUTPUT_FILENAME)

    def _find_completion_evidence(self) -> dict[str, Any] | None:
        return _operator_completion_evidence(self.logs_dir)

    async def run(
        self,
        instruction: str,
        environment: BaseEnvironment,
        context: AgentContext,
    ) -> None:
        aggregate = self.logs_dir / self._AGGREGATE_FILENAME
        events = self.logs_dir / self._EVENTS_FILENAME
        aggregate.unlink(missing_ok=True)
        events.unlink(missing_ok=True)

        prompt = (
            "You are running under an enforced persistent benchmark objective:\n\n"
            f"{self._goal_objective}\n\n"
            "You must personally continue playing until "
            f"{self._TERMINAL_REQUIREMENT}. A final answer before "
            "that terminal evidence is a recorded premature stop and does not end "
            "the run. Do not treat partial progress, a checkpoint score, elapsed "
            "effort, or a difficult remaining situation as completion.\n\n"
            f"{instruction}"
        )

        try:
            while True:
                previous_assistant_id = _last_pi_stop(self.logs_dir)["id"]
                try:
                    await self._run_pi_segment(prompt, environment, context)
                finally:
                    self._record_segment()

                evidence = self._find_completion_evidence()
                if evidence is not None:
                    self._completion_evidence = evidence
                    self._record_event(
                        "goal_complete",
                        premature_finals=self._premature_finals,
                        evidence=evidence,
                    )
                    break

                self._resume = True
                stop = _last_pi_stop(self.logs_dir, after_id=previous_assistant_id)
                if stop["reason"] == "stop":
                    self._premature_finals += 1
                    self._record_event(
                        "premature_final",
                        number=self._premature_finals,
                    )
                    prompt = (
                        "Your previous final answer attempted to stop before the "
                        "authoritative game end. This premature stop has been "
                        f"recorded (number {self._premature_finals}). Continue the "
                        "same live game and the same Pi session now. Do not repeat "
                        "the summary or submit another checkpoint as a final result. "
                        f"{self._CONTINUE_INSTRUCTION}"
                    )
                else:
                    self._interrupted_segments += 1
                    self._record_event(
                        "segment_interrupted",
                        number=self._interrupted_segments,
                        stop=stop,
                    )
                    prompt = (
                        "The previous Pi segment was interrupted before the "
                        "authoritative game end. Resume the same live game and Pi "
                        f"session now. {self._CONTINUE_INSTRUCTION}"
                    )
        finally:
            self._resume = False
            self._restore_aggregate_output()

    def populate_context_post_run(self, context: AgentContext) -> None:
        super().populate_context_post_run(context)
        context.metadata = {
            **(context.metadata or {}),
            "goal_pi": {
                "completed": self._completion_evidence is not None,
                "premature_finals": self._premature_finals,
                "interrupted_segments": self._interrupted_segments,
                "completion_evidence": self._completion_evidence,
            },
        }


class CampaignGoalPi(EmergencyOperatorGoalPi):
    """Keep one Pi session active until a standard campaign is complete."""

    def __init__(
        self,
        *args,
        api_version: str,
        game_command: str,
        max_score: int,
        **kwargs,
    ):
        super().__init__(*args, **kwargs)
        try:
            max_score = int(max_score)
        except (TypeError, ValueError) as error:
            raise ValueError("campaign max_score must be a positive integer") from error
        if not api_version or not game_command or max_score <= 0:
            raise ValueError("campaign completion settings are invalid")
        self._api_version = api_version
        self._max_score = max_score
        terminal = (
            f"a successful `{game_command} show` or `{game_command} submit` response "
            f"reports the task's authoritative `complete: true` state and a terminal "
            f"score out of {max_score}"
        )
        self._TERMINAL_REQUIREMENT = terminal
        self._CONTINUE_INSTRUCTION = (
            "Keep choosing and executing every action yourself until "
            f"{terminal}; only then may you give a final answer."
        )

    def _find_completion_evidence(self) -> dict[str, Any] | None:
        return _campaign_completion_evidence(
            self.logs_dir, self._api_version, self._max_score
        )


class ResumeParaboxCampaignGoalPi(ParaboxResume, CampaignGoalPi):
    """Resume one native Pi session and its matching private Parabox state."""

    def __init__(
        self,
        *args,
        resume_sessions_dir: str,
        resume_game_state_path: str,
        resume_game_audit_path: str | None = None,
        resume_game_events_path: str | None = None,
        **kwargs,
    ):
        super().__init__(*args, **kwargs)
        self._resume_sessions_dir = Path(resume_sessions_dir)
        if not self._resume_sessions_dir.is_dir():
            raise ValueError(
                f"resume_sessions_dir is not a directory: "
                f"{self._resume_sessions_dir}"
            )
        self._configure_parabox_resume(
            resume_game_state_path=resume_game_state_path,
            resume_game_audit_path=resume_game_audit_path,
            resume_game_events_path=resume_game_events_path,
        )

    async def setup(self, environment: BaseEnvironment) -> None:
        await super().setup(environment)
        await self.exec_as_root(
            environment,
            command="mkdir -p /logs/agent/pi/sessions && chmod -R 777 /logs/agent",
        )
        await environment.upload_dir(
            self._resume_sessions_dir,
            "/logs/agent/pi/sessions",
        )
        await self._restore_parabox(environment)

    async def run(
        self,
        instruction: str,
        environment: BaseEnvironment,
        context: AgentContext,
    ) -> None:
        self._resume = True
        await super().run(instruction, environment, context)


class SokobanGoalPi(CampaignGoalPi):
    """Compatibility name for existing Sokoban jobs."""

    def __init__(self, *args, **kwargs):
        super().__init__(
            *args,
            api_version="sokoban-api-v1",
            game_command="sokoban",
            max_score=305,
            **kwargs,
        )
