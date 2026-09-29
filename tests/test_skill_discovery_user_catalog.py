"""User-catalog opt-in: real CLI, fake gateway, no private data or network."""
import json
import os
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

from test_skill_discovery_host import Fixture, BINARY, host, baseline, response


class UserCatalogTests(unittest.TestCase):
    def setUp(self):
        self.fixture = Fixture()
        self.f = self.fixture
        self.f.add("review-one", "Review production code and tests")
        self.f.add("review-two", "Review source code and quality")
        self.home = self.f.root / "home"
        self.home.mkdir()
        self.env = os.environ.copy()
        self.env.pop("METACTL_PROFILE", None)
        self.env["HOME"] = str(self.home)
        self.env["XDG_CONFIG_HOME"] = str(self.home / ".config")
        self.env.pop("TYPESAFE_API_KEY", None)
        self.catalog_path = self.home / ".config/metactl/discovery-catalog.json"
        self.project_file = self.f.project / "metactl.yaml"

    def tearDown(self):
        self.f.close()

    def run_cli(self, *args, okay=True, input=None, project=None):
        command = [str(BINARY), "--project", str(project or self.f.project),
                   "--no-profile", "--json", "--full", *args]
        result = subprocess.run(command, env=self.env, text=True, input=input,
                                capture_output=True, timeout=20)
        if okay:
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        else:
            self.assertNotEqual(result.returncode, 0)
        return json.loads(result.stdout)

    def setup(self, policy="local-only", replace=False, apply=True):
        return self.run_cli("skills", "setup", "--scope", "user", "--source", str(self.f.library),
                            "--target", "codex-cli", "--metadata-policy", policy,
                            *(["--apply"] if apply else []), *(["--replace"] if replace else []))

    def user(self, *args, **kw):
        return self.run_cli("--catalog-mode", "project-or-user", "--discovery-target", "codex-cli",
                            "skills", *args, **kw)

    def unconfigure(self):
        self.project_file.unlink()

    def test_preview_save_discover_load_and_no_workspace_writes(self):
        before = sorted(p.name for p in self.f.project.iterdir())
        preview = self.setup(apply=False)
        self.assertEqual(preview["action"], "preview")
        self.assertFalse(self.catalog_path.exists())
        saved = self.setup()
        self.assertFalse(saved["workspace_enrollment_changed"])
        self.assertEqual(self.catalog_path.stat().st_mode & 0o777, 0o600)
        self.unconfigure()
        value = self.user("discover", "Review code")
        self.assertEqual(value["discovery_context"]["catalog_origin"], "user")
        selected = value["result"]["skills"][0]
        loaded = self.user("load", selected["id"], "--digest", selected["digest"])
        self.assertEqual(loaded["result"]["instructions"],
                         (self.f.library / "packs" / selected["name"] / "SKILL.md").read_text())
        self.assertEqual(sorted(p.name for p in self.f.project.iterdir()), [])
        self.assertFalse((self.home / ".config/metactl/discovery.json").exists())

    def test_project_behavior_and_empty_project_take_precedence(self):
        project = self.run_cli("skills", "catalog")["result"]
        self.setup()
        self.assertEqual(self.user("catalog")["result"], project)
        self.assertEqual(self.user("catalog")["discovery_context"]["catalog_origin"], "project")
        for p in (self.f.library / "packs").glob("*.json"):
            p.unlink()
        value = self.user("catalog")
        self.assertEqual(value["result"]["skills"], [])
        self.assertEqual(value["discovery_context"]["catalog_origin"], "project")

    def test_configured_overlay_report_uses_existing_effective_target(self):
        overlay = self.f.root / "overlay.json"
        overlay.write_text(json.dumps({"entrypoint": "cli", "selected_target_override": {"kind": "target", "id": "claude-code"}}))
        value = self.run_cli("--overlay", str(overlay), "skills", "catalog")
        self.assertEqual(value["discovery_context"]["effective_target"], "claude-code")
        self.assertEqual(value["discovery_context"]["catalog_origin"], "project")

    def test_invalid_project_dangling_and_explicit_config_do_not_fallback(self):
        self.setup()
        self.project_file.write_text("targets: [")
        self.user("catalog", okay=False)
        self.project_file.unlink()
        self.project_file.symlink_to(self.f.root / "missing")
        self.user("catalog", okay=False)
        self.project_file.unlink()
        self.run_cli("--catalog-mode", "project-or-user", "--discovery-target", "codex-cli",
                     "--config", str(self.f.root / "absent"), "skills", "catalog", okay=False)

    def test_target_source_and_profile_fail_safely(self):
        self.setup()
        self.unconfigure()
        self.run_cli("--catalog-mode", "project-or-user", "skills", "catalog", okay=False)
        self.run_cli("--catalog-mode", "project-or-user", "--discovery-target", "unknown",
                     "skills", "catalog", okay=False)
        self.run_cli("--catalog-mode", "project-or-user", "--discovery-target", "codex-cli",
                     "--overlay", str(self.f.root / "none"), "skills", "catalog", okay=False)
        doc = json.loads(self.catalog_path.read_text())
        doc["sources"] = ["missing-relative-source"]
        self.catalog_path.write_text(json.dumps(doc))
        self.user("catalog", okay=False)

    def test_missing_disabled_and_relative_sources(self):
        self.unconfigure()
        self.user("catalog", okay=False)
        self.setup()
        doc = json.loads(self.catalog_path.read_text())
        doc["sources"] = [os.path.relpath(self.f.library, self.catalog_path.parent)]
        self.catalog_path.write_text(json.dumps(doc))
        self.assertEqual(len(self.user("catalog")["result"]["skills"]), 2)
        doc["fallback_enabled"] = False
        self.catalog_path.write_text(json.dumps(doc))
        self.user("catalog", okay=False)

    def test_persistent_toggle_preserves_policy_sources_and_enrollment(self):
        self.setup(policy="private-owned")
        original = json.loads(self.catalog_path.read_text())
        preview = self.run_cli("skills", "setup", "--disable")
        self.assertFalse(preview["fallback_enabled"])
        self.assertEqual(json.loads(self.catalog_path.read_text()), original)
        self.run_cli("skills", "setup", "--disable", "--apply")
        disabled = json.loads(self.catalog_path.read_text())
        self.assertEqual(disabled, {**original, "fallback_enabled": False})
        self.unconfigure()
        self.user("catalog", okay=False)
        self.run_cli("skills", "setup", "--enable", "--apply")
        self.assertEqual(json.loads(self.catalog_path.read_text()), original)
        self.assertTrue(self.user("catalog")["result"]["skills"])
        self.assertFalse((self.catalog_path.parent / "discovery.json").exists())

    def test_direct_load_exclusion_digest_and_resource_escape(self):
        self.setup()
        self.unconfigure()
        selected = self.user("catalog")["result"]["skills"][0]
        doc = json.loads(self.catalog_path.read_text())
        doc["exclusions"] = [selected["name"]]
        self.catalog_path.write_text(json.dumps(doc))
        self.user("load", selected["id"], "--digest", selected["digest"], okay=False)
        doc["exclusions"] = []
        self.catalog_path.write_text(json.dumps(doc))
        card = self.f.library / "packs" / selected["name"] / "SKILL.md"
        card.write_text(card.read_text() + "\nChanged.")
        self.user("load", selected["id"], "--digest", selected["digest"], okay=False)
        card.unlink()
        card.symlink_to(self.f.project / "secret")
        (self.f.project / "secret").write_text("synthetic fixture must not load")
        self.assertNotIn(selected["id"], [s["id"] for s in self.user("catalog")["result"]["skills"]])

    def fake_preferences(self, policy="private-owned", enroll=True):
        gateway = self.f.root / "fake-gateway"
        gateway.write_text("""#!/usr/bin/env python3
import json,os,sys
from pathlib import Path
wire=json.load(sys.stdin)
Path(os.environ["FAKE_GATEWAY_MARKER"]).write_text(os.getcwd())
ids=list(wire["questions"]["first"]["criteria"])
choice=ids[0]
prob={i: (1.0 if i==choice else 0.0) for i in ids}
print(json.dumps({"available":True,"response":{"model":"jev-1.13.0","answers":{"first":{"type":"choice","choice":choice,"confidence":1.0,"probabilities":prob}},"usage":{"input_tokens":10,"output_tokens":2}}}))
""")
        gateway.chmod(0o700)
        prefs = self.catalog_path.parent / "discovery.json"
        doc = {"schema": 1, "mode": "enabled", "allow_provider_data": True,
               "gateway_command": str(gateway), "max_provider_calls": 4,
               "provider_deadline": 2.0, "projects": {}}
        if enroll:
            doc["projects"][str(self.f.project.resolve())] = {
                "mode": "inherit", "gateway_project": "synthetic-fixture", "data_class": policy}
        prefs.write_text(json.dumps(doc))
        prefs.chmod(0o600)
        self.env["FAKE_GATEWAY_MARKER"] = str(self.f.root / "gateway-marker")
        return prefs

    def host_call(self, *flags):
        return self.run_cli("--catalog-mode", "project-or-user", "skills", "host",
                            "--target", "codex-cli", "--runtime", "codex-cli",
                            *flags, "--call-tool", "discover_skills",
                            input=json.dumps({"query": "Review code and tests"}))

    def test_existing_exact_enrollment_and_metadata_decisions(self):
        self.setup(policy="private-owned")
        self.unconfigure()
        prefs = self.fake_preferences()
        first = self.host_call("--use-preferences")
        self.assertEqual(first["metrics"]["provider_calls"], 1)
        events = list((self.home / ".local/state/metactl/discovery").glob("*.jsonl"))
        self.assertTrue(events)
        event = json.loads(events[0].read_text().splitlines()[0])
        self.assertEqual(event["schema"], "metactl.discovery_trial.v2")
        from skill_discovery_trials import validate_event
        validate_event(event)
        self.assertEqual(first["metrics"]["catalog_origin"], "user")
        self.assertEqual(first["recommendation_status"], "recommended")
        self.assertEqual((self.f.root / "gateway-marker").read_text(), str(self.f.project))
        # Classification changes and revocation apply on the next request.
        self.setup(policy="local-only", replace=True)
        value = self.host_call("--use-preferences")
        self.assertEqual(value["metrics"]["reason"], "candidate_metadata_not_authorized")
        self.assertEqual(value["metrics"]["provider_attempts"], 0)
        self.setup(policy="private-owned", replace=True)
        doc = json.loads(prefs.read_text())
        doc["projects"] = {}
        prefs.write_text(json.dumps(doc))
        value = self.host_call("--use-preferences")
        self.assertEqual(value["metrics"]["reason"], "project_not_enrolled")
        self.assertEqual(value["metrics"]["provider_calls"], 0)

    def test_direct_flags_cannot_bypass_user_catalog_preferences(self):
        self.setup(policy="public-nonsensitive")
        self.unconfigure()
        self.env["TYPESAFE_API_KEY"] = "synthetic-never-used"
        value = self.host_call("--ranker", "jev", "--allow-provider-data",
                               "--max-provider-calls", "2", "--jev-transport", "gateway",
                               "--gateway-command", "/nonexistent", "--gateway-project", "fixture",
                               "--gateway-data-class", "public-nonsensitive")
        self.assertEqual(value["metrics"]["reason"], "user_catalog_requires_preferences")
        self.assertEqual(value["metrics"]["provider_attempts"], 0)

    def test_running_host_detects_origin_change_before_load(self):
        self.setup()
        self.unconfigure()
        with patch.dict(os.environ, self.env, clear=True):
            h = host.Host(str(BINARY), str(self.f.project),
                          cli_args=["--no-profile", "--catalog-mode", "project-or-user",
                                    "--discovery-target", "codex-cli"])
            value = h.call("discover_skills", {"query": "Review code"})
            selected = value["result"]["skills"][0]
            self.f.save_config()
            with self.assertRaises(host.DiscoveryCliError) as error:
                h.call("load_skill", {"id": selected["id"], "digest": selected["digest"]})
            self.assertEqual(error.exception.reason, "catalog_context_changed")

    def test_exact_workspace_and_alias_never_inherit_parent_or_sibling(self):
        self.setup(policy="public-nonsensitive")
        self.unconfigure()
        self.fake_preferences(policy="public-nonsensitive")
        nested = self.f.project / "nested"
        nested.mkdir()
        value = self.run_cli("--catalog-mode", "project-or-user", "skills", "host",
                             "--runtime", "codex-cli", "--use-preferences", "--status", project=nested)
        self.assertEqual(value["preferences"]["reason"], "project_not_enrolled")
        self.assertEqual(value["project"], str(nested))
        alias = self.f.root / "alias"
        alias.symlink_to(self.f.project, target_is_directory=True)
        a = self.user("catalog")["discovery_context"]["context_identity"]
        b = self.run_cli("--catalog-mode", "project-or-user", "--discovery-target", "codex-cli",
                         "skills", "catalog", project=alias)["discovery_context"]["context_identity"]
        self.assertEqual(a, b)

    def test_catalog_replacement_invalidates_existing_host_even_with_same_package(self):
        self.setup()
        self.unconfigure()
        with patch.dict(os.environ, self.env, clear=True):
            h = host.Host(str(BINARY), str(self.f.project),
                          cli_args=["--no-profile", "--catalog-mode", "project-or-user",
                                    "--discovery-target", "codex-cli"])
            selected = h.call("discover_skills", {"query": "Review code"})["result"]["skills"][0]
            doc = json.loads(self.catalog_path.read_text())
            doc["exclusions"] = ["unused-explicit-exclusion"]
            self.catalog_path.write_text(json.dumps(doc))
            with self.assertRaises(host.DiscoveryCliError) as error:
                h.call("load_skill", {"id": selected["id"], "digest": selected["digest"]})
            self.assertEqual(error.exception.reason, "catalog_context_changed")

    def test_client_config_preserves_user_mode_and_runtime_mismatch_fails(self):
        self.setup()
        self.unconfigure()
        config = self.run_cli("--catalog-mode", "project-or-user", "skills", "host",
                              "--target", "codex-cli", "--runtime", "codex-cli", "--client-config")
        args = config["mcpServers"]["metactl-skills"]["args"]
        self.assertIn("project-or-user", args)
        self.assertIn("--discovery-target", args)
        value = self.run_cli("--catalog-mode", "project-or-user", "skills", "host",
                             "--target", "codex-cli", "--runtime", "claude-code", "--call-tool", "discover_skills",
                             input=json.dumps({"query": "Review code"}), okay=False)
        self.assertEqual(value["reason"], "user_catalog_target_mismatch")
        self.assertEqual(value["metrics"]["provider_attempts"], 0)

    def test_fixed_root_connection_preview_apply_doctor_remove(self):
        self.setup()
        self.unconfigure()
        args = ("--catalog-mode", "project-or-user", "skills", "connect", "--scope", "user",
                "--target", "codex-cli", "--use-preferences")
        preview = self.run_cli(*args)
        self.assertFalse((self.home / ".codex/config.toml").exists())
        self.run_cli(*args, "--apply")
        report = self.run_cli("--catalog-mode", "project-or-user", "skills", "doctor",
                              "--scope", "user", "--target", "codex-cli", "--use-preferences")
        # Doctor output nests diagnostic fields in its result.
        self.assertEqual(report["catalog_origin"], "user")
        self.assertTrue(report["catalog_ready"])
        self.assertEqual(report["workspace_resolution"], "exact")
        config = (self.home / ".codex/config.toml").read_text()
        self.assertIn("--catalog-mode", config)
        self.assertIn(str(self.f.project), config)
        self.run_cli(*args, "--remove")


