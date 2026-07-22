#!/usr/bin/env python3
"""Read-only live run index for local Harbor game benchmarks."""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import threading
import time
import urllib.parse
import urllib.request
from dataclasses import dataclass
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any


SUPPORTED_TASKS = {
    "parabox-intro": ("Patrick's Parabox", "parabox"),
    "swarm-farming": ("Swarm Farming", "swarm"),
    "sausage-roll": ("Stephen's Sausage Roll", "sausage"),
    "emergency-operator": ("Emergency Operator", "operator"),
}

INFRASTRUCTURE_EXCEPTIONS = {"OutputTokenExceededError"}

EXPERIENCE_MARKERS = {
    "solved": re.compile(r"\bsolved\b|已解", re.IGNORECASE),
    "rejected": re.compile(
        r"\breject(?:ed|ion)?\b|\bdead[ -]?end\b|\binvalidated\b|死路|不可行",
        re.IGNORECASE,
    ),
    "verified": re.compile(
        r"\bverified\b|\bconfirmed\b|\breusable\b|\bproves?\b|规律|经验|机制",
        re.IGNORECASE,
    ),
}


def run_command(*args: str, timeout: float = 8) -> str:
    return subprocess.run(
        args,
        check=True,
        capture_output=True,
        text=True,
        timeout=timeout,
    ).stdout


def json_lines(text: str) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for line in text.splitlines():
        try:
            value = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(value, dict):
            rows.append(value)
    return rows


def iso_timestamp(value: str | None) -> int | None:
    if not value:
        return None
    try:
        return int(datetime.fromisoformat(value.replace("Z", "+00:00")).timestamp() * 1000)
    except ValueError:
        return None


def safe_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text())
    except (OSError, json.JSONDecodeError):
        return {}
    return value if isinstance(value, dict) else {}


def parse_experience_notes(texts: list[str]) -> dict[str, list[str]]:
    """Project explicit Markdown notes without interpreting hidden reasoning."""
    categories: dict[str, list[str]] = {
        "plan": [],
        "verified": [],
        "rejected": [],
        "solved": [],
    }
    for text in texts:
        heading = ""
        solved_list = False
        solved_level = False
        for raw_line in text.splitlines():
            line = raw_line.strip()
            if line.startswith("#"):
                heading = line.lstrip("#").strip().lower()
                solved_list = any(
                    marker in heading for marker in ("solved this run", "solved levels", "已解关卡")
                )
                solved_level = bool(EXPERIENCE_MARKERS["solved"].search(heading)) and not solved_list
                if solved_level:
                    normalized_heading = re.sub(r"\s+", " ", line.lstrip("#").strip())[:700]
                    if normalized_heading not in categories["solved"]:
                        categories["solved"].append(normalized_heading)
                continue
            match = re.match(r"^[-*+]\s+(.+)$", line)
            if not match:
                continue
            item = match.group(1).strip()
            combined = f"{heading} {item}".lower()
            if EXPERIENCE_MARKERS["solved"].search(item) or solved_list:
                category = "solved"
            elif EXPERIENCE_MARKERS["rejected"].search(item) or EXPERIENCE_MARKERS[
                "rejected"
            ].search(heading):
                category = "rejected"
            elif EXPERIENCE_MARKERS["verified"].search(item) or EXPERIENCE_MARKERS[
                "verified"
            ].search(heading) or any(
                marker in heading for marker in ("mechanic", "verified", "rule", "规律", "经验")
            ):
                category = "verified"
            elif any(
                marker in combined
                for marker in (
                    "active",
                    "lead",
                    "plan",
                    "unsolved",
                    "remaining",
                    "next",
                    "partial",
                    "in progress",
                    "near-complete",
                )
            ):
                category = "plan"
            else:
                category = "verified"
            normalized = re.sub(r"\s+", " ", item)[:700]
            if normalized not in categories[category]:
                categories[category].append(normalized)
    return categories


def subscription_identity(value: Any) -> Any:
    if isinstance(value, dict):
        return {key: subscription_identity(item) for key, item in value.items() if key != "consumed_ms"}
    if isinstance(value, list):
        return [subscription_identity(item) for item in value]
    return value


def model_label(job_name: str, model: str) -> str:
    lower = f"{job_name} {model}".lower()
    labels = [
        ("deepseek-flash", "DeepSeek V4 Flash"),
        ("deepseek-pro", "DeepSeek V4 Pro"),
        ("gpt-5.6-sol", "GPT-5.6 Sol"),
        ("gpt-sol", "GPT-5.6 Sol"),
        ("gpt-5.6-terra", "GPT-5.6 Terra"),
        ("gpt-terra", "GPT-5.6 Terra"),
        ("gpt-5.6-luna", "GPT-5.6 Luna"),
        ("gpt-luna", "GPT-5.6 Luna"),
        ("glm-5-2", "GLM-5.2"),
        ("qwen3-8", "Qwen3.8"),
        ("k3[1m]", "Kimi K3 1M"),
        ("kimi-k3", "Kimi K3 1M"),
    ]
    for marker, label in labels:
        if marker in lower:
            return label
    return model.rsplit("/", 1)[-1] or "Unknown model"


