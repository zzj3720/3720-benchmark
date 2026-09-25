"""Per-run adapter adding Codex Fast service tier to isolated benchmark agent."""
from harbor.agents.installed.base import CliFlag
from tools.agents.isolated_codex import IsolatedCodex

class FastIsolatedCodex(IsolatedCodex):
    CLI_FLAGS = [
        *IsolatedCodex.CLI_FLAGS,
        CliFlag('service_tier', cli='-c', type='enum', choices=['default', 'priority'],
                format='-c service_tier={value}'),
    ]

from tools.agents.isolated_codex import ResumeIsolatedCodex

class FastResumeIsolatedCodex(ResumeIsolatedCodex):
    CLI_FLAGS = FastIsolatedCodex.CLI_FLAGS

from tools.agents.isolated_codex import ContinueGoalResumeIsolatedCodex
class FastContinueGoalResumeIsolatedCodex(ContinueGoalResumeIsolatedCodex):
    CLI_FLAGS = FastIsolatedCodex.CLI_FLAGS