class RecommendationTests(unittest.TestCase):
    def test_abstention_is_no_recommendation_with_compatible_candidates(self):
        answer = response("none")
        answer["answers"]["first"]["probabilities"] = {"a" * 64: .1, "b" * 64: .1, "none": .8}
        ranker = host.Ranker(True, True, 1, key="synthetic", sender=lambda *args: answer)
        h = host.Host("unused", "unused", ranker=ranker, runner=lambda *args: baseline())
        value = h.call("discover_skills", {"query": "Unrelated task"})
        self.assertEqual(value["recommendation_status"], "abstained")
        self.assertEqual(value["recommended_ids"], [])
        self.assertEqual(value["result"], baseline())

    def test_shadow_abstention_public_recommendations_equal_chosen_baseline(self):
        answer = response("none")
        answer["answers"]["first"]["probabilities"] = {"a" * 64: .1, "b" * 64: .1, "none": .8}
        values = []
        for wire in (answer, response()):
            ranker = host.Ranker(True, True, 1, key="synthetic", sender=lambda *args, wire=wire: wire, mode="shadow")
            value = host.Host("unused", "unused", ranker=ranker, runner=lambda *args: baseline()).call(
                "discover_skills", {"query": "Review"})
            values.append((value["result"], value["recommendation_status"], value["recommended_ids"]))
        self.assertEqual(values[0], values[1])

    def test_shadow_never_exposes_advisory_recommendation(self):
        ranker = host.Ranker(True, True, 1, key="synthetic", sender=lambda *args: response(), mode="shadow")
        value = host.Host("unused", "unused", ranker=ranker, runner=lambda *args: baseline()).call(
            "discover_skills", {"query": "Review"})
        self.assertEqual(value["recommended_ids"], [])
        self.assertEqual(value["recommendation_status"], "ranked_candidates")