def task_from_config(config: dict[str, Any]) -> str | None:
    tasks = config.get("tasks")
    if not isinstance(tasks, list) or not tasks:
        return None
    path = tasks[0].get("path") if isinstance(tasks[0], dict) else None
    if not isinstance(path, str):
        return None
    task = path.rstrip("/").rsplit("/", 1)[-1]
    return task if task in SUPPORTED_TASKS else None


def agent_info(config: dict[str, Any], job_name: str) -> dict[str, str]:
    agents = config.get("agents")
    agent = agents[0] if isinstance(agents, list) and agents else {}
    if not isinstance(agent, dict):
        agent = {}
    kwargs = agent.get("kwargs") if isinstance(agent.get("kwargs"), dict) else {}
    model = str(agent.get("model_name") or "unknown")
    return {
        "model": model_label(job_name, model),
        "model_id": model,
        "agent": str(agent.get("name") or "unknown").split(":")[-1],
        "effort": str(kwargs.get("reasoning_effort") or "default"),
    }


@dataclass
class RunSource:
    run_id: str
    job_name: str
    task_id: str
    trial_name: str
    config: dict[str, Any]
    active: bool
    events_path: Path | None = None
    game_container: str | None = None
    main_container: str | None = None
    started_at: int | None = None
    finished_at: int | None = None
    jobs_root: Path | None = None


