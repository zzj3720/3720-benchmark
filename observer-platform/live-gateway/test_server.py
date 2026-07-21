import importlib.util
import json
import sys
import tempfile
import threading
import unittest
import urllib.request
from pathlib import Path


SPEC = importlib.util.spec_from_file_location("live_gateway", Path(__file__).with_name("server.py"))
gateway = importlib.util.module_from_spec(SPEC)
assert SPEC.loader
sys.modules[SPEC.name] = gateway
SPEC.loader.exec_module(gateway)


class LiveGatewayTests(unittest.TestCase):
    def test_subscription_does_not_emit_for_elapsed_time_alone(self):
        before = {"runs": [{"id": "run-1", "score": 3, "consumed_ms": 1000}]}
        after = {"runs": [{"id": "run-1", "score": 3, "consumed_ms": 2000}]}
        self.assertEqual(
            gateway.subscription_identity(before),
            gateway.subscription_identity(after),
        )

    def test_subscription_sends_runs_and_selected_detail_as_sse(self):
        class Repository:
            def revision(self):
                return 0

            def wait_for_change(self, revision, timeout):
                return revision

            def list_runs(self):
                return [{"id": "run-1", "score": 3}]

            def get_run(self, run_id):
                return {"id": run_id, "events": [{"sequence": 1}]}

        gateway.Handler.repository = Repository()
        server = gateway.ThreadingHTTPServer(("127.0.0.1", 0), gateway.Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            with urllib.request.urlopen(
                f"http://127.0.0.1:{server.server_port}/v1/subscribe?run_id=run-1",
                timeout=2,
            ) as response:
                self.assertEqual(response.headers.get_content_type(), "text/event-stream")
                line = response.readline()
                self.assertTrue(line.startswith(b"data: "))
                payload = json.loads(line.removeprefix(b"data: "))
                self.assertEqual(payload["schema"], "benchmark-live-subscription-v1")
                self.assertEqual(payload["runs"][0]["score"], 3)
                self.assertEqual(payload["run"]["id"], "run-1")
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=2)

    def test_archived_parabox_run_preserves_score_history_without_full_states(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            index = root / "tasks/parabox-intro/environment/game/campaign/index.tsv"
            index.parent.mkdir(parents=True)
            index.write_text("a1\tFirst Steps\ta1.level\tArea\tcore\t-\t1\n")
            job = root / ".harbor/jobs/parabox-gpt-sol-xhigh-r1"
            trial = job / "parabox-intro__abc/artifacts/var/lib/parabox"
            trial.mkdir(parents=True)
            (job / "config.json").write_text(json.dumps({
                "agents": [{"name": "codex", "model_name": "openai/gpt-5.6-sol", "kwargs": {"reasoning_effort": "xhigh"}}],
                "tasks": [{"path": "tasks/parabox-intro"}],
            }))
            started_at = 1_767_225_600_000
            events = [
                {"timestamp_ms": started_at, "command": "move", "score": 0, "score_delta": 0, "selected": "a1", "total": 364},
                {"timestamp_ms": started_at + 1000, "command": "move", "score": 1, "score_delta": 1, "selected": "a1", "total": 364},
            ]
            (trial / "parabox-events.jsonl").write_text("\n".join(json.dumps(row) for row in events))
            result_dir = job / "parabox-intro__abc"
            (result_dir / "result.json").write_text(json.dumps({"started_at": "2026-01-01T00:00:00Z", "finished_at": "2026-01-01T01:00:00Z"}))

            repository = gateway.LiveRepository(root, standalone_sausage_origin=None)
            runs = repository.list_runs()

            self.assertEqual(len(runs), 1)
            self.assertEqual(runs[0]["model"], "GPT-5.6 Sol")
            self.assertEqual(runs[0]["score"], 1)
            self.assertEqual(runs[0]["objective"], "a1 / First Steps")
            self.assertEqual(runs[0]["consumed_ms"], 3_600_000)
            self.assertEqual(runs[0]["score_history"], [
                {"timestamp_ms": started_at, "elapsed_ms": 0, "score": 0},
                {"timestamp_ms": started_at + 1000, "elapsed_ms": 1000, "score": 1},
            ])

    def test_continuation_timeline_counts_only_agent_execution(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            jobs = root / ".harbor/jobs"
            previous = jobs / "parabox-kimi-r1"
            previous_trial = previous / "parabox-intro__one"
            infrastructure = jobs / "parabox-kimi-r-infra"
            infrastructure_trial = infrastructure / "parabox-intro__infra"
            current = jobs / "parabox-kimi-r2"
            current_trial = current / "parabox-intro__two"
            events_dir = current_trial / "artifacts/var/lib/parabox"
            events_dir.mkdir(parents=True)
            previous_trial.mkdir(parents=True)
            common = {
                "agents": [{"name": "claude-code", "model_name": "k3[1m]", "kwargs": {}}],
                "tasks": [{"path": "tasks/parabox-intro"}],
            }
            (previous / "config.json").write_text(json.dumps(common))
            after_previous = json.loads(json.dumps(common))
            after_previous["agents"][0]["kwargs"]["resume_game_events_path"] = str(
                previous_trial / "artifacts/var/lib/parabox/parabox-events.jsonl"
            )
            infrastructure_trial.mkdir(parents=True)
            (infrastructure / "config.json").write_text(json.dumps(after_previous))
            resumed = json.loads(json.dumps(common))
            resumed["agents"][0]["kwargs"]["resume_game_events_path"] = str(
                infrastructure_trial / "artifacts/var/lib/parabox/parabox-events.jsonl"
            )
            (current / "config.json").write_text(json.dumps(resumed))
            (previous_trial / "result.json").write_text(json.dumps({
                "agent_execution": {
                    "started_at": "1970-01-01T00:00:01Z",
                    "finished_at": "1970-01-01T00:00:04Z",
                }
            }))
            (infrastructure_trial / "result.json").write_text(json.dumps({
                "agent_execution": {
                    "started_at": "1970-01-01T00:00:05Z",
                    "finished_at": "1970-01-01T00:00:08Z",
                },
                "exception_info": {"exception_type": "OutputTokenExceededError"},
            }))
            (current_trial / "result.json").write_text(json.dumps({
                "started_at": "1970-01-01T00:00:10Z",
                "finished_at": "1970-01-01T00:00:14Z",
                "agent_execution": {
                    "started_at": "1970-01-01T00:00:10Z",
                    "finished_at": "1970-01-01T00:00:14Z",
                }
            }))
            events = [
                {"timestamp_ms": 2000, "command": "move", "score": 0},
                {"timestamp_ms": 12000, "command": "move", "score": 1, "score_delta": 1},
            ]
            (events_dir / "parabox-events.jsonl").write_text(
                "\n".join(json.dumps(row) for row in events)
            )

            run = gateway.LiveRepository(root, standalone_sausage_origin=None).list_runs()[0]

            self.assertEqual(run["consumed_ms"], 7000)
            self.assertEqual(
                [point["elapsed_ms"] for point in run["score_history"]],
                [1000, 5000],
            )

    def test_archived_operator_run_uses_common_events_and_dispatch_score(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            job = root / "jobs/operator-gpt-sol-r1"
            trial = job / "emergency-operator__abc/artifacts/var/lib/operator"
            trial.mkdir(parents=True)
            (job / "config.json").write_text(json.dumps({
                "agents": [{"name": "codex", "model_name": "openai/gpt-5.6-sol", "kwargs": {}}],
                "tasks": [{"path": "tasks/emergency-operator"}],
            }))
            state = {
                "campaign": {"score": 205, "max_score": 660},
                "shift": {"status": "running", "elapsed_ms": 300_000, "remaining_ms": 1_200_000},
                "active_call": "fire-1",
                "calls": [{"id": "fire-1", "status": "active"}],
            }
            events = [{
                "schema": "benchmark-observer-event-v1",
                "sequence": 7,
                "timestamp_ms": 1_000_000,
                "type": "action",
                "action": {"command": "answer", "call": "fire-1"},
                "state": state,
                "result": {"ok": True, "elapsed_ms": 300_000, "score": 205},
                "score": 205,
                "score_delta": 0,
            }]
            (trial / "events.jsonl").write_text("\n".join(json.dumps(row) for row in events))
            result_dir = job / "emergency-operator__abc"
            (result_dir / "result.json").write_text(json.dumps({
                "started_at": "1970-01-01T00:16:40Z",
                "finished_at": "1970-01-01T00:41:40Z",
            }))

            run = gateway.LiveRepository(root, standalone_sausage_origin=None).list_runs()[0]

            self.assertEqual(run["game"], "operator")
            self.assertEqual(run["score"], 205)
            self.assertEqual(run["total"], 660)
            self.assertEqual(run["objective"], "Active call fire-1")
            self.assertEqual(run["latest_action"]["command"], "answer")

    def test_control_jobs_are_not_public_runs(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            job = root / ".harbor/jobs/parabox-240h-nop-control"
            job.mkdir(parents=True)
            (job / "config.json").write_text(json.dumps({
                "agents": [{"name": "nop", "model_name": "nop"}],
                "tasks": [{"path": "tasks/parabox-intro"}],
            }))
            repository = gateway.LiveRepository(root, standalone_sausage_origin=None)
            self.assertEqual(repository.list_runs(), [])


if __name__ == "__main__":
    unittest.main()
