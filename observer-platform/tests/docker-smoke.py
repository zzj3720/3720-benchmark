"""Explicit integration check against an isolated Docker gateway and host recorder."""
import argparse
import json
from pathlib import Path
import select
import subprocess
import tempfile
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
RECORDER = ROOT / "tools/observer/runtime/target/release/run-recorder"


def now():
    return int(time.time() * 1000)


def wait_for(check, timeout=20):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        try:
            result = check()
            if result:
                return result
        except (OSError, ValueError, KeyError, StopIteration):
            pass
        time.sleep(0.15)
    raise AssertionError("integration condition timed out")


def run(image):
    processes = []
    container = None
    with tempfile.TemporaryDirectory(prefix="live-docker-smoke-") as directory:
        root = Path(directory)
        journals = root / "run-journals"
        archive = root / "live-archive"
        journals.mkdir(); archive.mkdir()
        try:
            container = subprocess.check_output([
                "docker", "run", "-d", "--rm", "--read-only", "--memory=256m", "--memory-swap=256m",
                "--tmpfs", "/cache:rw,uid=10001,gid=10001,size=48m", "-p", "127.0.0.1::3740",
                "--mount", f"type=bind,src={journals},dst=/data/.harbor/run-journals,readonly",
                "--mount", f"type=bind,src={archive},dst=/data/.harbor/live-archive,readonly", image,
            ], text=True).strip()
            port = subprocess.check_output(["docker", "port", container, "3740/tcp"], text=True).strip().rsplit(":", 1)[1]
            base = "http://127.0.0.1:" + port

            def get(path):
                with urllib.request.urlopen(base + path, timeout=10) as response:
                    return json.load(response)

            def summary(chain):
                return next(run for run in get("/v1/runs")["runs"] if run["id"] == chain)

            def send(process, message):
                process.stdin.write(json.dumps(message) + "\n"); process.stdin.flush()
                assert select.select([process.stdout], [], [], 10)[0], "recorder response timed out"
                result = json.loads(process.stdout.readline())
                assert result["ok"], result

            def start(chain, segment):
                process = subprocess.Popen([str(RECORDER), chain, str(journals)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
                processes.append(process)
                inbox = root / f"inbox-{segment}"
                send(process, {"command": "register", "segment_id": segment, "created_at_ms": now(), "observer_dir": str(inbox), "job_id": segment, "job_name": segment, "trial_id": segment, "trial_name": segment, "task": "sokoban", "model": "fixture/model", "agent": "fixture"})
                send(process, {"command": "lifecycle", "segment_id": segment, "event": "agent-start", "timestamp_ms": now()})
                return process, inbox / "game-inbox.jsonl"

            def observation(sequence, score):
                return json.dumps({"schema": "benchmark-observer-event-v1", "sequence": sequence, "timestamp_ms": now(), "action": {"command": "move"}, "score": score, "score_delta": 1,
                                   "state": {"campaign": {"score": score, "max_score": 305}, "level": {"id": "one", "title": "One"}}}) + "\n"

            def finish(process, segment):
                for event in ("agent-end", "end"):
                    send(process, {"command": "lifecycle", "segment_id": segment, "event": event, "timestamp_ms": now()})
                send(process, {"command": "shutdown"}); process.wait(timeout=10)
                assert process.returncode == 0

            wait_for(lambda: get("/health")["ready"])
            process, inbox = start("orphan", "first")
            with inbox.open("a") as file: file.write(observation(1, 1))
            before = wait_for(lambda: (value if (value := summary("orphan"))["live"] and value["score"] == 1 else None))
            with urllib.request.urlopen(base + "/v1/subscribe?protocol=2&run_id=orphan", timeout=12) as stream:
                def event():
                    while True:
                        line = stream.readline()
                        assert line, "SSE disconnected"
                        if line.startswith(b"data:"):
                            return json.loads(line[5:])
                assert event()["reset"]
                line = observation(2, 2); split = len(line) // 2
                with inbox.open("a") as file: file.write(line[:split])
                time.sleep(0.4)
                assert summary("orphan")["score"] == 1
                with inbox.open("a") as file: file.write(line[split:])
                change = event()
                while not any(run["score"] == 2 for run in change["runs"]): change = event()
                update = next(run for run in change["runs"] if run["id"] == "orphan")
                assert "score_history" not in update and len(update["score_history_delta"]) == 1
            stable = summary("orphan")
            process.kill(); process.wait(timeout=5)
            after = wait_for(lambda: (value if (value := summary("orphan"))["termination"]["kind"] == "orphaned" else None), 15)
            assert after["latest_sequence"] == stable["latest_sequence"]
            assert after["detail_revision"] != stable["detail_revision"]

            first, inbox = start("continued", "segment-one")
            time.sleep(0.05)
            with inbox.open("a") as file: file.write(observation(1, 3))
            wait_for(lambda: summary("continued")["score"] == 3)
            finish(first, "segment-one")
            old = wait_for(lambda: (value if not (value := summary("continued"))["live"] else None))
            assert not (journals / "continued/journal.jsonl").exists()
            assert (journals / "continued/journal-index.json").exists()
            second, inbox = start("continued", "segment-two")
            with inbox.open("a") as file: file.write(observation(1, 4))
            current = wait_for(lambda: (value if (value := summary("continued"))["score"] == 4 else None))
            assert current["consumed_ms"] >= old["consumed_ms"] > 0
            assert current["score_history"][-1]["elapsed_ms"] >= old["consumed_ms"]
            finish(second, "segment-two")
            assert len(json.loads((journals / "continued/journal-index.json").read_text())["chunks"]) == 2
            print(json.dumps({"heartbeat": "passed", "partial_inbox": "passed", "sse_delta": "passed", "orphan_invalidation": "passed", "segmented_continuation": "passed"}))
        finally:
            for process in processes:
                if process.poll() is None: process.kill(); process.wait(timeout=5)
            if container: subprocess.run(["docker", "rm", "-f", container], stdout=subprocess.DEVNULL, check=False)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--image", required=True)
    run(parser.parse_args().image)