class LiveRepository:
    def __init__(
        self,
        root: Path,
        *,
        standalone_sausage_origin: str | None = "http://127.0.0.1:3733",
        watch: bool = False,
    ):
        self.root = root
        self.jobs = root / ".harbor" / "jobs"
        self.job_roots = (self.jobs, root / "jobs")
        self.standalone_sausage_origin = standalone_sausage_origin
        self._watch = watch
        self._lock = threading.Lock()
        self._changed = threading.Condition()
        self._revision = 0
        self._watchers: set[tuple[str, str]] = set()
        self._cache_at = 0.0
        self._cache: list[dict[str, Any]] = []
        self._sources: dict[str, RunSource] = {}
        self._event_cache: dict[str, tuple[str, list[dict[str, Any]]]] = {}
        if watch:
            threading.Thread(target=self._watch_docker_events, daemon=True).start()
            if standalone_sausage_origin:
                threading.Thread(target=self._watch_sausage_events, daemon=True).start()

    def list_runs(self) -> list[dict[str, Any]]:
        with self._lock:
            if time.monotonic() - self._cache_at < 1.5:
                return self._cache
            sources = self._discover()
            if self._watch:
                self._ensure_watchers(sources)
            summaries: list[dict[str, Any]] = []
            self._sources = {source.run_id: source for source in sources}
            for source in sources:
                try:
                    summaries.append(self._summarize(source, include_detail=False))
                except (OSError, subprocess.SubprocessError, ValueError):
                    continue
            summaries.sort(
                key=lambda item: (
                    item["game"],
                    not item["live"],
                    -int(item.get("score") or 0),
                    item["model"],
                )
            )
            self._cache = summaries
            self._cache_at = time.monotonic()
            return summaries

    def revision(self) -> int:
        with self._changed:
            return self._revision

    def wait_for_change(self, revision: int, timeout: float) -> int:
        with self._changed:
            self._changed.wait_for(lambda: self._revision != revision, timeout)
            return self._revision

    def _notify(self, run_id: str | None = None) -> None:
        with self._lock:
            self._cache_at = 0
            if run_id:
                self._event_cache.pop(run_id, None)
        with self._changed:
            self._revision += 1
            self._changed.notify_all()

    def _ensure_watchers(self, sources: list[RunSource]) -> None:
        for source in sources:
            if not source.active or not source.game_container:
                continue
            self._start_container_watcher(
                source,
                "game",
                ["tail", "-n", "0", "-F", self._container_event_path(source.task_id)],
            )
            if source.main_container:
                self._start_container_watcher(
                    source,
                    "agent",
                    [
                        "sh",
                        "-lc",
                        "while :; do set -- $(find /logs/agent -type f 2>/dev/null | "
                        "grep -E '(\\.jsonl|(claude-code|codex)\\.txt)$'); "
                        "if [ $# -gt 0 ]; then exec tail -n 0 -F \"$@\"; fi; sleep 1; done",
                    ],
                )

    def _start_container_watcher(
        self,
        source: RunSource,
        role: str,
        command: list[str],
    ) -> None:
        key = (source.game_container or "", role)
        if key in self._watchers:
            return
        self._watchers.add(key)
        threading.Thread(
            target=self._follow_container,
            args=(key, source.run_id, source.game_container or "", command),
            daemon=True,
        ).start()

    def _follow_container(
        self,
        key: tuple[str, str],
        run_id: str,
        container: str,
        command: list[str],
    ) -> None:
        try:
            process = subprocess.Popen(
                ["docker", "exec", container, *command],
                stdout=subprocess.PIPE,
                stderr=subprocess.DEVNULL,
                text=True,
                bufsize=1,
            )
            if process.stdout:
                for _line in process.stdout:
                    self._notify(run_id)
            process.wait()
        except OSError:
            pass
        finally:
            with self._lock:
                self._watchers.discard(key)
            self._notify()

    def _watch_docker_events(self) -> None:
        while self._watch:
            try:
                process = subprocess.Popen(
                    [
                        "docker",
                        "events",
                        "--filter",
                        "label=com.docker.compose.service=game",
                        "--format",
                        "{{.Action}}",
                    ],
                    stdout=subprocess.PIPE,
                    stderr=subprocess.DEVNULL,
                    text=True,
                    bufsize=1,
                )
                if process.stdout:
                    for _line in process.stdout:
                        self._notify()
                process.wait()
            except OSError:
                pass
            time.sleep(1)

    def _watch_sausage_events(self) -> None:
        after = 0
        while self._watch and self.standalone_sausage_origin:
            try:
                url = (
                    f"{self.standalone_sausage_origin}/v1/observe/events"
                    f"?after={after}&limit=1000&wait_ms=30000"
                )
                payload = json.loads(urllib.request.urlopen(url, timeout=35).read())
                events = payload.get("events", [])
                if events:
                    after = max(int(event.get("sequence", after)) for event in events)
                    self._notify("sausage-sidecar-only")
            except (OSError, ValueError):
                time.sleep(1)

    def get_run(self, run_id: str) -> dict[str, Any] | None:
        self.list_runs()
        source = self._sources.get(run_id)
        if not source:
            return None
        return self._summarize(source, include_detail=True)

    def _discover(self) -> list[RunSource]:
        active = self._active_sources()
        active_keys = {(item.task_id, agent_info(item.config, item.job_name)["model"]) for item in active}
        archived: dict[tuple[str, str], RunSource] = {}
        for jobs_root in self.job_roots:
            if not jobs_root.exists():
                continue
            for job_dir in jobs_root.iterdir():
                if not job_dir.is_dir():
                    continue
                if any(marker in job_dir.name.lower() for marker in ("nop", "oracle", "control", "smoke", "e2e")):
                    continue
                config = safe_json(job_dir / "config.json")
                task_id = task_from_config(config)
                if not task_id:
                    continue
                info = agent_info(config, job_dir.name)
                key = (task_id, info["model"])
                if key in active_keys:
                    continue
                trial_dirs = [path for path in job_dir.iterdir() if path.is_dir()]
                for trial in trial_dirs:
                    event_path = self._artifact_path(trial, task_id)
                    if not event_path:
                        continue
                    result = safe_json(trial / "result.json")
                    candidate = RunSource(
                        run_id=job_dir.name,
                        job_name=job_dir.name,
                        task_id=task_id,
                        trial_name=trial.name,
                        config=config,
                        active=False,
                        events_path=event_path,
                        started_at=iso_timestamp(result.get("started_at")),
                        finished_at=iso_timestamp(result.get("finished_at")),
                        jobs_root=jobs_root,
                    )
                    existing = archived.get(key)
                    candidate_time = candidate.finished_at or int(event_path.stat().st_mtime * 1000)
                    existing_time = (
                        existing.finished_at
                        if existing and existing.finished_at
                        else int(existing.events_path.stat().st_mtime * 1000)
                        if existing and existing.events_path
                        else 0
                    )
                    if candidate_time >= existing_time:
                        archived[key] = candidate
        return active + list(archived.values()) + self._standalone_sausage(active)

    def _active_sources(self) -> list[RunSource]:
        try:
            names = run_command(
                "docker",
                "ps",
                "--filter",
                "label=com.docker.compose.service=game",
                "--format",
                "{{.Names}}",
            ).splitlines()
        except subprocess.SubprocessError:
            return []
        sources: list[RunSource] = []
        for name in names:
            if "__env-game-1" not in name:
                continue
            trial_name = name.removesuffix("__env-game-1")
            task_id = trial_name.split("__", 1)[0]
            if task_id not in SUPPORTED_TASKS:
                continue
            matches = [
                trial
                for jobs_root in self.job_roots
                if jobs_root.exists()
                for job in jobs_root.iterdir()
                if job.is_dir()
                for trial in job.iterdir()
                if trial.is_dir() and trial.name.lower() == trial_name.lower()
            ]
            if not matches:
                continue
            trial = matches[0]
            job_dir = trial.parent
            config = safe_json(job_dir / "config.json")
            inspect = json.loads(run_command("docker", "inspect", name))[0]
            started = iso_timestamp(inspect.get("State", {}).get("StartedAt"))
            sources.append(
                RunSource(
                    run_id=job_dir.name,
                    job_name=job_dir.name,
                    task_id=task_id,
                    trial_name=trial_name,
                    config=config,
                    active=True,
                    game_container=name,
                    main_container=name.replace("__env-game-1", "__env-main-1"),
                    started_at=started,
                    jobs_root=job_dir.parent,
                )
            )
        return sources

    def _standalone_sausage(self, active: list[RunSource]) -> list[RunSource]:
        if not self.standalone_sausage_origin or any(source.task_id == "sausage-roll" for source in active):
            return []
        try:
            snapshot = json.loads(
                urllib.request.urlopen(
                    f"{self.standalone_sausage_origin}/v1/observe/snapshot?include_map=0",
                    timeout=1,
                ).read()
            )
        except (OSError, ValueError):
            return []
        snapshot_events = snapshot.get("events")
        started_at = (
            snapshot_events[0].get("timestamp_ms")
            if isinstance(snapshot_events, list)
            and snapshot_events
            and isinstance(snapshot_events[0], dict)
            else None
        )
        return [
            RunSource(
                run_id="sausage-sidecar-only",
                job_name="sausage-sidecar-only",
                task_id="sausage-roll",
                trial_name="live-sausage",
                config={
                    "agents": [
                        {
                            "name": "sidecar-only",
                            "model_name": "No agent attached",
                            "kwargs": {},
                        }
                    ]
                },
                active=True,
                started_at=started_at,
            )
        ]

    @staticmethod
    def _artifact_path(trial: Path, task_id: str) -> Path | None:
        relative = {
            "parabox-intro": "var/lib/parabox/parabox-events.jsonl",
            "swarm-farming": "var/lib/swarm/audit.jsonl",
            "sausage-roll": "var/lib/sausage/sausage-events.jsonl",
            "emergency-operator": "var/lib/operator/events.jsonl",
        }[task_id]
        path = trial / "artifacts" / relative
        return path if path.exists() else None

    def _events(self, source: RunSource) -> list[dict[str, Any]]:
        if source.run_id == "sausage-sidecar-only":
            url = (
                f"{self.standalone_sausage_origin}"
                "/v1/observe/events?after=0&limit=1000&wait_ms=0"
            )
            payload = json.loads(urllib.request.urlopen(url, timeout=2).read())
            return payload.get("events", [])
        if source.active and source.game_container:
            stat = run_command(
                "docker",
                "exec",
                source.game_container,
                "stat",
                "-c",
                "%s:%Y",
                self._container_event_path(source.task_id),
            ).strip()
            cached = self._event_cache.get(source.run_id)
            if cached and cached[0] == stat:
                return cached[1]
            text = run_command(
                "docker",
                "exec",
                source.game_container,
                "cat",
                self._container_event_path(source.task_id),
                timeout=15,
            )
            rows = json_lines(text)
            self._event_cache[source.run_id] = (stat, rows)
            return rows
        if source.events_path:
            stat = f"{source.events_path.stat().st_size}:{source.events_path.stat().st_mtime_ns}"
            cached = self._event_cache.get(source.run_id)
            if cached and cached[0] == stat:
                return cached[1]
            rows = json_lines(source.events_path.read_text())
            self._event_cache[source.run_id] = (stat, rows)
            return rows
        return []

    @staticmethod
    def _container_event_path(task_id: str) -> str:
        return {
            "parabox-intro": "/var/lib/parabox/parabox-events.jsonl",
            "swarm-farming": "/var/lib/swarm/audit.jsonl",
            "sausage-roll": "/var/lib/sausage/sausage-events.jsonl",
            "emergency-operator": "/var/lib/operator/events.jsonl",
        }[task_id]

    def _summarize(self, source: RunSource, *, include_detail: bool) -> dict[str, Any]:
        events = self._events(source)
        normalized = self._normalize_events(source.task_id, events)
        latest = next(
            (
                event
                for event in reversed(normalized)
                if isinstance(event.get("state"), dict) and event.get("state")
            ),
            normalized[-1] if normalized else {},
        )
        state = latest.get("state") if isinstance(latest.get("state"), dict) else {}
        overworld_map = None
        if source.task_id == "sausage-roll" and state:
            overworld_map = next(
                (
                    event_state["overworld_map"]
                    for event in reversed(normalized)
                    if isinstance((event_state := event.get("state")), dict)
                    and isinstance(event_state.get("overworld_map"), dict)
                ),
                None,
            )
            if overworld_map is not None:
                state = {**state, "overworld_map": overworld_map}
        if source.task_id == "parabox-intro" and not state:
            reference = str(latest.get("selected") or "")
            score = int(latest.get("score") or 0)
            state = {
                "status": "in_progress",
                "campaign": {"score": score, "solved": score, "total": int(latest.get("total") or 364)},
                "level": {"reference": reference, "title": self._parabox_title(reference)},
            }
        info = agent_info(source.config, source.job_name)
        score, total, objective = self._score(source.task_id, state)
        history = self._score_history(normalized, score)
        if not history and (source.started_at or source.finished_at):
            history = [{"timestamp_ms": source.finished_at or source.started_at, "score": score}]
        history, consumed_ms = self._effective_timeline(source, normalized, history)
        last_score_at = next(
            (
                event.get("timestamp_ms")
                for event in reversed(normalized)
                if (event.get("score_delta") or 0) > 0
            ),
            None,
        )
        task_label, game = SUPPORTED_TASKS[source.task_id]
        result: dict[str, Any] = {
            "id": source.run_id,
            "job": source.job_name,
            "trial": source.trial_name,
            "task_id": source.task_id,
            "task": task_label,
            "game": game,
            **info,
            "live": source.active and source.run_id != "sausage-sidecar-only",
            "status": "live" if source.active else "finished",
            "sidecar_only": source.run_id == "sausage-sidecar-only",
            "score": score,
            "total": total,
            "objective": objective,
            "started_at": source.started_at or (history[0]["timestamp_ms"] if history else None),
            "finished_at": source.finished_at,
            "last_activity_at": latest.get("timestamp_ms"),
            "last_score_at": last_score_at,
            "consumed_ms": consumed_ms,
            "latest_sequence": latest.get("sequence", len(normalized)),
            "latest_action": latest.get("action"),
            "latest_result": latest.get("result"),
            "score_history": history,
        }
        if include_detail:
            detail_events = normalized[-120:]
            if overworld_map is not None:
                detail_events = [
                    {
                        **event,
                        "state": {
                            key: value
                            for key, value in event["state"].items()
                            if key != "overworld_map"
                        },
                    }
                    if isinstance(event.get("state"), dict)
                    else event
                    for event in detail_events
                ]
            result.update(
                {
                    "state": state,
                    "events": detail_events,
                    "agent_activity": self._agent_activity(source),
                    "agent_experience": self._agent_experience(source),
                }
            )
        return result

    def _effective_timeline(
        self,
        source: RunSource,
        events: list[dict[str, Any]],
        history: list[dict[str, int]],
    ) -> tuple[list[dict[str, int]], int]:
        now = int(time.time() * 1000)
        windows: list[tuple[int, int]] = []
        current_trial = (source.jobs_root or self.jobs) / source.job_name / source.trial_name
        for trial in self._continuation_trials(source):
            if source.active and trial == current_trial:
                if source.started_at:
                    windows.append((source.started_at, now))
                continue
            result = safe_json(trial / "result.json")
            execution = result.get("agent_execution")
            if not isinstance(execution, dict):
                continue
            started = iso_timestamp(execution.get("started_at"))
            finished = iso_timestamp(execution.get("finished_at"))
            if started is None or finished is None or finished <= started:
                continue
            exception = result.get("exception_info")
            exception_type = exception.get("exception_type") if isinstance(exception, dict) else None
            has_game_event = any(
                isinstance(event.get("timestamp_ms"), int)
                and started <= event["timestamp_ms"] <= finished
                for event in events
            )
            if exception_type in INFRASTRUCTURE_EXCEPTIONS and not has_game_event:
                continue
            windows.append((started, finished))

        if not windows:
            timestamps = [point["timestamp_ms"] for point in history]
            if not timestamps:
                return history, 0
            started = min(source.started_at or timestamps[0], timestamps[0])
            finished = now if source.active else source.finished_at or timestamps[-1]
            windows = [(started, max(started, finished))]

        def elapsed_at(timestamp: int) -> int:
            elapsed = 0
            for started, finished in windows:
                if timestamp < started:
                    return elapsed
                elapsed += max(0, min(timestamp, finished) - started)
                if timestamp <= finished:
                    return elapsed
            return elapsed

        consumed_ms = sum(finished - started for started, finished in windows)
        return [
            {**point, "elapsed_ms": elapsed_at(point["timestamp_ms"])} for point in history
        ], consumed_ms

    def _continuation_trials(self, source: RunSource) -> list[Path]:
        trial = (source.jobs_root or self.jobs) / source.job_name / source.trial_name
        chain: list[Path] = []
        seen: set[Path] = set()
        while trial not in seen and trial.exists():
            seen.add(trial)
            chain.append(trial)
            config = safe_json(trial.parent / "config.json")
            previous = self._resume_trial(config, source.jobs_root or self.jobs)
            if previous is None:
                break
            trial = previous
        chain.reverse()
        return chain

    @staticmethod
    def _resume_trial(config: dict[str, Any], jobs_root: Path) -> Path | None:
        agents = config.get("agents")
        agent = agents[0] if isinstance(agents, list) and agents else {}
        kwargs = agent.get("kwargs") if isinstance(agent, dict) else None
        if not isinstance(kwargs, dict):
            return None
        raw_path = kwargs.get("resume_game_events_path") or kwargs.get("resume_game_state_path")
        if not isinstance(raw_path, str):
            return None
        path = Path(raw_path)
        artifact_trial = next(
            (parent.parent for parent in path.parents if parent.name == "artifacts"),
            None,
        )
        if artifact_trial is not None:
            return artifact_trial

        recovery = next(
            (parent for parent in path.parents if parent.parent.name == "recoveries"),
            None,
        )
        if recovery is not None:
            job = jobs_root / recovery.name
            if job.is_dir():
                trials = [candidate for candidate in job.iterdir() if candidate.is_dir()]
                if len(trials) == 1:
                    return trials[0]

        checkpoint = next(
            (parent for parent in path.parents if (parent / "manifest.json").is_file()),
            None,
        )
        if checkpoint is None:
            return None
        manifest = safe_json(checkpoint / "manifest.json")
        job = manifest.get("job")
        trial = manifest.get("trial")
        if (
            not isinstance(job, str)
            or not isinstance(trial, str)
            or Path(job).name != job
            or Path(trial).name != trial
        ):
            return None
        candidate = jobs_root / job / trial
        return candidate if candidate.is_dir() else None

    @staticmethod
    def _normalize_events(task_id: str, rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
        if task_id == "swarm-farming":
            normalized = []
            for index, row in enumerate(rows, 1):
                response = row.get("response") if isinstance(row.get("response"), dict) else {}
                state = response.get("data") if isinstance(response.get("data"), dict) else {}
                normalized.append(
                    {
                        "sequence": row.get("sequence", index),
                        "timestamp_ms": row.get("timestamp_ms"),
                        "type": "command",
                        "action": {"command": row.get("command"), "argument": row.get("argument")},
                        "state": state,
                        "result": {
                            "ok": response.get("ok", False),
                            "error": response.get("error"),
                        },
                        "score": state.get("score", 0),
                        "score_delta": 0,
                    }
                )
            return normalized
        if (
            task_id in {"sausage-roll", "emergency-operator"}
            and rows
            and rows[0].get("schema") == "benchmark-observer-event-v1"
        ):
            return rows
        normalized = []
        for index, row in enumerate(rows, 1):
            state = row.get("state")
            scene = row.get("scene")
            if isinstance(state, dict) and isinstance(scene, dict):
                state = {**state, "observer_scene": scene}
            normalized.append(
                {
                    "sequence": index,
                    "timestamp_ms": row.get("timestamp_ms"),
                    "type": "command" if row.get("command") else row.get("type", "state"),
                    "action": {
                        "command": row.get("command"),
                        "argument_count": row.get("argument_count"),
                    }
                    if row.get("command")
                    else None,
                    "state": state,
                    "result": {
                        "ok": row.get("ok", True),
                        "code": row.get("code"),
                        "solved_levels": row.get("solved_levels", []),
                        "score_delta": row.get("score_delta", 0),
                    },
                    "score": row.get("score", 0),
                    "score_delta": row.get("score_delta", 0),
                    "selected": row.get("selected"),
                    "total": row.get("total", 364),
                }
            )
        return normalized

    def _parabox_title(self, reference: str) -> str:
        if not reference:
            return "Waiting for state"
        index = self.root / "tasks" / "parabox-intro" / "environment" / "game" / "campaign" / "index.tsv"
        try:
            for line in index.read_text().splitlines():
                fields = line.split("\t")
                if len(fields) > 1 and fields[0] == reference:
                    return fields[1]
        except OSError:
            pass
        return reference

    @staticmethod
    def _score(task_id: str, state: dict[str, Any]) -> tuple[int, int, str]:
        if task_id == "emergency-operator":
            campaign = state.get("campaign") if isinstance(state.get("campaign"), dict) else {}
            shift = state.get("shift") if isinstance(state.get("shift"), dict) else {}
            active_call = state.get("active_call")
            calls = state.get("calls") if isinstance(state.get("calls"), list) else []
            ringing = [call for call in calls if isinstance(call, dict) and call.get("status") == "ringing"]
            if isinstance(active_call, str):
                objective = f"Active call {active_call}"
            elif ringing:
                objective = f"Answer {ringing[0].get('id', 'ringing call')}"
            elif shift.get("status") == "not_started":
                objective = "Start shift"
            elif shift.get("status") == "complete":
                objective = "Shift complete"
            else:
                remaining_minutes = max(0, int(shift.get("remaining_ms", 0) or 0) // 60_000)
                objective = f"Monitor dispatch / {remaining_minutes}m left"
            return (
                int(campaign.get("score", 0) or 0),
                int(campaign.get("max_score", 0) or 0),
                objective,
            )
        if task_id in {"parabox-intro", "sausage-roll"}:
            campaign = state.get("campaign") if isinstance(state.get("campaign"), dict) else {}
            level = state.get("level") if isinstance(state.get("level"), dict) else {}
            reference = level.get("reference") or level.get("id") or ""
            title = level.get("title") or state.get("status") or "Waiting for state"
            objective = f"{reference} / {title}" if reference else str(title)
            return int(campaign.get("score", campaign.get("solved", 0)) or 0), int(campaign.get("total", 0) or 0), objective
        objectives = json.dumps(state.get("objectives", {}), ensure_ascii=False)
        completed = '"get_many_lambdas"' in objectives and '"_completedIDs": ["get_many_lambdas"]' in objectives
        objective = "Make curry" if completed else "Get 256 lambdas"
        return int(state.get("score", 0) or 0), 1, objective

    @staticmethod
    def _score_history(events: list[dict[str, Any]], final_score: int) -> list[dict[str, int]]:
        points: list[dict[str, int]] = []
        previous: int | None = None
        for event in events:
            timestamp = event.get("timestamp_ms")
            score = event.get("score")
            if not isinstance(timestamp, int) or not isinstance(score, int):
                continue
            if previous is None or score != previous:
                points.append({"timestamp_ms": timestamp, "score": score})
                previous = score
        if not points and events:
            timestamp = events[-1].get("timestamp_ms")
            if isinstance(timestamp, int):
                points.append({"timestamp_ms": timestamp, "score": final_score})
        return points[-500:]

    def _agent_activity(self, source: RunSource) -> list[dict[str, Any]]:
        if not source.active or not source.main_container:
            return []
        try:
            output = run_command(
                "docker",
                "exec",
                source.main_container,
                "sh",
                "-lc",
                "find /logs/agent -type f 2>/dev/null | "
                "grep -E '(\\.jsonl|(claude-code|codex)\\.txt)$' | "
                "while read -r f; do tail -n 700 \"$f\"; done",
                timeout=12,
            )
        except subprocess.SubprocessError:
            return []
        messages: list[dict[str, Any]] = []
        for row in json_lines(output):
            timestamp = row.get("timestamp") or row.get("created_at")
            payload = row.get("payload") if isinstance(row.get("payload"), dict) else row
            text = self._message_text(payload)
            if text:
                messages.append({"timestamp": timestamp, "text": text[:2400]})
        deduplicated: list[dict[str, Any]] = []
        for message in messages:
            if not deduplicated or deduplicated[-1]["text"] != message["text"]:
                deduplicated.append(message)
        return deduplicated[-12:]

    def _agent_experience(self, source: RunSource) -> dict[str, Any]:
        workspaces: list[Path] = []
        current = (source.jobs_root or self.jobs) / source.job_name / source.trial_name
        for trial in [*self._continuation_trials(source), current]:
            workspaces.append(trial / "agent" / "workspace")
            config = safe_json(trial.parent / "config.json")
            agents = config.get("agents")
            agent = agents[0] if isinstance(agents, list) and agents else {}
            kwargs = agent.get("kwargs") if isinstance(agent, dict) else {}
            resumed = kwargs.get("resume_workspace_dir") if isinstance(kwargs, dict) else None
            if isinstance(resumed, str):
                workspaces.append(Path(resumed))

        notes: list[Path] = []
        for workspace in dict.fromkeys(workspaces):
            try:
                for path in workspace.iterdir():
                    lower = path.name.lower()
                    if path.is_file() and path.suffix.lower() == ".md" and (
                        "note" in lower or "plan" in lower
                    ):
                        notes.append(path)
            except OSError:
                continue

        readable: list[tuple[float, Path, str]] = []
        for path in dict.fromkeys(notes):
            try:
                readable.append((path.stat().st_mtime, path, path.read_text(errors="replace")))
            except OSError:
                continue
        readable.sort(key=lambda item: item[0])
        texts = [text for _, _, text in readable]
        categories = parse_experience_notes(texts)
        return {
            "updated_at": max((int(mtime * 1000) for mtime, _, _ in readable), default=None),
            "source_count": len(readable),
            "counts": {key: len(items) for key, items in categories.items()},
            **{key: items[-8:] for key, items in categories.items()},
        }

    @staticmethod
    def _message_text(payload: dict[str, Any]) -> str | None:
        item = payload.get("item")
        if (
            payload.get("type") == "item.completed"
            and isinstance(item, dict)
            and item.get("type") == "agent_message"
        ):
            text = item.get("text")
            return text.strip() if isinstance(text, str) and text.strip() else None
        if payload.get("type") == "message":
            content = payload.get("content")
            if isinstance(content, list):
                texts = [
                    item.get("text", "")
                    for item in content
                    if isinstance(item, dict) and item.get("type") in {"output_text", "text"}
                ]
                return "\n".join(text for text in texts if text).strip() or None
        if payload.get("type") == "assistant":
            message = payload.get("message")
            content = message.get("content") if isinstance(message, dict) else None
            if isinstance(content, list):
                texts = [item.get("text", "") for item in content if isinstance(item, dict) and item.get("type") == "text"]
                return "\n".join(text for text in texts if text).strip() or None
        message = payload.get("message")
        return message.strip() if isinstance(message, str) and message.strip() else None


class Handler(BaseHTTPRequestHandler):
    repository: LiveRepository
    protocol_version = "HTTP/1.1"

    def do_GET(self) -> None:  # noqa: N802
        parsed = urllib.parse.urlparse(self.path)
        path = parsed.path.rstrip("/") or "/"
        if path == "/health":
            self._json({"ok": True})
            return
        if path == "/v1/runs":
            self._json({"schema": "benchmark-live-runs-v1", "generated_at": int(time.time() * 1000), "runs": self.repository.list_runs()})
            return
        if path == "/v1/subscribe":
            query = urllib.parse.parse_qs(parsed.query)
            run_id = query.get("run_id", [None])[0]
            self._subscribe(run_id)
            return
        if path.startswith("/v1/runs/"):
            run_id = urllib.parse.unquote(path.removeprefix("/v1/runs/"))
            run = self.repository.get_run(run_id)
            if run is None:
                self._json({"error": "unknown run"}, 404)
            else:
                self._json({"schema": "benchmark-live-run-v1", "run": run})
            return
        self._json({"error": "read-only endpoint not found"}, 404)

    def _subscribe(self, run_id: str | None) -> None:
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream; charset=utf-8")
        self.send_header("Cache-Control", "no-store, no-transform")
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Connection", "keep-alive")
        self.send_header("X-Accel-Buffering", "no")
        self.end_headers()

        previous: bytes | None = None
        revision = self.repository.revision()
        try:
            while True:
                value: dict[str, Any] = {"runs": self.repository.list_runs()}
                if run_id:
                    value["run"] = self.repository.get_run(run_id)
                encoded = json.dumps(
                    subscription_identity(value),
                    ensure_ascii=False,
                    separators=(",", ":"),
                ).encode()
                if encoded != previous:
                    envelope = json.dumps(
                        {
                            "schema": "benchmark-live-subscription-v1",
                            "generated_at": int(time.time() * 1000),
                            **value,
                        },
                        ensure_ascii=False,
                        separators=(",", ":"),
                    ).encode()
                    self.wfile.write(b"data: " + envelope + b"\n\n")
                    previous = encoded
                self.wfile.flush()
                while True:
                    changed = self.repository.wait_for_change(revision, 15)
                    if changed != revision:
                        revision = changed
                        break
                    self.wfile.write(b": keepalive\n\n")
                    self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError, OSError):
            self.close_connection = True
            return

    def _json(self, value: Any, status: int = 200) -> None:
        body = json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Cache-Control", "no-store")
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, format: str, *args: Any) -> None:
        return


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument("--host", default=os.environ.get("LIVE_GATEWAY_HOST", "127.0.0.1"))
    parser.add_argument("--port", type=int, default=int(os.environ.get("LIVE_GATEWAY_PORT", "3740")))
    parser.add_argument(
        "--sausage-origin",
        default=os.environ.get("SAUSAGE_OBSERVER_ORIGIN", "http://127.0.0.1:3733"),
    )
    args = parser.parse_args()
    Handler.repository = LiveRepository(
        args.root.resolve(),
        standalone_sausage_origin=args.sausage_origin,
        watch=True,
    )
    server = ThreadingHTTPServer((args.host, args.port), Handler)
    print(f"3720 live gateway listening on http://{args.host}:{args.port}", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
