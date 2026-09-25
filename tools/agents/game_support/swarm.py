"""Restore a private Swarm audit before continuing an agent trial."""

import base64
import shlex
from pathlib import Path

from harbor.environments.base import BaseEnvironment


class SwarmResume:
    """Restore the append-only audit that deterministically defines the world."""

    def _configure_swarm_resume(self, *, resume_game_audit_path: str) -> None:
        self._resume_swarm_audit_path = Path(resume_game_audit_path)
        if not self._resume_swarm_audit_path.is_file():
            raise ValueError(
                f"Swarm resume artifact is missing: {self._resume_swarm_audit_path}"
            )

    async def _restore_swarm(self, environment: BaseEnvironment) -> None:
        source = self._resume_swarm_audit_path
        target = "/var/lib/swarm/audit.jsonl"
        encoded = base64.b64encode(source.read_bytes()).decode("ascii")
        quoted_target = shlex.quote(target)
        result = await environment.service_exec(
            (
                f"umask 077; printf %s {shlex.quote(encoded)} | "
                f"base64 -d > {quoted_target}; "
                f'test "$(wc -c < {quoted_target})" -eq {source.stat().st_size}'
            ),
            service="game",
            user=0,
        )
        if result.return_code != 0:
            raise RuntimeError(
                f"failed to restore Swarm audit {source} to {target}: "
                f"{result.stderr}"
            )
