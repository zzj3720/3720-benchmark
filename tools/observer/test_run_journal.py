from __future__ import annotations

import asyncio
import base64
import gzip
import json
import subprocess
import tempfile
import unittest
from datetime import datetime, timedelta, timezone
from pathlib import Path
from types import SimpleNamespace
from uuid import uuid4

from tools.observer.run_journal import RUN_EVENT_SCHEMA, RunJournalPlugin


def journal_rows(path: Path):
    archive = Path(__file__).parent / "runtime/target/release/run-archive"
    decoded = subprocess.check_output([str(archive), "--cat", str(path.parent)], text=True)
    return [json.loads(line) for line in decoded.splitlines()]


class FakeJob:
    def __init__(self, root: Path) -> None:
        self.config = SimpleNamespace(job_name="journal-e2e")
        self.job_dir = root / "jobs" / "journal-e2e"
        self.job_dir.mkdir(parents=True)
        self.id = uuid4()
        self.hooks = {}

    def __len__(self):
        return 1

    def _hook(self, name, callback):
        self.hooks[name] = callback
        return self

    def on_trial_started(self, callback):
        return self._hook("start", callback)

    def on_environment_started(self, callback):
        return self._hook("environment-start", callback)

    def on_agent_started(self, callback):
        return self._hook("agent-start", callback)

    def on_agent_ended(self, callback):
        return self._hook("agent-end", callback)

    def on_verification_started(self, callback):
        return self._hook("verification-start", callback)

    def on_trial_cancelled(self, callback):
        return self._hook("cancel", callback)

    def on_trial_ended(self, callback):
        return self._hook("end", callback)


def event(name, timestamp, trial_id, *, exception=None):
    result = SimpleNamespace(
        exception_info=SimpleNamespace(exception_type=exception) if exception else None,
        verifier_result=SimpleNamespace(rewards={"reward": 1})
        if name == "end"
        else None,
    )
    return SimpleNamespace(
        event=SimpleNamespace(value=name),
        trial_id=trial_id,
        trial_name="parabox-intro__test",
        task_name="parabox-intro",
        timestamp=timestamp,
        config=SimpleNamespace(
            agent=SimpleNamespace(
                model_name="model", name="agent", kwargs={"reasoning_effort": "high"}
            )
        ),
        result=result,
    )


