"""Harbor Codex adapter with external tools disabled for benchmark trials."""

import json
import shlex
from pathlib import Path

from harbor.agents.installed.base import CliFlag
from harbor.agents.installed.codex import Codex
from harbor.environments.base import BaseEnvironment
from harbor.models.agent.context import AgentContext
from harbor.models.trial.paths import EnvironmentPaths

from tools.agents.game_support.parabox import ParaboxResume
from tools.agents.game_support.swarm import SwarmResume


_GOAL_CONTEXT_MARKER = '<codex_internal_context source="goal">'
_SESSION_SYNC_PID = "/tmp/harbor-codex-session-sync.pid"
_SESSION_SYNC_STOP = "/tmp/harbor-codex-session-sync.stop"
_SAME_TURN_PROGRESS_INSTRUCTION = (
    "Restoring the Goal is setup, not task progress. Do not wait for an "
    "automatic Goal continuation because this headless CLI aborts that "
    "automatic turn. In this same explicit turn, immediately inspect the "
    "current task state and perform concrete task work. Do not end with only "
    "a Goal, score, or status confirmation."
)


def _session_sync_start_command() -> str:
    """Build the command that continuously checkpoints a live Codex rollout."""

    source = (Codex._REMOTE_CODEX_HOME / "sessions").as_posix()
    destination = (EnvironmentPaths.agent_dir / "sessions").as_posix()
    loop = (
        f"while [ ! -e {shlex.quote(_SESSION_SYNC_STOP)} ]; do "
        f"if [ -d {shlex.quote(source)} ]; then "
        f"mkdir -p {shlex.quote(destination)}; "
        f"cp -R {shlex.quote(source + '/.')} "
        f"{shlex.quote(destination + '/')} 2>/dev/null || true; "
        "fi; sleep 5; done"
    )
    return (
        f"rm -f {shlex.quote(_SESSION_SYNC_STOP)} "
        f"{shlex.quote(_SESSION_SYNC_PID)}; "
        f"mkdir -p {shlex.quote(destination)}; "
        f"nohup sh -c {shlex.quote(loop)} </dev/null >/dev/null 2>&1 & "
        f"echo $! > {shlex.quote(_SESSION_SYNC_PID)}"
    )


def _session_sync_stop_command() -> str:
    """Build the best-effort shutdown command for the checkpoint loop."""

    return (
        f"touch {shlex.quote(_SESSION_SYNC_STOP)}; "
        f"if [ -s {shlex.quote(_SESSION_SYNC_PID)} ]; then "
        f"kill \"$(cat {shlex.quote(_SESSION_SYNC_PID)})\" 2>/dev/null || true; "
        "fi; "
        f"rm -f {shlex.quote(_SESSION_SYNC_PID)} "
        f"{shlex.quote(_SESSION_SYNC_STOP)}"
    )


def _completed_turn_left_goal_active(events: list[dict]) -> bool:
    """Return whether the latest completed turn created but did not close a Goal."""

    saw_complete = False
    for event in reversed(events):
        payload = event["payload"]
        if not saw_complete:
            if (
                event.get("type") == "event_msg"
                and payload.get("type") == "task_complete"
            ):
                saw_complete = True
            continue
        if event.get("type") == "event_msg" and payload.get("type") == "task_started":
            return False
        if (
            event.get("type") != "response_item"
            or payload.get("type") != "custom_tool_call"
        ):
            continue
        source = payload.get("input")
        if not isinstance(source, str):
            continue
        if "tools.update_goal(" in source:
            return False
        if "tools.create_goal(" in source:
            return True
    return False


def _goal_continuation_pending(logs_dir: Path) -> bool:
    """Return whether Codex queued a Goal turn that CLI teardown aborted."""

    session_dir = logs_dir / "sessions"
    try:
        sessions = list(session_dir.rglob("rollout-*.jsonl"))
        session = max(sessions, key=lambda path: path.stat().st_mtime_ns)
        lines = session.read_text(encoding="utf-8").splitlines()
    except (OSError, ValueError):
        return False

    all_events = []
    for line in lines:
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(event.get("payload"), dict):
            all_events.append(event)
    events = all_events[-32:]

    last_complete = -1
    last_started = -1
    for index, event in enumerate(events):
        payload = event["payload"]
        if event.get("type") == "event_msg" and payload.get("type") == "task_complete":
            last_complete = index
        elif event.get("type") == "event_msg" and payload.get("type") == "task_started":
            last_started = index
    if last_started > last_complete >= 0:
        for event in events[last_started + 1 :]:
            payload = event["payload"]
            content = payload.get("content")
            content_parts = content if isinstance(content, list) else [content]
            if (
                event.get("type") == "response_item"
                and payload.get("type") == "message"
                and payload.get("role") == "user"
                and any(
                    _GOAL_CONTEXT_MARKER
                    in (
                        part.get("text", "")
                        if isinstance(part, dict)
                        else str(part or "")
                    )
                    for part in content_parts
                )
            ):
                return True

    if _completed_turn_left_goal_active(all_events):
        return True

    saw_aborted_turn = False
    saw_started_turn = False
    for event in reversed(events):
        payload = event["payload"]
        if event.get("type") == "event_msg" and payload.get("type") == "turn_aborted":
            saw_aborted_turn = True
            continue
        if not saw_aborted_turn:
            continue
        if event.get("type") == "event_msg" and payload.get("type") == "task_started":
            saw_started_turn = True
            continue
        if (
            saw_started_turn
            and event.get("type") == "event_msg"
            and payload.get("type") == "task_complete"
        ):
            return True
        content = payload.get("content")
        content_parts = content if isinstance(content, list) else [content]
        if (
            event.get("type") == "response_item"
            and payload.get("type") == "message"
            and payload.get("role") == "user"
            and any(
                _GOAL_CONTEXT_MARKER
                in (part.get("text", "") if isinstance(part, dict) else str(part or ""))
                for part in content_parts
            )
        ):
            return True
    return False


