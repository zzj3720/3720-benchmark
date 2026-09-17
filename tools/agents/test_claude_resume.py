import asyncio
import json

from claude_resume import ClaudeSessionResume


class _Parent:
    def __init__(self):
        self.calls = []
        self._resume = False

    async def setup(self, environment):
        self.calls.append(("parent_setup", environment))

    async def exec_as_root(self, environment, *, command):
        self.calls.append(("root", command))

    async def run(self, instruction, environment, context):
        self.calls.append(("run", instruction, self._resume))


class _ResumeProbe(ClaudeSessionResume, _Parent):
    pass


class _Environment:
    def __init__(self):
        self.uploads = []

    async def upload_dir(self, source, target):
        self.uploads.append((source, target))


def test_claude_resume_restores_session_workspace_and_resume_flag(tmp_path):
    sessions = tmp_path / "sessions"
    workspace = tmp_path / "workspace"
    sessions.mkdir()
    workspace.mkdir()
    agent = _ResumeProbe()
    agent._configure_claude_resume(
        resume_sessions_dir=str(sessions),
        resume_workspace_dir=str(workspace),
    )
    environment = _Environment()

    asyncio.run(agent.setup(environment))
    asyncio.run(agent.run("continue", environment, None))

    assert environment.uploads == [
        (sessions, "/logs/agent/sessions"),
        (workspace, "/app"),
    ]
    assert ("run", "continue", True) in agent.calls
    assert not agent._resume


def test_claude_resume_rejects_missing_workspace(tmp_path):
    sessions = tmp_path / "sessions"
    sessions.mkdir()
    agent = _ResumeProbe()

    try:
        agent._configure_claude_resume(
            resume_sessions_dir=str(sessions),
            resume_workspace_dir=str(tmp_path / "missing"),
        )
    except ValueError as error:
        assert "resume_workspace_dir is not a directory" in str(error)
    else:
        raise AssertionError("missing workspace was accepted")


def test_trim_after_last_compact_keeps_its_summary(tmp_path):
    session = tmp_path / "session.jsonl"
    rows = [
        {"type": "user", "uuid": "old"},
        {
            "type": "system",
            "subtype": "compact_boundary",
            "uuid": "boundary",
        },
        {
            "type": "user",
            "uuid": "summary",
            "parentUuid": "boundary",
            "message": {"content": "summary"},
        },
        {"type": "assistant", "uuid": "discard", "parentUuid": "summary"},
    ]
    session.write_text(
        "\n".join(json.dumps(row) for row in rows) + "\n",
        encoding="utf-8",
    )

    ClaudeSessionResume._trim_after_last_compact(session)

    assert [json.loads(line)["uuid"] for line in session.read_text().splitlines()] == [
        "old",
        "boundary",
        "summary",
    ]