class RunJournalPluginTests(unittest.IsolatedAsyncioTestCase):
    async def test_chain_allows_only_one_active_writer(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            job = FakeJob(root)
            first = RunJournalPlugin(journal_root=str(root / "journals"))
            second = RunJournalPlugin(journal_root=str(root / "journals"))
            await first.on_job_start(job)
            with self.assertRaisesRegex(RuntimeError, "active writer"):
                await second.on_job_start(job)
            await first.on_job_end(None)
            await second.on_job_start(job)
            await second.on_job_end(None)

    async def test_runtime_and_game_records_share_one_effective_timeline(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            job = FakeJob(root)
            plugin = RunJournalPlugin(journal_root=str(root / "journals"))
            await plugin.on_job_start(job)
            trial_id = uuid4()
            start = datetime.now(timezone.utc)
            await job.hooks["start"](event("start", start, trial_id))
            await job.hooks["agent-start"](
                event("agent-start", start + timedelta(seconds=1), trial_id)
            )
            inbox = (
                job.job_dir
                / "parabox-intro__test/artifacts/logs/artifacts/observer/game-inbox.jsonl"
            )
            inbox.write_text(
                json.dumps(
                    {
                        "schema": "benchmark-observer-event-v1",
                        "timestamp_ms": int(
                            (start + timedelta(seconds=3)).timestamp() * 1000
                        ),
                        "score": 1,
                        "score_delta": 1,
                        "state": {"level": {"reference": "a1"}},
                        "scene": {"schema": "test-scene-v1"},
                        "instruction_trace": {
                            "encoding": "gzip+base64",
                            "count": 1,
                            "uncompressed_bytes": len(
                                trace_payload := json.dumps(
                                    [{"state": {"level": {"reference": "a1"}}}],
                                    separators=(",", ":"),
                                ).encode()
                            ),
                            "data": base64.b64encode(
                                gzip.compress(trace_payload)
                            ).decode(),
                        },
                    }
                )
                + "\n"
            )
            agent_dir = job.job_dir / "parabox-intro__test/agent"
            session_dir = agent_dir / "sessions"
            session_dir.mkdir(parents=True)
            (session_dir / "rollout.jsonl").write_text(
                json.dumps(
                    {
                        "timestamp": (
                            start + timedelta(seconds=4)
                        ).isoformat(),
                        "payload": {
                            "type": "item.completed",
                            "item": {
                                "type": "agent_message",
                                "text": "Try the lower entrance next.",
                            },
                        },
                    }
                )
                + "\n"
            )
            workspace = agent_dir / "workspace"
            workspace.mkdir()
            (workspace / "notes.md").write_text(
                "# Verified mechanics\n- Boxes preserve direction.\n"
            )
            await asyncio.sleep(0.5)
            journal = root / "journals/journal-e2e/journal.jsonl"
            live_rows = journal_rows(journal)
            self.assertTrue(any(row["source"] == "game" for row in live_rows))
            await job.hooks["agent-end"](
                event("agent-end", start + timedelta(seconds=5), trial_id)
            )
            await job.hooks["end"](event("end", start + timedelta(seconds=6), trial_id))
            await plugin.on_job_end(None)

            rows = journal_rows(journal)
            self.assertTrue(all(row["schema"] == RUN_EVENT_SCHEMA for row in rows))
            self.assertEqual(
                [row["sequence"] for row in rows], list(range(1, len(rows) + 1))
            )
            game = next(row for row in rows if row["source"] == "game")
            self.assertEqual(game["type"], "score_changed")
            self.assertEqual(game["effective_elapsed_ms"], 2_000)
            trace = game["payload"]["instruction_trace"]
            self.assertEqual(trace["encoding"], "zstd")
            self.assertNotIn("data", trace)
            snapshot = game["payload"]["state_snapshot"]
            self.assertEqual(snapshot["encoding"], "zstd")
            self.assertNotIn("state", game["payload"])
            self.assertNotIn("scene", game["payload"])
            self.assertEqual(len(list((journal.parent / "objects").iterdir())), 2)
            self.assertTrue(
                any(
                    row["source"] == "agent"
                    and row["type"] == "agent_message"
                    and row["payload"]["text"] == "Try the lower entrance next."
                    for row in rows
                )
            )
            self.assertTrue(
                any(
                    row["source"] == "agent"
                    and row["type"] == "experience_updated"
                    and "Boxes preserve direction." in row["payload"]["markdown"]
                    for row in rows
                )
            )
            self.assertEqual(inbox.read_bytes(), b"")
            finished = next(row for row in rows if row["type"] == "segment_finished")
            self.assertEqual(finished["effective_elapsed_ms"], 4_000)
            manifest = json.loads((inbox.parent / "manifest.json").read_text())
            self.assertEqual(manifest["chain_id"], "journal-e2e")
            self.assertEqual(manifest["segment_id"], str(trial_id))

    async def test_reused_chain_keeps_sequence_and_elapsed_offset(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            journal = root / "journals/shared/journal.jsonl"
            journal.parent.mkdir(parents=True)
            journal.write_text(
                json.dumps(
                    {
                        "schema": RUN_EVENT_SCHEMA,
                        "sequence": 7,
                        "effective_elapsed_ms": 12_000,
                        "segment_id": "parent",
                        "type": "segment_finished",
                    }
                )
                + "\n"
            )
            job = FakeJob(root)
            plugin = RunJournalPlugin(
                chain_id="shared", journal_root=str(root / "journals")
            )
            await plugin.on_job_start(job)
            trial_id = uuid4()
            now = datetime.now(timezone.utc)
            await job.hooks["start"](event("start", now, trial_id))
            await job.hooks["end"](event("end", now, trial_id))
            await plugin.on_job_end(None)
            rows = journal_rows(journal)
            self.assertEqual(rows[-2]["sequence"], 8)
            self.assertEqual(rows[-2]["effective_elapsed_ms"], 12_000)
            self.assertEqual(rows[-2]["payload"]["parent_segment_id"], "parent")


if __name__ == "__main__":
    unittest.main()
