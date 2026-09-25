"""Trace receipts distinguish effective routing from proposals and failures."""
import importlib.util
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("receipt_host", ROOT / "scripts/skill_discovery_host.py")
host = importlib.util.module_from_spec(spec)
spec.loader.exec_module(host)


class ReceiptTests(unittest.TestCase):
    def test_shadow_masks_only_provider_advice_and_retains_no_call_diagnostics(self):
        baseline = {"catalog_digest": "c" * 64, "skills": []}
        for enabled, key, budget, mode, reason in [
            (False, "fixture", 1, "baseline", "disabled"),
            (True, "fixture", 0, "shadow", "budget_exhausted"),
            (True, None, 1, "shadow", "missing_credential")]:
            with self.subTest(enabled=enabled, reason=reason):
                ranker = host.Ranker(enabled, True, budget, key=key, mode="shadow")
                response = host.Host("unused", "/unused", ranker, runner=lambda *a: baseline).call(
                    "discover_skills", {"query": "fixture"})
                self.assertEqual(response["metrics"]["reason"], reason)
                self.assertIn(f"mode={mode}; reason={reason}; provider_calls=0", response["routing_receipt"])
                self.assertIn("log=disabled", response["routing_receipt"])

    def test_one_shot_paid_direct_transport_is_rejected_before_dispatch(self):
        import subprocess
        import sys
        result = subprocess.run([sys.executable, str(ROOT / "scripts/skill_discovery_host.py"),
            "--metactl", "unused", "--project", str(ROOT), "--ranker", "jev",
            "--trial-mode", "advisory", "--call-tool", "discover_skills"],
            input='{"query":"fixture"}', capture_output=True, text=True, timeout=5)
        self.assertEqual(result.returncode, 2)
        self.assertIn("requires gateway", result.stderr)

    def test_baseline_shadow_advisory_and_uncertain_provider_receipts(self):
        baseline = {"catalog_digest": "c" * 64, "skills": [
            {"id": "a" * 64, "name": "first", "description": "first", "score": 1},
            {"id": "b" * 64, "name": "second", "description": "second", "score": 1}]}

        def sender(payload, key, deadline):
            return {"model": host.MODEL, "usage": {"input_tokens": 2, "output_tokens": 1},
                    "answers": {"first": {"type": "choice", "choice": "b" * 64,
                        "confidence": 1, "probabilities": {"a" * 64: 0, "b" * 64: 1, "none": 0}}}}

        def failed(*args):
            raise TimeoutError()

        with tempfile.TemporaryDirectory() as directory:
            directory = str(Path(directory).resolve())
            for mode, transport, calls, changed, reason in [
                ("baseline", sender, "0", "False", "baseline"),
                ("shadow", sender, "1", "False", "shadow"),
                ("advisory", sender, "1", "True", "reordered"),
                ("advisory", failed, "unknown", "False", "deadline")]:
                with self.subTest(mode=mode, reason=reason):
                    ranker = host.Ranker(True, True, 1, 5, "fixture", transport, mode=mode)
                    h = host.Host("unused", directory, ranker, runner=lambda *a: baseline,
                                  event_log=str(Path(directory) / "events.jsonl"))
                    response = h.call("discover_skills", {"query": "public fixture"})
                    receipt = response["routing_receipt"]
                    for value in (f"mode={mode}", f"reason={reason}", f"provider_calls={calls}",
                                  f"order_changed={changed}", "log=recorded",
                                  f"event={response['metrics']['event_id']}"):
                        self.assertIn(value, receipt)
                    self.assertNotIn("public fixture", receipt)
                    if mode == "shadow":
                        self.assertNotIn("proposed_ids", response["metrics"])
                        self.assertEqual(response["metrics"]["ranker"], "deterministic")
                        import json
                        rows = [json.loads(line) for line in (Path(directory) / "events.jsonl").read_text().splitlines()]
                        self.assertEqual(rows[-1]["proposed_ids"], ["b" * 64, "a" * 64])
                        self.assertEqual(rows[-1]["reason"], "reordered")

    def test_logging_failure_remains_visible_and_discovery_usable(self):
        baseline = {"catalog_digest": "c" * 64, "skills": []}
        with tempfile.TemporaryDirectory() as directory:
            h = host.Host("unused", directory, runner=lambda *a: baseline, event_log=directory)
            response = h.call("discover_skills", {"query": "fixture"})
            self.assertEqual(response["result"], baseline)
            self.assertIn("log=failed", response["routing_receipt"])

    def test_real_cli_receipt_can_be_joined_to_inspected_ledger(self):
        import json
        import subprocess
        from test_skill_discovery_host import Fixture
        fixture = Fixture()
        self.addCleanup(fixture.temp.cleanup)
        fixture.add("review-tests", "Review regression tests")
        log = fixture.root.resolve() / "events.jsonl"
        binary = str(ROOT / "target/debug/metactl")
        result = subprocess.run([binary, "--project", str(fixture.project), "--no-profile",
            "skills", "host", "--ranker", "deterministic", "--trial-mode", "baseline",
            "--runtime", "claude-code", "--event-log", str(log),
            "--call-tool", "discover_skills"], input='{"query":"review tests"}',
            text=True, capture_output=True, check=True, timeout=15)
        response = json.loads(result.stdout)
        self.assertIn("log=recorded", response["routing_receipt"])
        metric = response["metrics"]
        result = subprocess.run([binary, "skills", "trials", "inspect", "--log", str(log),
            "--session-id", metric["session_id"], "--run-id", metric["run_id"]],
            capture_output=True, text=True, check=True, timeout=15)
        row = json.loads(result.stdout)["events"][0]
        self.assertEqual(row["event_id"], metric["event_id"])
        self.assertEqual(row["provider_calls"], 0)
        self.assertEqual(row["runtime"], "claude-code")
