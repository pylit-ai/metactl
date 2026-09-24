"""Gateway client protocol and trial-mode regression checks; no provider access."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from test_skill_discovery_host import host, baseline, response, Fixture, BINARY


class GatewayTrials(unittest.TestCase):
    @unittest.skipUnless(BINARY.exists(), "build metactl first")
    def test_health_check_is_not_a_task_event_and_synthetic_is_check_only(self):
        fixture = Fixture()
        self.addCleanup(fixture.close)
        with tempfile.TemporaryDirectory() as root:
            ledger = Path(root).resolve() / "events.jsonl"
            script = self.client(root, "import sys\nsys.stdin.read()\nprint(" + repr(json.dumps({"available": True, "response": response()})) + ")\n")
            args = [str(BINARY), "--project", str(fixture.project), "--no-profile", "skills", "host",
                    "--ranker", "jev", "--jev-transport", "gateway", "--gateway-command", script,
                    "--gateway-data-class", "synthetic", "--allow-provider-data", "--max-provider-calls", "1",
                    "--event-log", str(ledger)]
            result = subprocess.run(args + ["--check"], capture_output=True, text=True, timeout=10)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertTrue(json.loads(result.stdout)["provider_verified"])
            self.assertFalse(ledger.exists())
            rejected = subprocess.run(args + ["--call-tool", "discover_skills"], input='{"query":"task"}',
                                      capture_output=True, text=True, timeout=10)
            self.assertNotEqual(rejected.returncode, 0)
            self.assertIn("reserved for --check", rejected.stderr)

    def test_oversized_gateway_output_is_terminated_before_deadline(self):
        with tempfile.TemporaryDirectory() as root:
            script = self.client(root, "import sys,time\nsys.stdin.read()\nsys.stdout.write('x'*2097152)\nsys.stdout.flush()\ntime.sleep(10)\n")
            start = host.time.monotonic()
            with self.assertRaises(ValueError):
                host.exchange_child([script], {}, 3)
            self.assertLess(host.time.monotonic() - start, 1)

    def test_real_discovery_event_is_recorded_and_joinable(self):
        with tempfile.TemporaryDirectory() as root:
            ledger = Path(root).resolve() / "events.jsonl"
            h = host.Host("unused", "fixed", runner=lambda *a: baseline(), event_log=ledger)
            result = h.call("discover_skills", {"query": "private-query"})
            self.assertEqual(result["metrics"]["telemetry_status"], "recorded")
            event = json.loads(ledger.read_text())
            self.assertEqual(result["metrics"]["run_id"], event["run_id"])
            self.assertNotIn("private-query", ledger.read_text())

    def client(self, root, body):
        path = Path(root) / "scoped-client"
        path.write_text("#!" + os.sys.executable + "\n" + body)
        path.chmod(0o700)
        return str(path)

    def test_real_child_uses_fixed_identity_and_stdin_without_provider_key(self):
        with tempfile.TemporaryDirectory() as root:
            script = self.client(root, "import sys,json,os\n"
                "assert sys.argv[1:] == ['evaluate','--project','approved']\n"
                "value=json.load(sys.stdin)\n"
                "assert set(value)=={'state','questions','dataClass'}\n"
                "assert value['dataClass']=='synthetic'\n"
                "assert value['state']['task']=='private-sentinel'\n"
                "print(" + repr(json.dumps({"available": True, "response": response()})) + ")\n")
            ranker = host.Ranker(True, True, 1, key="scoped-client", transport_kind="gateway",
                sender=lambda p, k, d: host.gateway_transport(p, d, script, "approved", root, "synthetic"))
            result, metrics = ranker.rank("private-sentinel", baseline())
            self.assertEqual(result["skills"][0]["id"], "b" * 64)
            self.assertEqual(metrics["provider_calls"], 1)
            self.assertEqual(metrics["provider_attempts"], 1)
            self.assertNotIn("private-sentinel", json.dumps(metrics))

    def test_gateway_denial_retains_baseline_and_marks_dispatch_unknown(self):
        with tempfile.TemporaryDirectory() as root:
            script = self.client(root, "import sys\nsys.stdin.read()\nprint('sensitive-error',file=sys.stderr)\n"
                                 "print('{\"available\":false}')\nsys.exit(1)\n")
            ranker = host.Ranker(True, True, 1, key="scoped-client", transport_kind="gateway",
                sender=lambda p,k,d: host.gateway_transport(p,d,script,None,root,"synthetic"))
            result, metrics = ranker.rank("task", baseline())
            self.assertEqual(result, baseline())
            self.assertEqual(metrics["provider_attempts"], 1)
            self.assertIsNone(metrics["provider_calls"])
            self.assertNotIn("sensitive-error", json.dumps(metrics))
            self.assertEqual(ranker.rank("task", baseline())[1]["reason"], "budget_exhausted")

    def test_shadow_records_proposal_without_applying_and_baseline_never_dispatches(self):
        ranker = host.Ranker(True, True, 1, key="client", sender=lambda *a: response(), mode="shadow")
        result, metric = ranker.rank("task", baseline())
        self.assertEqual(result, baseline())
        self.assertEqual(metric["proposed_ids"], ["b" * 64, "a" * 64])
        ranker = host.Ranker(True, True, 1, key="client", mode="baseline", sender=lambda *a: self.fail("dispatch"))
        self.assertEqual(ranker.rank("task", baseline())[1]["provider_calls"], 0)
        self.assertEqual(ranker.remaining, 1)

    def test_gateway_timeout_is_bounded_and_candidate_set_preserved(self):
        with tempfile.TemporaryDirectory() as root:
            script = self.client(root, "import time\ntime.sleep(10)\n")
            ranker = host.Ranker(True, True, 1, .05, key="client", transport_kind="gateway",
                sender=lambda p,k,d: host.gateway_transport(p,d,script,None,root,"synthetic"))
            start = host.time.monotonic()
            result, metric = ranker.rank("task", baseline())
            self.assertEqual(result, baseline())
            self.assertEqual(metric["reason"], "deadline")
            self.assertLess(host.time.monotonic()-start, .7)

    def test_failed_telemetry_does_not_change_baseline_or_expose_paths(self):
        h = host.Host("unused", "fixed", runner=lambda *a: baseline(), event_log="/nonexistent/private/sentinel")
        result = h.call("discover_skills", {"query": "private-query"})
        self.assertEqual(result["result"], baseline())
        self.assertEqual(result["metrics"]["telemetry_status"], "failed")
        self.assertNotIn("sentinel", json.dumps(result))
        self.assertNotIn("private-query", json.dumps(result))


if __name__ == "__main__":
    unittest.main()
