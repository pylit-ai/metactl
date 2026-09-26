"""Offline contract tests: fake provider, real CLI, isolated public fixtures."""
import copy
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("skill_host", ROOT / "scripts/skill_discovery_host.py")
host = importlib.util.module_from_spec(spec)
spec.loader.exec_module(host)
BINARY = ROOT / "target/debug/metactl"


class Fixture:
    def __init__(self):
        self.temp = tempfile.TemporaryDirectory(prefix="metactl-discovery-")
        self.root = Path(self.temp.name)
        self.library = self.root / "library"
        self.project = self.root / "project"
        self.project.mkdir()
        for directory in ("roles", "policies", "targets"):
            shutil.copytree(ROOT / "library/starter" / directory, self.library / directory)
        (self.library / "packs").mkdir()
        shutil.copyfile(ROOT / "library/starter/library.json", self.library / "library.json")
        role_path = self.library / "roles/builder.json"
        role = json.loads(role_path.read_text())
        role["default_pack_refs"] = []
        role_path.write_text(json.dumps(role))
        self.config = {"api_version": "0.1.21", "role": "builder", "policy": "brownfield-safe-builder",
                       "targets": ["codex-cli"], "packs": [], "starter_library": [str(self.library)]}
        self.save_config()

    def save_config(self):
        (self.project / "metactl.yaml").write_text(json.dumps(self.config))

    def add(self, name, description, extra="", **changes):
        base = self.library / "packs" / name
        base.mkdir(exist_ok=True)
        text = f"---\nname: {name}\ndescription: {json.dumps(description)}\n{extra}---\n\nOriginal instructions for {name}.\n"
        (base / "SKILL.md").write_text(text)
        manifest = {"kind": "pack", "id": name, "version": "1.0.0", "title": name,
                    "activation_class": "instruction", "side_effect_class": "none",
                    "trust_tier": "org_validated", "compatible_roles": ["builder"],
                    "compatible_targets": ["codex-cli"], "requires_confirmation": False,
                    "resources": [{"path": f"packs/{name}/SKILL.md", "kind": "instruction", "required": True}]}
        manifest.update(changes)
        (self.library / "packs" / (name + ".json")).write_text(json.dumps(manifest))
        return text

    def run(self, args, private_input=None, success=True, skill_command=True):
        if private_input is False:
            success, private_input = False, None
        env = os.environ.copy()
        env.pop("METACTL_PROFILE", None)
        env.pop("XDG_CONFIG_HOME", None)
        env["HOME"] = str(self.root / "isolated-home")
        run = subprocess.run([str(BINARY), "--project", str(self.project), "--no-profile", "--json", "--full",
                              "--no-input", *(["skills"] if skill_command else []), *args], input=private_input, capture_output=True, text=True, env=env, timeout=20)
        if success and run.returncode:
            raise AssertionError(run.stderr + run.stdout)
        if not success:
            return run
        result = json.loads(run.stdout)
        return result["result"] if skill_command else result

    def close(self):
        self.temp.cleanup()


def baseline():
    return {"catalog_digest": "c" * 64, "skills": [
        {"id": "a" * 64, "name": "a", "description": "Tests", "score": 4, "digest": "d" * 64},
        {"id": "b" * 64, "name": "b", "description": "Review", "score": 4, "digest": "e" * 64}]}


def response(choice="b" * 64):
    return {"model": host.MODEL, "answers": {"first": {"type": "choice", "choice": choice,
             "confidence": .8, "probabilities": {"a" * 64: .1, "b" * 64: .8, "none": .1}}},
            "usage": {"input_tokens": 100, "output_tokens": 10}}


