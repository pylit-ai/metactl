"""Saved policy and real packaged MCP host behavior; all provider responses simulated."""
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import unittest
from unittest.mock import patch

from test_skill_discovery_host import Fixture, ROOT, BINARY, host, baseline, response

sys.path.insert(0, str(ROOT / "scripts"))
import skill_discovery_preferences as preferences


class PersistentPreferences(unittest.TestCase):
    def setUp(self):
        self.fixture = Fixture()
        self.addCleanup(self.fixture.close)
        self.fixture.add("testing", "Verify a regression with tests")
        self.fixture.add("review", "Review code and tests")
        self.env = dict(os.environ, XDG_CONFIG_HOME=str(self.fixture.root / "config"),
                        XDG_STATE_HOME=str(self.fixture.root / "state"))
        self.gateway = self.fixture.root / "jev-test"
        self.marker = self.fixture.root / "requests.jsonl"
        self.gateway.write_text(f'''#!{sys.executable}
import json, sys
from pathlib import Path
p = json.load(sys.stdin)
with Path({str(self.marker)!r}).open('a') as log: log.write(json.dumps(p) + '\\n')
criteria = p['questions']['first']['criteria']
choice = next(iter(criteria))
print(json.dumps({{"available": True, "response": {{"model": "jev-1.13.0", "answers": {{"first": {{"type": "choice", "choice": choice, "confidence": 1.0, "probabilities": {{k: float(k == choice) for k in criteria}}}}}}, "usage": {{"input_tokens": 12, "output_tokens": 3}}}}}}))
''')
        self.gateway.chmod(0o755)

    def cli(self, *args, ok=True):
        result = subprocess.run([str(BINARY), "--project", str(self.fixture.project), "--no-profile",
                                 "--json", "--full", "skills", *args], env=self.env,
                                text=True, capture_output=True, timeout=25)
        if ok:
            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            return json.loads(result.stdout)
        return result

    def enable(self):
        return self.cli("preferences", "--mode", "enabled", "--allow-provider-data",
                        "--gateway-command", str(self.gateway), "--enroll",
                        "--gateway-project", "owned-project", "--data-class", "private-owned")

    def test_permission_scope_overrides_and_corruption(self):
        self.assertFalse(self.cli("preferences")["enabled"])
        denied = self.cli("preferences", "--mode", "enabled", ok=False)
        self.assertNotEqual(denied.returncode, 0)
        state = self.enable()
        self.assertTrue(state["enabled"])
        config = Path(state["config_path"])
        self.assertEqual(stat.S_IMODE(config.stat().st_mode), 0o600)
        with patch.dict(os.environ, self.env):
            self.assertEqual(preferences.resolve(self.fixture.root)["reason"], "project_not_enrolled")
        self.cli("preferences", "--project-mode", "disabled")
        self.assertEqual(self.cli("preferences")["reason"], "project_disabled")
        self.enable()  # Re-enrollment preserves explicit opt-out.
        self.assertEqual(self.cli("preferences")["reason"], "project_disabled")
        self.cli("preferences", "--project-mode", "inherit")
        self.assertTrue(self.cli("preferences")["enabled"])
        self.cli("preferences", "--mode", "disabled")
        self.assertEqual(self.cli("preferences")["reason"], "user_disabled")
        self.cli("preferences", "--mode", "enabled")  # Saved permission, no repeated prompt.
        self.cli("preferences", "--revoke-provider-data")
        self.assertEqual(self.cli("preferences")["reason"], "data_not_authorized")
        self.assertNotEqual(self.cli("preferences", "--mode", "enabled", ok=False).returncode, 0)
        self.assertTrue(self.cli("preferences", "--allow-provider-data")["saved"])
        with patch.dict(os.environ, dict(self.env, METACTL_JEV_DISABLE="1")):
            self.assertEqual(preferences.resolve(self.fixture.project)["reason"], "session_disabled")
        config.write_text('{broken')
        self.assertEqual(self.cli("preferences")["reason"], "preferences_unavailable")
        self.assertFalse(self.marker.exists())

    def test_budget_not_refilled_and_disable_live_ranker(self):
        self.enable()
        self.cli("preferences", "--max-provider-calls", "1")
        with patch.dict(os.environ, self.env), patch.object(host, "gateway_transport", return_value=response()) as sender:
            ranker = host.PreferenceRanker(str(self.fixture.project))
            self.assertEqual(ranker.rank("test", baseline())[1]["provider_calls"], 1)
            self.assertEqual(ranker.rank("test", baseline())[1]["reason"], "budget_exhausted")
            self.cli("preferences", "--mode", "disabled")
            self.assertEqual(ranker.rank("test", baseline())[1]["reason"], "user_disabled")
            self.assertEqual(sender.call_count, 1)

    def test_real_host_persists_across_tasks_and_stops_without_restart(self):
        self.enable()
        connected = self.cli("connect", "--target", "codex-cli", "--use-preferences", "--apply")
        self.assertEqual(connected["mode"], "preferences")
        self.assertFalse(self.marker.exists())
        process = subprocess.Popen([connected["command"], *connected["args"]], env=self.env,
                                   stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                   text=True)
        def close():
            process.terminate()
            process.communicate(timeout=10)
        self.addCleanup(close)
        def discover(number):
            process.stdin.write(json.dumps({"jsonrpc": "2.0", "id": number, "method": "tools/call",
                "params": {"name": "discover_skills", "arguments": {"query": "Review tests for a repair"}}}) + "\n")
            process.stdin.flush()
            wire = json.loads(process.stdout.readline())
            return json.loads(wire["result"]["content"][0]["text"])
        first, second = discover(1), discover(2)
        self.assertEqual(first["metrics"]["provider_calls"], 1)
        self.assertEqual(second["metrics"]["provider_calls"], 1)
        fresh = subprocess.run([connected["command"], *connected["args"], "--call-tool", "discover_skills"],
                               env=self.env, input=json.dumps({"query": "Review tests for another repair"}),
                               capture_output=True, text=True, timeout=25)
        self.assertEqual(fresh.returncode, 0, fresh.stderr)
        self.assertEqual(json.loads(fresh.stdout)["metrics"]["provider_calls"], 1)
        self.cli("preferences", "--project-mode", "disabled")
        third = discover(3)
        self.assertEqual(third["metrics"]["provider_calls"], 0)
        self.assertEqual(third["metrics"]["reason"], "project_disabled")
        self.assertEqual(third["metrics"]["telemetry_status"], "recorded")
        requests = [json.loads(line) for line in self.marker.read_text().splitlines()]
        self.assertEqual(len(requests), 3)
        self.assertEqual(requests[0]["dataClass"], "private-owned")
        self.assertNotIn("Original instructions", self.marker.read_text())
        log = Path(connected["event_log"]).read_text()
        self.assertEqual(len(log.splitlines()), 4)
        self.assertNotIn("Review tests for a repair", log)
        self.assertNotIn("Original instructions", log)
        doctor = self.cli("doctor", "--target", "codex-cli", "--use-preferences")
        self.assertEqual(doctor["preferences"]["reason"], "project_disabled")
        self.assertEqual(doctor["registered_mode"], "preferences")
        self.assertEqual(doctor["effective_mode"], "baseline")
        self.assertEqual(doctor["check_provider_calls"], 0)
        self.assertEqual(len(self.marker.read_text().splitlines()), 3)
        self.cli("connect", "--target", "codex-cli", "--remove")

    def test_preference_fallbacks_and_executable_diagnostics(self):
        self.enable()
        for failure, expected in [(TimeoutError(), "deadline"), (ValueError("denied"), "provider_or_schema_failure")]:
            with patch.dict(os.environ, self.env), patch.object(host, "gateway_transport", side_effect=failure) as sender:
                result, metric = host.PreferenceRanker(str(self.fixture.project)).rank("test", baseline())
                self.assertEqual(result, baseline())
                self.assertEqual(metric["reason"], expected)
                self.assertEqual(metric["provider_attempts"], 1)
                self.assertIsNone(metric["provider_calls"])
                self.assertEqual(sender.call_count, 1)
        self.cli("connect", "--target", "codex-cli", "--use-preferences", "--apply")
        self.gateway.chmod(0o600)
        doctor = self.cli("doctor", "--target", "codex-cli", "--use-preferences")
        self.assertEqual(doctor["effective_gateway_state"], "not_executable")
        self.assertFalse(doctor["provider_ready"])
        self.gateway.unlink()
        with patch.dict(os.environ, self.env), patch.object(host, "gateway_transport") as sender:
            result, metric = host.PreferenceRanker(str(self.fixture.project)).rank("test", baseline())
            self.assertEqual(result, baseline())
            self.assertEqual(metric["reason"], "missing_credential")
            self.assertEqual(metric["provider_attempts"], 0)
            sender.assert_not_called()
        self.assertEqual(self.cli("doctor", "--target", "codex-cli", "--use-preferences")["effective_gateway_state"], "missing")

    def test_preferences_packaged_mirror(self):
        self.assertEqual((ROOT / "scripts/skill_discovery_preferences.py").read_bytes(),
                         (ROOT / "crates/metactl/assets/skill_discovery_preferences.py").read_bytes())

    def test_direct_preference_host_logs_without_connecting(self):
        self.enable()
        status = self.cli("host", "--use-preferences", "--status")
        self.assertFalse(Path(status["event_log"]).exists())
        result = subprocess.run([str(BINARY), "--project", str(self.fixture.project), "--no-profile",
                                 "skills", "host", "--use-preferences", "--call-tool", "discover_skills"],
                                env=self.env, input=json.dumps({"query": "Review tests for a repair"}),
                                capture_output=True, text=True, timeout=25)
        self.assertEqual(result.returncode, 0, result.stderr)
        receipt = json.loads(result.stdout)
        self.assertEqual(receipt["metrics"]["telemetry_status"], "recorded")
        self.assertTrue(Path(status["event_log"]).exists())


if __name__ == "__main__":
    unittest.main()
