import asyncio

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