class RankerTests(unittest.TestCase):
    def test_embedded_host_matches_canonical_source(self):
        self.assertEqual((ROOT / "scripts/skill_discovery_host.py").read_bytes(),
                         (ROOT / "crates/metactl/assets/skill_discovery_host.py").read_bytes())

    def test_status_is_not_provider_proof(self):
        ranker = host.Ranker(True, True, 1, key="not-a-real-key", sender=lambda *a: self.fail("network"))
        state = host.readiness(ranker)
        self.assertTrue(state["provider_ready"])
        self.assertFalse(state["provider_verified"])
        self.assertNotIn("not-a-real-key", json.dumps(state))
        self.assertFalse(host.readiness(host.Ranker())["provider_ready"])

    def test_synthetic_check_validates_provider_and_preserves_fallback(self):
        ranker = host.Ranker(True, True, 1, key="not-a-real-key", sender=lambda *a: response())
        self.assertTrue(host.check_provider(ranker)["provider_verified"])
        self.assertFalse(host.check_provider(ranker)["provider_verified"])
        self.assertFalse(host.check_provider(host.Ranker())["provider_verified"])

    def test_caller_deadline_does_not_depend_on_communicate_timeout(self):
        real_popen = subprocess.Popen
        def stalled_worker(argv, **kwargs):
            child = real_popen([os.sys.executable, "-c", "import time; time.sleep(5)"], **kwargs)
            communicate = child.communicate
            def delayed(*args, **options):
                host.time.sleep(.8)
                return communicate(*args, **options)
            child.communicate = delayed
            return child
        start = host.time.monotonic()
        with patch.object(host.subprocess, "Popen", side_effect=stalled_worker):
            result, metric = host.Ranker(True, True, 1, .05, key="fake").rank("test", baseline())
        self.assertEqual(result, baseline())
        self.assertEqual(metric["reason"], "deadline")
        self.assertLess(host.time.monotonic() - start, .6)

    def test_private_query_uses_stdin(self):
        sentinel = "private-query-sentinel"
        h = host.Host("metactl", "fixed")
        def run(argv, **kwargs):
            self.assertNotIn(sentinel, " ".join(argv))
            self.assertEqual(kwargs["input"], sentinel)
            return subprocess.CompletedProcess(argv, 0, json.dumps({"result": baseline()}), "")
        with patch.object(host.subprocess, "run", side_effect=run):
            h.call("discover_skills", {"query": sentinel})

    def test_provider_worker_deadline_terminates_real_child(self):
        # Substitute a sleeping local worker; no network is used.
        real_popen = subprocess.Popen
        trace = []
        def sleeping_worker(argv, **kwargs):
            # Descendant inherits output pipes: parent-only kill is insufficient.
            before = host.time.monotonic()
            child = real_popen([os.sys.executable, "-c", "import subprocess,sys,time; subprocess.Popen([sys.executable,'-c','import time; time.sleep(5)']); time.sleep(5)"], **kwargs)
            trace.append(("spawn", host.time.monotonic() - before))
            communicate = child.communicate
            def observed(*args, **options):
                before = host.time.monotonic()
                try:
                    return communicate(*args, **options)
                finally:
                    trace.append(("communicate", options.get("timeout"), host.time.monotonic() - before))
            child.communicate = observed
            return child
        start = host.time.monotonic()
        r = host.Ranker(True, True, 1, .1, key="fake")
        with patch.object(host.subprocess, "Popen", side_effect=sleeping_worker):
            result, metric = r.rank("test", baseline())
        self.assertEqual(result, baseline())
        self.assertEqual(metric["reason"], "deadline")
        self.assertLess(host.time.monotonic() - start, 1.5, trace)

    def test_unavailable_paths_preserve_baseline_without_calls(self):
        for kwargs, reason in [({}, "disabled"), ({"enabled": True}, "data_not_authorized"),
                ({"enabled": True, "allow_data": True}, "missing_credential"),
                ({"enabled": True, "allow_data": True, "key": "test"}, "budget_exhausted")]:
            with self.subTest(reason=reason):
                r = host.Ranker(sender=lambda *a: self.fail("network called"), **kwargs)
                result, metric = r.rank("test", baseline())
                self.assertEqual(result, baseline())
                self.assertEqual(metric["reason"], reason)
                self.assertEqual(metric["provider_calls"], 0)

    def test_failure_schema_and_deadline_fallback(self):
        def fail(*args):
            raise OSError("SECRET must never reach result")
        def timeout(*args):
            raise subprocess.TimeoutExpired("provider", 1)
        malformed = [None, {}, {"answers": {}}, response("unknown")]
        for sender in [fail, timeout, *[lambda *a, value=v: value for v in malformed]]:
            r = host.Ranker(True, True, 1, key="test", sender=sender)
            result, metric = r.rank("test", baseline())
            self.assertEqual(result, baseline())
            self.assertEqual(r.remaining, 0)
            self.assertNotIn("SECRET", json.dumps(metric))

    def test_reorders_without_dropping_candidates(self):
        r = host.Ranker(True, True, 1, key="test", sender=lambda *a: response())
        original = baseline()
        ranked, metric = r.rank("test", original)
        self.assertEqual(original, baseline())
        self.assertEqual([s["name"] for s in ranked["skills"]], ["b", "a"])
        self.assertEqual(metric["reason"], "reordered")
        self.assertEqual(r.rank("test", original)[0], original)

    def test_abstention_and_invalid_probabilities(self):
        for value in (float("nan"), float("inf"), -.1, True):
            fake = response()
            fake["answers"]["first"]["confidence"] = value
            r = host.Ranker(True, True, 1, key="test", sender=lambda *a: fake)
            self.assertEqual(r.rank("test", baseline())[0], baseline())
        fake = response("none")
        fake["answers"]["first"]["probabilities"] = {"a" * 64: .1, "b" * 64: .1, "none": .8}
        r = host.Ranker(True, True, 1, key="test", sender=lambda *a: fake)
        self.assertEqual(r.rank("test", baseline())[1]["reason"], "abstained")

    def test_tiny_tools_no_catalog_or_query_project_override(self):
        h = host.Host("unused", "fixed", runner=lambda args: self.fail("called CLI"))
        self.assertEqual(len(host.tools()), 2)
        self.assertNotIn("enum", json.dumps(host.tools()))
        for args in ({"query": "test", "project": "/other"}, {"query": 1}):
            with self.assertRaises(ValueError):
                h.call("discover_skills", args)
        with self.assertRaises(ValueError):
            h.call("unknown", {})


