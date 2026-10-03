"""Bounded broader retrieval through the real CLI and packaged MCP host; offline providers."""
import copy
import json
import os
from pathlib import Path
import subprocess
import sys
import unittest

from test_skill_discovery_host import Fixture, BINARY, ROOT, host
sys.path.insert(0, str(ROOT / "scripts"))
import skill_discovery_trials as trial


class CandidatePoolTests(unittest.TestCase):
    def setUp(self):
        self.fx = Fixture()
        self.addCleanup(self.fx.close)
        for number in range(23):
            self.fx.add(f"pool-case-{number}", "Inspect regression correctness evidence")
        self.query = "regression correctness evidence"
        self.pool = self.fx.run(["discover", "--limit", "20", "--query-stdin"], self.query)
        self.chosen = self.pool["skills"][7]
        self.log = self.fx.root.resolve() / "pool-events.jsonl"

    def sender(self, choice=None):
        def send(payload, *args):
            ids = list(payload["questions"]["first"]["criteria"])
            selected = self.chosen["id"] if choice is None else choice
            return {"model": host.MODEL, "usage": {"input_tokens": 321, "output_tokens": 9},
                "answers": {"first": {"type": "choice", "choice": selected, "confidence": 1,
                    "probabilities": {key: float(key == selected) for key in ids}}}}
        return send

    def make_host(self, mode="advisory", choice=None, limit=20):
        ranker = host.Ranker(True, True, 1, key="offline-fixture", sender=self.sender(choice), mode=mode)
        return host.Host(str(BINARY), str(self.fx.project), ranker, runner=self.fx.run,
            event_log=str(self.log), candidate_limit=limit)

    def test_outside_five_promotes_loads_and_full_private_proposal_roundtrips(self):
        h = self.make_host()
        value = h.call("discover_skills", {"query": self.query})
        self.assertEqual(value["result"]["skills"][0], self.chosen)
        self.assertEqual(len(value["result"]["skills"]), 5)
        self.assertEqual(value["recommended_ids"], [self.chosen["id"]])
        self.assertEqual(value["metrics"]["candidate_count"], 20)
        self.assertEqual(value["metrics"]["usage"], {"input_tokens": 321, "output_tokens": 9})
        self.assertNotIn("candidate_ids", value["metrics"])
        self.assertEqual(len(value["metrics"]["proposed_ids"]), 5)
        self.assertEqual(value["metrics"]["telemetry_status"], "recorded")
        loaded = h.call("load_skill", {"id": self.chosen["id"], "digest": self.chosen["digest"]})
        self.assertIn("Original instructions for " + self.chosen["name"], loaded["result"]["instructions"])
        rows = trial.read_events(self.log)
        self.assertEqual(len(rows[0]["candidate_ids"]), 20)
        self.assertEqual(len(rows[0]["proposed_ids"]), 20)
        self.assertNotIn(self.chosen["id"], rows[0]["baseline_ids"])
        summary = trial.summarize(rows)["cohorts"][0]
        self.assertEqual((summary["candidate_coverage_known"], summary["candidate_count_reported"],
            summary["outside_shortlist_choices"], summary["reordered"]), (1, 20, 1, 1))
        self.assertNotIn(self.query, self.log.read_text())
        self.assertNotIn("Original instructions", self.log.read_text())
        inspected = subprocess.run([str(BINARY), "skills", "trials", "inspect", "--log", str(self.log),
            "--session-id", value["metrics"]["session_id"]], capture_output=True, text=True, timeout=15)
        self.assertEqual(inspected.returncode, 0, inspected.stderr)
        self.assertEqual(json.loads(inspected.stdout)["events"][0]["candidate_count"], 20)

    def test_abstention_shadow_failure_and_cap_preserve_baseline_five(self):
        expected = self.pool["skills"][:5]
        for mode, choice, limit, reason in [("baseline", None, 20, "baseline"),
            ("shadow", None, 20, "shadow"), ("advisory", "none", 20, "abstained"),
            ("advisory", "f" * 64, 20, "provider_or_schema_failure"),
            ("advisory", None, 5, "provider_or_schema_failure")]:
            with self.subTest(mode=mode, choice=choice, limit=limit):
                value = self.make_host(mode, choice, limit).call("discover_skills", {"query": self.query})
                self.assertEqual(value["result"]["skills"], expected)
                self.assertEqual(value["metrics"]["reason"], reason)
                self.assertEqual(value["recommended_ids"], [])
                if mode == "shadow":
                    self.assertNotIn("proposed_ids", value["metrics"])
                    row = trial.read_events(self.log)[-1]
                    self.assertEqual(row["proposed_ids"][0], self.chosen["id"])
                if mode == "baseline":
                    self.assertEqual(value["metrics"]["provider_attempts"], 0)

    def test_invalid_host_pool_bounds_reject_before_cli(self):
        for limit in (4, 21, True):
            with self.subTest(limit=limit), self.assertRaises(ValueError):
                host.Host("unused", "unused", candidate_limit=limit,
                    runner=lambda *args: self.fail("invalid bound reached CLI"))

    def test_full_pool_unchanged_choice_is_not_false_reorder(self):
        value = self.make_host(choice=self.pool["skills"][0]["id"]).call("discover_skills", {"query": self.query})
        self.assertEqual(value["metrics"]["reason"], "unchanged")
        row = trial.read_events(self.log)[0]
        self.assertEqual(len(row["proposed_ids"]), 20)
        self.assertEqual(trial.summarize([dict(row, reason="reordered")])["cohorts"][0]["reordered"], 0)

    def test_payload_ceiling_still_prevents_attempts_with_twenty_candidates(self):
        pool = copy.deepcopy(self.pool)
        for descriptor in pool["skills"]:
            descriptor["description"] = "x" * 5000
        for transport in ("gateway", "direct"):
            ranker = host.Ranker(True, True, 1, key="fixture", transport_kind=transport,
                sender=lambda *args: self.fail("oversized pool dispatched"))
            value, metrics = ranker.rank(self.query, dict(pool, skills=pool["skills"][:5]), pool)
            self.assertEqual(metrics["reason"], "payload_budget")
            self.assertEqual(metrics["provider_attempts"], 0)
            self.assertEqual(ranker.remaining, 1)
            self.assertEqual(metrics["submitted_candidate_count"], 0)
            self.assertEqual(metrics["candidate_count"], 5)
            self.assertEqual(len(value["skills"]), 5)

    def test_twenty_tail_trims_to_exact_wire_bound_with_outside_five_promotion(self):
        pool = copy.deepcopy(self.pool)
        for descriptor in pool["skills"]:
            descriptor["description"] = "x" * 500
        capture = []
        sender = self.sender()
        def send(payload, *args):
            capture.append(payload)
            return sender(payload, *args)
        for mode, data_class in (("advisory", "public-nonsensitive"), ("shadow", "public-nonsensitive"),
                                  ("advisory", "private-owned"), ("shadow", "private-owned")):
            ranker = host.Ranker(True, True, 1, key="fixture", sender=send,
                transport_kind="gateway", mode=mode, data_class=data_class)
            h = host.Host("unused", str(self.fx.project), ranker, runner=lambda *args: copy.deepcopy(pool),
                event_log=str(self.log))
            value = h.call("discover_skills", {"query": self.query})
            metric = value["metrics"]
            self.assertEqual(metric["retrieved_count"], 20)
            self.assertGreater(metric["candidate_count"], 7)
            self.assertLess(metric["candidate_count"], 20)
            self.assertEqual(metric["submitted_candidate_count"], metric["candidate_count"])
            self.assertLessEqual(metric["payload_bytes"], 15000)
            wire = {"state": capture[-1]["state"], "questions": capture[-1]["questions"],
                "dataClass": data_class}
            self.assertEqual(metric["payload_bytes"], len(host.compact(wire).encode()))
            self.assertTrue(all(s["description"] == "x" * 500 for s in wire["state"]["candidates"]))
            self.assertEqual(len(value["result"]["skills"]), 5)
            self.assertEqual(metric["telemetry_status"], "recorded")
            if mode == "advisory":
                self.assertEqual(value["result"]["skills"][0]["id"], self.chosen["id"])
            else:
                self.assertEqual(value["result"]["skills"], pool["skills"][:5])
                self.assertNotIn("proposed_ids", metric)
            row = trial.read_events(self.log)[-1]
            self.assertEqual(len(row["candidate_ids"]), metric["candidate_count"])
            self.assertEqual(len(row["proposed_ids"]), metric["candidate_count"])

    def test_pool_ledger_rejects_partial_unbounded_and_forged_ids(self):
        self.make_host().call("discover_skills", {"query": self.query})
        row = trial.read_events(self.log)[0]
        changes = [{"candidate_count": True}, {"candidate_count": 21}, {"candidate_limit": 21},
            {"result_limit": 6}, {"retrieved_count": 21}, {"submitted_candidate_count": 21},
            {"payload_bytes": row["payload_limit"] + 1}, {"payload_limit": 15001}, {"proposed_ids": ["f" * 64]}, {"effective_ids": ["f" * 64]},
            {"candidate_ids": row["candidate_ids"] + [row["candidate_ids"][0]]}]
        for change in changes:
            with self.subTest(change=change), self.assertRaises(ValueError):
                trial.validate_event(dict(row, **change))
        partial = dict(row)
        del partial["candidate_count"]
        with self.assertRaises(ValueError): trial.validate_event(partial)

    def test_packaged_mcp_promotes_outside_five_and_flags_roundtrip(self):
        gateway = self.fx.root / "offline-gateway"
        gateway.write_text("#!" + sys.executable + "\nimport json,sys\np=json.load(sys.stdin)\n"
            + "criteria=p['questions']['first']['criteria']\nchoice=" + repr(self.chosen["id"]) + "\n"
            + "print(json.dumps({'available':True,'response':{'model':'jev-1.13.0','usage':{'input_tokens':321,'output_tokens':9},"
            + "'answers':{'first':{'type':'choice','choice':choice,'confidence':1,'probabilities':{k:float(k==choice) for k in criteria}}}}}))\n")
        gateway.chmod(0o700)
        command = [str(BINARY), "--project", str(self.fx.project), "--no-profile", "skills", "host",
            "--ranker", "jev", "--jev-transport", "gateway", "--gateway-command", str(gateway),
            "--gateway-data-class", "public-nonsensitive", "--allow-provider-data", "--max-provider-calls", "1",
            "--candidate-limit", "20", "--event-log", str(self.log)]
        env = dict(os.environ, HOME=str(self.fx.root / "home"), XDG_CONFIG_HOME=str(self.fx.root / "config"))
        env.pop("METACTL_PROFILE", None)
        request = {"jsonrpc":"2.0","id":1,"method":"tools/call","params":{
            "name":"discover_skills","arguments":{"query":self.query}}}
        run = subprocess.run(command, input=json.dumps(request)+"\n", env=env, capture_output=True, text=True, timeout=20)
        self.assertEqual(run.returncode, 0, run.stderr)
        wire = json.loads(run.stdout)
        self.assertFalse(wire["result"]["isError"])
        value = json.loads(wire["result"]["content"][0]["text"])
        self.assertEqual(value["result"]["skills"][0], self.chosen)
        self.assertEqual(len(value["result"]["skills"]), 5)
        self.assertEqual(value["metrics"]["telemetry_status"], "recorded")
        config = subprocess.run(command + ["--client-config"], env=env, capture_output=True, text=True, timeout=15)
        self.assertEqual(config.returncode, 0, config.stderr)
        args = json.loads(config.stdout)["mcpServers"]["metactl-skills"]["args"]
        self.assertEqual(args[args.index("--candidate-limit")+1], "20")
        for invalid in ("4", "21"):
            rejected = subprocess.run(command + ["--candidate-limit", invalid], env=env, capture_output=True, text=True, timeout=10)
            self.assertNotEqual(rejected.returncode, 0)


if __name__ == "__main__": unittest.main()