async def _continue_goal_turns(
    agent: "IsolatedCodex",
    environment: BaseEnvironment,
    context: AgentContext,
) -> None:
    """Keep the CLI process alive across Codex's persisted Goal turns."""

    while _goal_continuation_pending(agent.logs_dir):
        goal_objective = getattr(agent, "_goal_objective", "")
        continuation_instruction = (
            "Before taking any task action, call get_goal. The headless "
            "`codex exec` host cannot retain the Goal state database between "
            "segments. If no unfinished Goal exists, call create_goal with "
            "this exact unchanged objective:\n\n"
            f"{goal_objective}\n\n"
            "If a different unfinished Goal exists, stop and report the "
            "mismatch. Continue the same work from the current workspace and "
            "external state. Do not clear, replace, narrow, or prematurely "
            "complete the Goal. "
            f"{_SAME_TURN_PROGRESS_INSTRUCTION}"
        )
        agent._resume = True
        try:
            await IsolatedCodex.run(
                agent,
                continuation_instruction,
                environment,
                context,
            )
        finally:
            agent._resume = False


class IsolatedCodex(Codex):
    """Run Codex with only the task container's built-in terminal tools.

    A ChatGPT-authenticated Codex CLI can expose account-level Apps even when
    its local CODEX_HOME has no MCP configuration. Those tools bypass the
    task container's network policy, so benchmark trials must disable them at
    the Codex feature layer.
    """

    CLI_FLAGS = [
        *Codex.CLI_FLAGS,
        CliFlag(
            "isolate_external_tools",
            cli="",
            type="bool",
            default=True,
            format=(
                "--ignore-user-config "
                "--disable apps "
                "--disable plugins "
                "--disable remote_plugin "
                "--disable browser_use "
                "--disable browser_use_external "
                "--disable browser_use_full_cdp_access "
                "--disable in_app_browser "
                "--disable computer_use "
                "--disable image_generation"
            ),
        ),
    ]

    async def run(
        self,
        instruction: str,
        environment: BaseEnvironment,
        context: AgentContext,
    ) -> None:
        # Harbor's stock adapter copies Codex sessions only after `codex exec`
        # exits. Checkpoint the live JSONL as well so a killed container still
        # leaves a resumable and auditable trajectory.
        await self.exec_as_agent(
            environment,
            command=_session_sync_start_command(),
        )
        try:
            await super().run(instruction, environment, context)
        finally:
            try:
                await self.exec_as_agent(
                    environment,
                    command=_session_sync_stop_command(),
                )
            except Exception:
                self.logger.exception("Failed to stop Codex session checkpoint loop")


class GoalIsolatedCodex(IsolatedCodex):
    """Start a fresh isolated Codex session with one persisted Goal."""

    def __init__(self, *args, goal_objective: str, **kwargs):
        super().__init__(*args, **kwargs)
        self._goal_objective = goal_objective.strip()
        if not self._goal_objective:
            raise ValueError("goal_objective must not be empty")
        if len(self._goal_objective) > 4_000:
            raise ValueError("goal_objective must be at most 4,000 characters")

    async def run(
        self,
        instruction: str,
        environment: BaseEnvironment,
        context: AgentContext,
    ) -> None:
        goal_instruction = (
            "Before taking any other action, call create_goal with this exact "
            f"objective:\n\n{self._goal_objective}\n\n"
            "Do not start work until create_goal confirms that the persisted "
            "Goal is active.\n\n"
            f"{instruction}"
        )
        await super().run(goal_instruction, environment, context)
        await _continue_goal_turns(self, environment, context)


