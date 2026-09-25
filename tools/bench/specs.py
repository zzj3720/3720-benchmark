"""Game run specs (games/<game>/run.toml) and agent profiles (tools/agents/profiles/*.toml)."""

from __future__ import annotations

import tomllib
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

# Checkpoint roles and the task artifact each game stores them in.
GAME_ROLES = ("game_state", "game_audit", "game_events")


@dataclass(frozen=True)
class GameSpec:
    name: str
    task: str
    objective: str
    kwargs: dict[str, Any] = field(default_factory=dict)
    artifacts: dict[str, str] = field(default_factory=dict)

    def render_objective(self, session: str) -> str:
        return self.objective.strip().replace("{session}", session)


@dataclass(frozen=True)
class Profile:
    name: str
    agent: str
    model: str | None
    session: str
    kwargs: dict[str, Any]
    env: list[str]
    agent_hosts: list[str]
    environment_hosts: list[str]
    include_logs: list[str]
    session_dir: str | None
    game_kwargs: bool
    start_agents: dict[str, str]
    resume_agents: dict[str, str]
    raw: dict[str, Any]

    def start_agent(self, game: str) -> str:
        return self.start_agents.get(game, self.agent)

    def resume_agent(self, game: str) -> str:
        agent = self.resume_agents.get(game) or self.resume_agents.get("default")
        if not agent:
            raise SystemExit(f"profile {self.name} has no resume agent for {game}")
        return agent


def load_game(root: Path, name: str) -> GameSpec:
    path = root / "games" / name / "run.toml"
    if not path.is_file():
        known = sorted(p.parent.name for p in (root / "games").glob("*/run.toml"))
        raise SystemExit(f"unknown game {name!r}; known: {', '.join(known)}")
    data = tomllib.loads(path.read_text())
    artifacts = data.get("artifacts", {})
    unknown = set(artifacts) - set(GAME_ROLES)
    if unknown:
        raise SystemExit(f"{path}: unknown artifact roles {sorted(unknown)}")
    return GameSpec(
        name=name,
        task=data["task"],
        objective=data["objective"],
        kwargs=data.get("kwargs", {}),
        artifacts=artifacts,
    )


def load_profile(root: Path, name: str) -> Profile:
    path = root / "tools" / "agents" / "profiles" / f"{name}.toml"
    if not path.is_file():
        known = sorted(p.stem for p in path.parent.glob("*.toml"))
        raise SystemExit(f"unknown profile {name!r}; known: {', '.join(known)}")
    data = tomllib.loads(path.read_text())
    return Profile(
        name=name,
        agent=data["agent"],
        model=data.get("model"),
        session=data.get("session", "native"),
        kwargs=data.get("kwargs", {}),
        env=data.get("env", []),
        agent_hosts=data.get("agent_hosts", []),
        environment_hosts=data.get("environment_hosts", []),
        include_logs=data.get("include_logs", []),
        session_dir=data.get("session_dir"),
        game_kwargs=data.get("game_kwargs", False),
        start_agents=data.get("agents", {}),
        resume_agents=data.get("resume", {}),
        raw=data,
    )
