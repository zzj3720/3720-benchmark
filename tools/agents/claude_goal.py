"""Shared native Claude Code `/goal` lifecycle for Harbor adapters."""

import json
from pathlib import Path

from harbor.environments.base import BaseEnvironment
from harbor.models.agent.context import AgentContext


def _latest_goal_stop_was_rejected(logs_dir: Path) -> bool:
    """Return whether Claude's most recent Goal stop check rejected the exit."""

    session_root = logs_dir / "sessions" / "projects"
    try:
        session = max(
            session_root.rglob("*.jsonl"),
            key=lambda path: path.stat().st_mtime_ns,
        )
        lines = session.read_text(encoding="utf-8").splitlines()
    except (OSError, ValueError):
        return False

    for line in reversed(lines[-64:]):
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        attachment = event.get("attachment")
        if not isinstance(attachment, dict):
            continue
        if (
            attachment.get("type") != "hook_non_blocking_error"
            or attachment.get("hookName") != "Stop"
        ):
            continue
        output = attachment.get("stdout")
        return isinstance(output, str) and (
            '"ok": false' in output
            or "stop condition has **not** been satisfied" in output.lower()
        )
    return False


class NativeClaudeGoal:
    """Run a task through Claude Code's session-scoped goal evaluator."""

    _TASK_INSTRUCTION_PATH = "/tmp/harbor-task-instruction.md"

    def __init__(self, *args, goal_objective: str, **kwargs):
        super().__init__(*args, **kwargs)
        self._goal_objective = goal_objective.strip()
        if not self._goal_objective:
            raise ValueError("goal_objective must not be empty")

        self._goal_condition = (
            f"{self._goal_objective}\n\n"
            "Before taking any task action, read and follow the complete task "
            f"specification at {self._TASK_INSTRUCTION_PATH}. Do not clear or "
            "replace this goal. Treat the goal as achieved only after your own "
            "transcript contains the task's required independent verification "
            "and proves the stated completion condition; partial progress, an "
            "unsupported success claim, or a difficult remaining step is not "
            "completion."
        )
        if len(self._goal_condition) > 4_000:
            raise ValueError("native Claude goal condition exceeds 4,000 characters")

    async def run(
        self,
        instruction: str,
        environment: BaseEnvironment,
        context: AgentContext,
    ) -> None:
        instruction_env = "HARBOR_CLAUDE_GOAL_TASK_INSTRUCTION"
        await self.exec_as_agent(
            environment,
            command=(
                f'printf "%s" "${instruction_env}" > '
                f"{self._TASK_INSTRUCTION_PATH}; "
                f"chmod 400 {self._TASK_INSTRUCTION_PATH}"
            ),
            env={instruction_env: instruction},
        )
        await super().run(
            f"/goal {self._goal_condition}",
            environment,
            context,
        )
        while _latest_goal_stop_was_rejected(self.logs_dir):
            self._resume = True
            try:
                await super().run(
                    "The persistent Goal stop evaluator rejected the previous "
                    "attempt because the completion condition is still false. "
                    "Continue the same task now from the current workspace and "
                    "external state. Do not repeat the rejected final answer, "
                    "clear or replace the Goal, or stop at partial progress. "
                    "Take concrete task actions and continue until the Goal's "
                    "stated completion condition is proved or the runtime "
                    "actually ends.",
                    environment,
                    context,
                )
            finally:
                self._resume = False