class ResumeIsolatedCodex(IsolatedCodex):
    """Continue a captured Codex session under the isolated tool policy."""

    def __init__(
        self,
        *args,
        resume_sessions_dir: str,
        resume_workspace_dir: str | None = None,
        **kwargs,
    ):
        super().__init__(*args, **kwargs)
        self._resume_sessions_dir = Path(resume_sessions_dir)
        self._resume_workspace_dir = (
            Path(resume_workspace_dir) if resume_workspace_dir else None
        )
        if not self._resume_sessions_dir.is_dir():
            raise ValueError(
                f"resume_sessions_dir is not a directory: {self._resume_sessions_dir}"
            )
        if (
            self._resume_workspace_dir is not None
            and not self._resume_workspace_dir.is_dir()
        ):
            raise ValueError(
                f"resume_workspace_dir is not a directory: {self._resume_workspace_dir}"
            )

    async def setup(self, environment: BaseEnvironment) -> None:
        await super().setup(environment)
        await self.exec_as_root(
            environment,
            command=(
                f"mkdir -p {EnvironmentPaths.agent_dir / 'sessions'} "
                "&& chmod -R 777 /logs/agent"
            ),
        )
        await environment.upload_dir(
            self._resume_sessions_dir,
            (EnvironmentPaths.agent_dir / "sessions").as_posix(),
        )
        if self._resume_workspace_dir is not None:
            await environment.upload_dir(self._resume_workspace_dir, "/app")

    async def run(
        self,
        instruction: str,
        environment: BaseEnvironment,
        context: AgentContext,
    ) -> None:
        self._resume = True
        try:
            await super().run(instruction, environment, context)
        finally:
            self._resume = False


class GoalResumeIsolatedCodex(ResumeIsolatedCodex):
    """Resume a captured session after creating a persisted Codex Goal."""

    def __init__(self, *args, goal_objective: str, **kwargs):
        super().__init__(*args, **kwargs)
        self._goal_objective = goal_objective.strip()
        if not self._goal_objective:
            raise ValueError("goal_objective must not be empty")
        if len(self._goal_objective) > 4_000:
            raise ValueError("goal_objective must be at most 4,000 characters")

    async def run(
        self,
        instruction: str,
        environment: BaseEnvironment,
        context: AgentContext,
    ) -> None:
        goal_instruction = (
            "Before taking any other action, call create_goal with this exact "
            f"objective:\n\n{self._goal_objective}\n\n"
            "Do not start work until create_goal confirms that the persisted "
            "Goal is active.\n\n"
            f"{instruction}"
        )
        await super().run(goal_instruction, environment, context)
        await _continue_goal_turns(self, environment, context)


class ContinueGoalResumeIsolatedCodex(GoalResumeIsolatedCodex):
    """Resume a session that should already contain the persisted Goal."""

    async def run(
        self,
        instruction: str,
        environment: BaseEnvironment,
        context: AgentContext,
    ) -> None:
        goal_instruction = (
            "Before taking any other action, call get_goal. Continue only if "
            "it confirms that this exact persisted Goal is still active:\n\n"
            f"{self._goal_objective}\n\n"
            "If no unfinished Goal exists, call create_goal with that exact "
            "objective and confirm that it becomes active. If a different "
            "unfinished Goal exists, stop and report the mismatch. Do not "
            "mark the Goal complete unless its full stated completion "
            "condition is actually satisfied. "
            f"{_SAME_TURN_PROGRESS_INSTRUCTION}\n\n"
            f"{instruction}"
        )
        await ResumeIsolatedCodex.run(
            self,
            goal_instruction,
            environment,
            context,
        )
        await _continue_goal_turns(self, environment, context)


class ContinueParaboxGoalResumeIsolatedCodex(
    ParaboxResume,
    ContinueGoalResumeIsolatedCodex,
):
    """Resume a Codex Goal together with private Parabox sidecar state."""

    def __init__(
        self,
        *args,
        resume_game_state_path: str,
        resume_game_audit_path: str | None = None,
        resume_game_events_path: str | None = None,
        **kwargs,
    ):
        super().__init__(*args, **kwargs)
        self._configure_parabox_resume(
            resume_game_state_path=resume_game_state_path,
            resume_game_audit_path=resume_game_audit_path,
            resume_game_events_path=resume_game_events_path,
        )

    async def setup(self, environment: BaseEnvironment) -> None:
        await super().setup(environment)
        await self._restore_parabox(environment)


class ContinueSwarmGoalResumeIsolatedCodex(
    SwarmResume,
    ContinueGoalResumeIsolatedCodex,
):
    """Resume a Codex Goal together with its deterministic Swarm world."""

    def __init__(
        self,
        *args,
        resume_game_audit_path: str,
        **kwargs,
    ):
        super().__init__(*args, **kwargs)
        self._configure_swarm_resume(
            resume_game_audit_path=resume_game_audit_path,
        )

    async def setup(self, environment: BaseEnvironment) -> None:
        await super().setup(environment)
        await self._restore_swarm(environment)