@unittest.skipUnless(BINARY.exists(), "build CLI before running end-user-path tests")
class CliTests(unittest.TestCase):
    def test_packaged_host_status_config_and_fail_closed_check(self):
        env = {k: v for k, v in os.environ.items() if not k.startswith("METACTL_") and k != "TYPESAFE_API_KEY"}
        env["XDG_CONFIG_HOME"] = str(self.fx.root / "config")
        command = [str(BINARY), "--project", str(self.fx.project), "--no-profile", "skills", "host"]
        result = subprocess.run(command + ["--status"], env=env, capture_output=True, text=True, timeout=20)
        self.assertEqual(result.returncode, 0, result.stderr)
        state = json.loads(result.stdout)
        self.assertTrue(state["project_ready"])
        self.assertFalse(state["provider_verified"])
        result = subprocess.run(command + ["--check", "--ranker", "jev", "--allow-provider-data", "--max-provider-calls", "1"], env=env, capture_output=True, text=True, timeout=20)
        self.assertEqual(result.returncode, 1)
        self.assertFalse(json.loads(result.stdout)["provider_verified"])
        result = subprocess.run(command + ["--client-config"], env=env, capture_output=True, text=True, timeout=20)
        config = json.loads(result.stdout)["mcpServers"]["metactl-skills"]
        self.assertEqual(config["command"], str(BINARY))
        self.assertIn("--no-profile", config["args"])
        self.assertIn("--python", config["args"])
        wire = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}) + "\n"
        result = subprocess.run(command, env=env, input=wire, capture_output=True, text=True, timeout=20)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(result.stdout.splitlines()), 1)
        self.assertEqual(json.loads(result.stdout)["result"]["serverInfo"]["name"], "metactl-skill-discovery")

    def test_packaged_registration_paths_exclusions_and_quiet(self):
        self.fx.add("excluded", "A hidden specialist")
        env = {k: v for k, v in os.environ.items() if not k.startswith("METACTL_") and k != "TYPESAFE_API_KEY"}
        env["XDG_CONFIG_HOME"] = str(self.fx.root / "config")
        command = [str(BINARY), "--project", str(self.fx.project), "--no-profile", "--config", "metactl.yaml", "skills", "host"]
        result = subprocess.run(command + ["--client-config"], cwd=self.fx.project, env=env, capture_output=True, text=True)
        config = json.loads(result.stdout)["mcpServers"]["metactl-skills"]
        emitted_path = Path(config["args"][config["args"].index("--config") + 1])
        self.assertTrue(emitted_path.is_absolute())
        self.assertEqual(emitted_path.resolve(), (self.fx.project / "metactl.yaml").resolve())
        replay = [config["command"], *config["args"], "--exclude-skill", "excluded", "--status"]
        result = subprocess.run(replay, cwd=self.fx.root, env=env, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout)["eligible_skills"], 0)
        result = subprocess.run(replay + ["--quiet"], cwd=self.fx.root, env=env, capture_output=True, text=True)
        self.assertEqual((result.returncode, result.stdout), (0, ""))
        result = subprocess.run([config["command"], *config["args"], "--quiet"], cwd=self.fx.root, env=env, capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)

    def test_core_only_projection_keeps_specialists_discoverable(self):
        self.fx.add("specialist-sentinel", "reconnect failures")
        source = ROOT / "library/starter/packs"
        shutil.copyfile(source / "skill-discovery.json", self.fx.library / "packs/skill-discovery.json")
        shutil.copytree(source / "skill-discovery", self.fx.library / "packs/skill-discovery")
        self.fx.config["packs"] = ["skill-discovery"]
        self.fx.save_config()
        self.fx.run(["sync"], skill_command=False)
        projected = list((self.fx.project / ".agents/skills").rglob("SKILL.md"))
        self.assertEqual(len(projected), 1)
        self.assertIn("name: skill-discovery", projected[0].read_text())
        self.assertNotIn("specialist-sentinel", (self.fx.project / "AGENTS.md").read_text())
        self.assertEqual(self.fx.run(["discover", "reconnect"])["skills"][0]["name"], "specialist-sentinel")

    def setUp(self):
        self.fx = Fixture()
        self.addCleanup(self.fx.close)

    def test_discover_and_load_original_through_host(self):
        original = self.fx.add("probe", "Investigate reconnect failures and stale revisions")
        h = host.Host(str(BINARY), str(self.fx.project), runner=self.fx.run)
        result = h.call("discover_skills", {"query": "reconnect failures"})
        skill = result["result"]["skills"][0]
        loaded = h.call("load_skill", {"id": skill["id"], "digest": skill["digest"]})
        self.assertEqual(loaded["result"]["instructions"], original)
        self.assertFalse(loaded["metrics"]["repeat_load"])
        self.assertTrue(h.call("load_skill", {"id": skill["id"], "digest": skill["digest"]})["metrics"]["repeat_load"])

    def test_stale_missing_and_disabled_source_fail(self):
        self.fx.add("probe", "reconnect")
        skill = self.fx.run(["discover", "probe"])["skills"][0]
        args = ["load", skill["id"], "--digest", skill["digest"]]
        path = self.fx.library / "packs/probe/SKILL.md"
        old = path.read_text()
        path.write_text(old + "changed")
        self.assertNotEqual(self.fx.run(args, False).returncode, 0)
        path.write_text(old.replace("---\n\n", "enabled: false\n---\n\n"))
        self.assertNotEqual(self.fx.run(args, False).returncode, 0)
        path.unlink()
        self.assertNotEqual(self.fx.run(args, False).returncode, 0)

    def test_native_only_and_approval_excluded(self):
        for name, extra, changes in [("manual", "disable-model-invocation: true\n", {}),
                ("disabled", "enabled: false\n", {}), ("tools", "allowed-tools: Bash\n", {}),
                ("approval", "", {"requires_confirmation": True}),
                ("hook", "", {"activation_class": "hook"}),
                ("side-effect", "", {"side_effect_class": "external_write"}),
                ("wrong-target", "", {"compatible_targets": ["cursor"]})]:
            self.fx.add(name, "test", extra, **changes)
        self.fx.add("native-sidecar", "test")
        sidecar = self.fx.library / "packs/native-sidecar/agents"
        sidecar.mkdir()
        (sidecar / "openai.yaml").write_text("policy: {}")
        self.assertEqual(self.fx.run(["catalog"])["skills"], [])

    def test_policy_approval_and_revocation(self):
        self.fx.add("probe", "test")
        skill = self.fx.run(["catalog"])["skills"][0]
        path = self.fx.library / "policies/brownfield-safe-builder.json"
        policy = json.loads(path.read_text())
        policy["rules"].append({"id": "approve-pack", "subject": "pack", "operator": "require_approval",
                                "requested_enforcement_class": "enforceable_local", "selectors": {"ids": ["probe"]}})
        path.write_text(json.dumps(policy))
        self.assertEqual(self.fx.run(["catalog"])["skills"], [])
        self.assertNotEqual(self.fx.run(["load", skill["id"], "--digest", skill["digest"]], False).returncode, 0)

    def test_rejected_retired_candidate_provenance(self):
        self.fx.add("probe", "test")
        skill = self.fx.run(["catalog"])["skills"][0]
        directory = self.fx.library / "provenance"
        directory.mkdir()
        for status in ("candidate", "rejected", "retired", "promoted"):
            provenance = {"api_version":"metactl/v2alpha1", "subject_ref":{"kind":"pack","id":"probe","version":"1.0.0"},
                          "digest":"sha256:test", "origin":"test", "imported_from_ecosystem":"first_party",
                          "review":{"promotion_status":status}}
            (directory / "probe.json").write_text(json.dumps(provenance))
            self.assertEqual(bool(self.fx.run(["catalog"])["skills"]), status == "promoted")
            self.assertEqual(self.fx.run(["load", skill["id"], "--digest", skill["digest"]], False).returncode == 0, status == "promoted")

    def test_more_than_twenty_exclusions_do_not_hide_eligible_match(self):
        for i in range(21):
            self.fx.add(f"match-{i}", "reconnect")
        h = host.Host(str(BINARY), str(self.fx.project), excluded=[f"match-{i}" for i in range(20)], runner=self.fx.run)
        skills = h.call("discover_skills", {"query":"reconnect"})["result"]["skills"]
        self.assertEqual([s["name"] for s in skills], ["match-20"])

    def test_card_prerequisite_and_target_restriction(self):
        self.fx.add("probe", "test")
        manifest_path = self.fx.library / "packs/probe.json"
        manifest = json.loads(manifest_path.read_text())
        manifest["resources"].append({"path":"packs/probe/skill-card.json", "kind":"example"})
        manifest_path.write_text(json.dumps(manifest))
        card = {"schema_version":"2alpha1", "name":"probe", "version":"1", "summary":"test", "aliases":[],
                "intents":{"positive":["test"],"negative":[]}, "facets":{}, "reviewed_relations":[],
                "host_compatibility":{"targets":["codex-cli"]},
                "provenance":{"source_kind":"first_party","reviewed_by":"fixture","reviewed_at":"2026-09-20"}}
        path = self.fx.library / "packs/probe/skill-card.json"
        path.write_text(json.dumps(card))
        self.assertEqual(len(self.fx.run(["catalog"])["skills"]), 1)
        card["reviewed_relations"] = [{"type":"requires","target":"other", "reviewed_by":"fixture","reviewed_at":"2026-09-20"}]
        path.write_text(json.dumps(card))
        self.assertEqual(self.fx.run(["catalog"])["skills"], [])
        card["reviewed_relations"] = []
        card["host_compatibility"]["targets"] = ["cursor"]
        path.write_text(json.dumps(card))
        self.assertEqual(self.fx.run(["catalog"])["skills"], [])

    def test_package_reference_changes_digest_and_symlinks_rejected(self):
        self.fx.add("probe", "test")
        manifest_path = self.fx.library / "packs/probe.json"
        manifest = json.loads(manifest_path.read_text())
        manifest["resources"].append({"path": "packs/probe/guide.md", "kind": "example"})
        manifest_path.write_text(json.dumps(manifest))
        guide = self.fx.library / "packs/probe/guide.md"
        guide.write_text("first")
        skill = self.fx.run(["catalog"])["skills"][0]
        guide.write_text("other")
        self.assertNotEqual(self.fx.run(["load", skill["id"], "--digest", skill["digest"]], False).returncode, 0)
        guide.unlink()
        outside = self.fx.root / "outside.md"
        outside.write_text("not admitted")
        guide.symlink_to(outside)
        self.assertEqual(self.fx.run(["catalog"])["skills"], [])

    def test_host_exclusion_applies_to_direct_load(self):
        self.fx.add("probe", "test")
        skill = self.fx.run(["catalog"])["skills"][0]
        h = host.Host(str(BINARY), str(self.fx.project), excluded=["probe"], runner=self.fx.run)
        self.assertEqual(h.call("discover_skills", {"query": "probe"})["result"]["skills"], [])
        with self.assertRaises(ValueError):
            h.call("load_skill", {"id": skill["id"], "digest": skill["digest"]})

    def test_stdio_actual_cli_without_provider(self):
        self.fx.add("probe", "test")
        request = {"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                   "params": {"name": "discover_skills", "arguments": {"query": "probe"}}}
        env = os.environ.copy()
        env.pop("METACTL_PROFILE", None)
        env["HOME"] = str(self.fx.root / "empty-home")
        env["XDG_CONFIG_HOME"] = str(self.fx.root / "empty-config")
        p = subprocess.run([os.sys.executable, str(ROOT / "scripts/skill_discovery_host.py"),
                            "--metactl", str(BINARY), "--project", str(self.fx.project)],
                            input=json.dumps(request) + "\n", capture_output=True, text=True, env=env, timeout=20)
        self.assertEqual(p.returncode, 0, p.stderr)
        self.assertFalse(json.loads(p.stdout)["result"]["isError"], p.stdout)


if __name__ == "__main__":
    unittest.main()
