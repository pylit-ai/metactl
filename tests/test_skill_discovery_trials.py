"""Offline privacy, persistence and descriptive-report contracts."""

import importlib.util
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import unittest
import uuid


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts/skill_discovery_trials.py"
spec = importlib.util.spec_from_file_location("skill_discovery_trials", SCRIPT)
trial = importlib.util.module_from_spec(spec)
spec.loader.exec_module(trial)


def event(kind="discover", **changes):
    common = {"schema": trial.SCHEMA, "kind": kind, "event_id": uuid.uuid4().hex,
              "session_id": "a" * 64, "run_id": "b" * 32,
              "runtime": "codex", "arm": "baseline", "transport": "none", "time": 1.0}
    details = {
        "discover": {"elapsed_ms": 12.0, "rank_ms": 0.0, "result_bytes": 120,
                     "result_count": 2, "catalog_digest": "c" * 64,
                     "baseline_ids": ["d" * 64, "e" * 64],
                     "effective_ids": ["d" * 64, "e" * 64],
                     "proposed_ids": ["d" * 64, "e" * 64], "reason": "baseline",
                     "provider_attempts": 0, "provider_calls": 0, "usage": None,
                     "model": None, "native_catalog_suppressed": False, "cost_usd": None},
        "load": {"elapsed_ms": 3.0, "result_bytes": 500, "repeat_load": False,
                 "skill_id": "d" * 64},
        "outcome": {"success": "unknown"},
    }[kind]
    return {**common, **details, **changes}


class TrialTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(dir="/private/tmp" if Path("/private/tmp").is_dir() else None)
        self.addCleanup(self.temp.cleanup)
        self.log = Path(self.temp.name) / "trial.jsonl"

    def test_mirror_and_private_append_idempotency(self):
        self.assertEqual(SCRIPT.read_bytes(),
                         (ROOT / "crates/metactl/assets/skill_discovery_trials.py").read_bytes())
        row = event()
        trial.record_event(self.log, row)
        trial.record_event(self.log, row)
        self.assertEqual(len(trial.read_events(self.log)), 1)
        self.assertEqual(stat.S_IMODE(self.log.stat().st_mode), 0o600)
        self.assertEqual(trial.session_key("secret-session"),
                         __import__("hashlib").sha256(b"secret-session").hexdigest())
        with self.assertRaisesRegex(ValueError, "conflicting duplicate"):
            trial.record_event(self.log, {**row, "elapsed_ms": 15})
        unknown = event("outcome")
        trial.record_event(self.log, unknown)
        with self.assertRaisesRegex(ValueError, "duplicate outcome"):
            trial.record_event(self.log, event("outcome"))

    def test_rejects_raw_data_and_unproven_claims(self):
        for change in ({"query": "private task"}, {"error": "key=secret"},
                       {"native_catalog_suppressed": True}, {"cost_usd": 1.23},
                       {"baseline_ids": ["private-name"]}, {"reason": "<script>"},
                       {"model": "/private/path"},
                       {"usage": {"input_tokens": -1, "output_tokens": 0}},
                       {"provider_calls": 2}):
            with self.subTest(change=change), self.assertRaises(ValueError):
                trial.record_event(self.log, event(**change))
        self.assertFalse(self.log.exists())
        with self.assertRaises(ValueError):
            trial.validate_event(event("outcome", success="pass"))
        with self.assertRaises(ValueError):
            trial.validate_event(event("outcome", success="pass", verifier_ref=None))

    def test_rejects_permissions_links_incomplete_and_conflicts(self):
        self.log.write_text("{\"incomplete\":")
        self.log.chmod(0o600)
        with self.assertRaises(ValueError):
            trial.record_event(self.log, event())
        self.log.write_text("")
        self.log.chmod(0o644)
        with self.assertRaises(PermissionError):
            trial.record_event(self.log, event())
        self.log.chmod(0o600)
        linked = Path(self.temp.name) / "hardlink"
        os.link(self.log, linked)
        with self.assertRaises(PermissionError):
            trial.read_events(self.log)
        linked.unlink()
        self.log.unlink()
        self.log.symlink_to(Path(self.temp.name) / "target")
        with self.assertRaises(OSError):
            trial.record_event(self.log, event())
        linked_dir = Path(self.temp.name) / "linked-dir"
        linked_dir.symlink_to(self.temp.name, target_is_directory=True)
        with self.assertRaises(PermissionError):
            trial.record_event(linked_dir / "fresh.jsonl", event())
        self.log.unlink()
        self.log.write_text('{"schema":"a","schema":"b"}\n')
        self.log.chmod(0o600)
        with self.assertRaises(ValueError):
            trial.read_events(self.log)

    def test_report_cohorts_and_unknown_coverage(self):
        rows = [event(), event("load"),
                event("load", repeat_load=True, result_bytes=600),
                event("outcome", success="pass", verifier_ref="f" * 64,
                      task_ms=1000, cost_usd=0.25, input_tokens=123,
                      output_tokens=45, human_interventions=0),
                event(runtime="pi", arm="advisory", transport="direct",
                      reason="reordered", provider_attempts=1, provider_calls=1,
                      usage={"input_tokens": 10, "output_tokens": 2}, model="jev-1.13.0",
                      session_id="9" * 64, elapsed_ms=90, rank_ms=60)]
        for row in rows:
            trial.record_event(self.log, row)
        report = trial.summarize(trial.read_events(self.log))
        self.assertEqual(report["event_count"], 5)
        baseline, advisory = report["cohorts"]
        self.assertEqual((baseline["runtime"], baseline["repeated_loads"],
                          baseline["outcomes"], baseline["cost_usd_reported"]),
                         ("codex", 1, 1, .25))
        self.assertEqual((baseline["usage_known"], baseline["discover_ms_p95"]), (0, 12.0))
        self.assertIsNone(baseline["input_tokens_reported"])
        self.assertEqual((baseline["task_input_tokens_reported"], baseline["task_input_tokens_known"],
                          baseline["task_output_tokens_reported"], baseline["task_output_tokens_known"],
                          baseline["human_interventions_reported"], baseline["human_interventions_known"]),
                         (123, 1, 45, 1, 0, 1))
        self.assertEqual((advisory["runtime"], advisory["provider_calls_observed"],
                          advisory["sessions_without_outcome"]), ("pi", 1, 1))
        self.assertEqual(advisory["uncertain_provider_attempts"], 0)
        self.assertEqual(advisory["reordered"], 0)
        self.assertIsNone(advisory["cost_usd_reported"])
        self.assertIsNone(advisory["task_input_tokens_reported"])
        self.assertIsNone(advisory["human_interventions_reported"])
        html_output = trial.render_html(report)
        self.assertEqual(html_output.count("<table>"), 4)
        self.assertEqual(html_output.count("<th scope='col'>"), 30)
        self.assertIn("unknown <small>(0/0 known)</small>", html_output)
        self.assertIn("unknown / unknown", html_output)
        self.assertEqual(trial.summarize(rows, runtime="pi")["event_count"], 1)
        self.assertIn("No causal savings", report["interpretation"])

    def test_unchanged_jev_choice_is_neither_reorder_nor_fallback(self):
        row = event(arm="advisory", transport="gateway", reason="unchanged",
                    provider_attempts=1, provider_calls=1,
                    usage={"input_tokens": 10, "output_tokens": 2}, model="jev-1.13.0")
        trial.record_event(self.log, row)
        cohort = trial.summarize(trial.read_events(self.log))["cohorts"][0]
        self.assertEqual((cohort["reordered"], cohort["fallback"]), (0, 0))

    def test_html_escapes_and_report_cli_private_output(self):
        self.assertIn("&lt;script&gt;", trial.render_html({"cohorts": [], "event_count": 0,
                                                        "interpretation": "<script>"}))
        trial.record_event(self.log, event())
        output = Path(self.temp.name) / "report.html"
        json_output = Path(self.temp.name) / "report.json"
        result = subprocess.run([sys.executable, str(SCRIPT), "report", "--log", str(self.log),
                                 "--output", str(output), "--json-output", str(json_output)],
                                text=True, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("No causal savings", output.read_text())
        self.assertEqual(json.loads(json_output.read_text())["event_count"], 1)
        self.assertEqual(stat.S_IMODE(output.stat().st_mode), 0o600)
        collision = subprocess.run([sys.executable, str(SCRIPT), "report", "--log", str(self.log),
                                    "--output", str(self.log)], text=True, capture_output=True)
        self.assertNotEqual(collision.returncode, 0)
        self.assertEqual(len(trial.read_events(self.log)), 1)

    def test_outcome_cli_infers_existing_session(self):
        trial.record_event(self.log, event())
        result = subprocess.run([sys.executable, str(SCRIPT), "outcome", "--log", str(self.log),
                                 "--session-id", "a" * 64, "--runtime", "codex",
                                 "--arm", "baseline", "--success", "pass",
                                 "--verifier-ref", "f" * 64], text=True, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(trial.read_events(self.log)[-1]["run_id"], "b" * 32)
        repeat = subprocess.run([sys.executable, str(SCRIPT), "outcome", "--log", str(self.log),
                                 "--session-id", "a" * 64, "--runtime", "codex",
                                 "--arm", "baseline", "--success", "unknown"],
                                text=True, capture_output=True)
        self.assertNotEqual(repeat.returncode, 0)


if __name__ == "__main__":
    unittest.main()
