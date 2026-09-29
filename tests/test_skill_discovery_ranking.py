"""Public CLI/host regressions for useful skill retrieval, without provider access."""
import json
import unittest

from test_skill_discovery_host import BINARY, Fixture, host


class RetrievalTests(unittest.TestCase):
    def setUp(self):
        self.fx = Fixture()
        self.addCleanup(self.fx.close)

    def card(self, name, aliases=(), positive=("fixture sentinel",), negative=()):
        manifest_path = self.fx.library / "packs" / (name + ".json")
        manifest = json.loads(manifest_path.read_text())
        manifest["resources"].append({"path": f"packs/{name}/skill-card.json", "kind": "example"})
        manifest_path.write_text(json.dumps(manifest))
        card = {"schema_version": "2alpha1", "name": name, "version": "1",
                "summary": "Public retrieval fixture", "aliases": list(aliases),
                "intents": {"positive": list(positive), "negative": list(negative)},
                "facets": {}, "reviewed_relations": [],
                "host_compatibility": {"targets": ["codex-cli"]},
                "provenance": {"source_kind": "first_party", "reviewed_by": "fixture",
                               "reviewed_at": "2026-09-28"}}
        (self.fx.library / "packs" / name / "skill-card.json").write_text(json.dumps(card))

    def test_named_skill_in_prose_survives_cutoff_and_loads_original(self):
        original = self.fx.add("model-route-optimizer", "Choose supported models and shape prompts")
        for number in range(7):
            name = f"agent-work-{number}"
            self.fx.add(name, "Coordinate agent work for a repository")
            self.card(name, aliases=["agent selection"])
        self.assertEqual(len(self.fx.run(["catalog"])["skills"]), 8)
        ranker = host.Ranker(True, True, 1, key="fixture", sender=lambda *a: self.fail("explicit name called provider"))
        h = host.Host(str(BINARY), str(self.fx.project), ranker=ranker, runner=self.fx.run)
        result = h.call("discover_skills", {"query": "model-route-optimizer for SampleApp repo agent route selection"})
        skill = result["result"]["skills"][0]
        self.assertEqual(skill["name"], "model-route-optimizer")
        self.assertEqual(result["metrics"]["provider_calls"], 0)
        loaded = h.call("load_skill", {"id": skill["id"], "digest": skill["digest"]})
        self.assertEqual(loaded["result"]["instructions"], original)

    def test_task_evidence_beats_filler(self):
        self.fx.add("data-recovery", "Verify database backup and isolated restore before production migration")
        for number in range(7):
            self.fx.add(f"notes-and-handoff-{number}", "Use this for an agent and its repository work")
        query = "Verify database backup and isolated restore before a production migration"
        result = self.fx.run(["discover", "--query-stdin"], query)
        self.assertEqual(result["skills"][0]["name"], "data-recovery")
        for query in ("and for the with to", "flamingo balloon astronomy"):
            self.assertEqual(self.fx.run(["discover", "--query-stdin"], query)["skills"], [])

    def test_exact_id_lookup_preserves_case_sensitive_identity(self):
        self.fx.add("data-recovery", "Backup restore")
        skill = self.fx.run(["catalog"])["skills"][0]
        self.assertEqual(self.fx.run(["discover", skill["id"]])["skills"][0]["id"], skill["id"])
        self.assertNotEqual(skill["id"], skill["id"].upper())
        self.assertEqual(self.fx.run(["discover", skill["id"].upper()])["skills"], [])

    def test_negative_intent_needs_its_meaningful_terms(self):
        self.fx.add("data-recovery", "Backup restore")
        self.card("data-recovery", negative=["delete production data"])
        result = self.fx.run(["discover", "backup production"])
        self.assertEqual([s["name"] for s in result["skills"]], ["data-recovery"])
        self.assertEqual(self.fx.run(["discover", "delete production data backup"])["skills"], [])

    def test_alias_mentions_and_identifier_boundaries(self):
        self.fx.add("data-recovery", "Backup restore")
        self.card("data-recovery", aliases=["restore rehearsal"])
        self.fx.add("restore", "restore rehearsal data")
        self.fx.add("data-recovery-extended", "Advanced data recovery")
        self.assertEqual(self.fx.run(["discover", "Please use restore rehearsal for this task"])["skills"][0]["name"], "data-recovery")
        skills = self.fx.run(["discover", "Use $data-recovery-extended for this task"])["skills"]
        self.assertEqual(skills[0]["name"], "data-recovery-extended")
        self.assertTrue(all(s["score"] < 10000 for s in skills if s["name"] != "data-recovery-extended"))

    def test_explicit_name_does_not_bypass_eligibility_or_host_exclusion(self):
        self.fx.add("restricted-recovery", "Backup restore", requires_confirmation=True)
        self.fx.add("hidden-recovery", "Backup restore")
        h = host.Host(str(BINARY), str(self.fx.project), excluded=["hidden-recovery"], runner=self.fx.run)
        result = h.call("discover_skills", {"query": "Use restricted-recovery and hidden-recovery"})
        self.assertEqual(result["result"]["skills"], [])

    def test_bare_task_word_is_not_an_explicit_name(self):
        self.fx.add("review", "Review code tests")
        ordinary = self.fx.run(["discover", "Review tests for a repair"])["skills"][0]
        self.assertLess(ordinary["score"], 10000)
        for query in ("Use $review for this task", "Use `review` for this task", "review"):
            self.assertGreaterEqual(self.fx.run(["discover", query])["skills"][0]["score"], 10000)

    def test_direct_named_exclusion_is_respected(self):
        self.fx.add("data-recovery", "Backup restore")
        for query in ("Do not use data-recovery", "Avoid $data-recovery", "without `data-recovery`"):
            self.assertEqual(self.fx.run(["discover", query])["skills"], [])


if __name__ == "__main__":
    unittest.main()
