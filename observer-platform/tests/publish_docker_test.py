import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

spec = importlib.util.spec_from_file_location("publisher", Path(__file__).parents[1] / "scripts/publish_docker.py")
publisher = importlib.util.module_from_spec(spec)
spec.loader.exec_module(publisher)


class PublicationTests(unittest.TestCase):
    def test_scores_and_identities_must_match(self):
        publisher.compare_runs({"runs": [{"id": "a", "score": 1}]}, {"runs": [{"id": "a", "score": 1}]})
        with self.assertRaises(RuntimeError):
            publisher.compare_runs({"runs": [{"id": "a", "score": 1}]}, {"runs": [{"id": "a", "score": 2}]})

    def test_candidate_failure_cannot_stop_existing_services(self):
        with patch.object(publisher, "compose"), patch.object(publisher, "command"), \
             patch.object(publisher, "verify", side_effect=RuntimeError("not ready")), \
             patch.object(publisher, "stop_native") as stop:
            with self.assertRaises(RuntimeError):
                publisher.publish("test", True, False)
            stop.assert_not_called()

    def test_failed_cutover_restores_native_services(self):
        snapshot = {"runs": [{"id": "a", "score": 1}]}
        actions = []
        compose = Mock(side_effect=lambda release, candidate, *args, **kwargs: actions.append((candidate, args)) or "container")
        with tempfile.TemporaryDirectory() as directory, \
             patch.object(publisher, "STATE", Path(directory)), \
             patch.object(publisher, "compose", compose), patch.object(publisher, "command"), \
             patch.object(publisher, "read_json", return_value=snapshot), \
             patch.object(publisher, "verify", side_effect=[snapshot, RuntimeError("cutover failed")]), \
             patch.object(publisher, "native_services", return_value=["site", "gateway"]), \
             patch.object(publisher, "stop_native") as stop, \
             patch.object(publisher, "restore_native") as restore:
            with self.assertRaises(RuntimeError):
                publisher.publish("test", True, False)
            stop.assert_called_once_with(["site", "gateway"])
            restore.assert_called_once_with(["site", "gateway"])
            self.assertIn((False, ("down",)), actions)
            self.assertEqual(actions[-1], (True, ("down",)))

    def test_image_tags_are_not_shell_programs(self):
        with patch.object(publisher, "compose") as compose:
            with self.assertRaises(ValueError):
                publisher.publish("bad;tag", False, False)
            compose.assert_not_called()


if __name__ == "__main__":
    unittest.main()
