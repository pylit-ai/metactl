"""Real CLI first-run checks for target-native discovery configuration."""

import json
import os
import pathlib
import re
import shlex
import shutil
import stat
import subprocess
import tempfile
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
BINARY = pathlib.Path(os.environ.get("METACTL_TEST_BINARY", ROOT / "target/debug/metactl")).resolve()
PATHS = {
    "codex-cli": ".codex/config.toml",
    "claude-code": ".mcp.json",
    "cursor": ".cursor/mcp.json",
    "gemini-cli": ".gemini/settings.json",
    "opencode": "opencode.json",
}


class ConnectionFirstRun(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="metactl-connect-test-")
        self.addCleanup(self.temp.cleanup)
        self.base = pathlib.Path(self.temp.name)
        self.project = self.base / "project"
        self.project.mkdir()
        self.env = dict(os.environ, XDG_CONFIG_HOME=str(self.base / "config"),
                        XDG_STATE_HOME=str(self.base / "state"))
        result = self.run_cli("init", "-t", "codex-cli", "--no-input")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def run_cli(self, *args, input_text=None):
        return subprocess.run([str(BINARY), "--project", str(self.project), *args],
                              env=self.env, text=True, capture_output=True, input=input_text,
                              timeout=25)

    def test_all_verified_formats_preview_apply_doctor_remove(self):
        for target, relative in PATHS.items():
            with self.subTest(target=target):
                path = self.project / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                original = 'model = "kept"\n' if target == "codex-cli" else '{"other":{"kept":true}}\n'
                path.write_text(original)
                preview = self.run_cli("skills", "connect", "--target", target, "--json")
                self.assertEqual(preview.returncode, 0, preview.stdout + preview.stderr)
                self.assertEqual(path.read_text(), original)
                self.assertEqual(json.loads(preview.stdout)["provider_calls"], 0)
                preview_args = json.loads(preview.stdout)["args"]
                self.assertTrue(pathlib.Path(preview_args[preview_args.index("--python") + 1]).is_absolute())
                apply = self.run_cli("skills", "connect", "--target", target, "--apply", "--json")
                self.assertEqual(apply.returncode, 0, apply.stdout + apply.stderr)
                connected = path.read_text()
                self.assertIn("metactl-skills", connected)
                if target == "codex-cli":
                    self.assertIn('model = "kept"', connected)
                else:
                    document = json.loads(connected)
                    self.assertEqual(document["other"]["kept"], True)
                    envelope = "mcp" if target == "opencode" else "mcpServers"
                    self.assertIn("metactl-skills", document[envelope])
                doctor = self.run_cli("skills", "doctor", "--target", target, "--json")
                self.assertEqual(doctor.returncode, 0, doctor.stdout + doctor.stderr)
                state = json.loads(doctor.stdout)
                self.assertEqual(state["registration"], "configured")
                self.assertEqual(state["host"], "ready")
                self.assertEqual(state["routing"], "unknown_no_event")
                self.assertEqual(state["agent_tools"], "unknown")
                self.assertEqual(state["check_provider_calls"], 0)
                remove = self.run_cli("skills", "connect", "--target", target, "--remove", "--json")
                self.assertEqual(remove.returncode, 0, remove.stdout + remove.stderr)
                self.assertNotIn("metactl-skills", path.read_text())
                if target != "codex-cli":
                    self.assertEqual(json.loads(path.read_text())["other"]["kept"], True)

    def test_conflict_refusal_and_manual_targets(self):
        path = self.project / ".codex/config.toml"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text('[mcp_servers.metactl-skills]\ncommand = "other"\n')
        result = self.run_cli("skills", "connect", "--target", "codex-cli", "--apply")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unmanaged", result.stderr)
        self.assertIn('command = "other"', path.read_text())
        manual = self.run_cli("skills", "connect", "--target", "openclaw")
        self.assertNotEqual(manual.returncode, 0)
        self.assertIn("manual adapter", manual.stderr)

    def test_json_conflict_and_foreign_project_entry_are_refused(self):
        path = self.project / ".cursor/mcp.json"
        path.parent.mkdir(parents=True, exist_ok=True)
        foreign = '{"mcpServers":{"metactl-skills":{"command":"other","args":[]}}}\n'
        path.write_text(foreign)
        refused = self.run_cli("skills", "connect", "--target", "cursor", "--apply")
        self.assertNotEqual(refused.returncode, 0)
        self.assertEqual(path.read_text(), foreign)
        path.unlink()
        applied = self.run_cli("skills", "connect", "--target", "cursor", "--apply")
        self.assertEqual(applied.returncode, 0, applied.stdout + applied.stderr)
        other = self.base / "other-project"
        other.mkdir()
        initialized = subprocess.run([str(BINARY), "--project", str(other), "init", "-t",
                                      "codex-cli", "--no-input"], env=self.env, text=True,
                                     capture_output=True, timeout=25)
        self.assertEqual(initialized.returncode, 0, initialized.stdout + initialized.stderr)
        other_config = other / ".cursor/mcp.json"
        other_config.parent.mkdir(parents=True, exist_ok=True)
        other_config.write_text(path.read_text())
        before = other_config.read_text()
        wrong_project = subprocess.run([str(BINARY), "--project", str(other), "skills", "connect",
                                        "--target", "cursor", "--apply"], env=self.env, text=True,
                                       capture_output=True, timeout=25)
        self.assertNotEqual(wrong_project.returncode, 0)
        self.assertEqual(other_config.read_text(), before)

    def test_wrong_project_cannot_be_connected(self):
        uninitialized = self.base / "uninitialized"
        uninitialized.mkdir()
        result = subprocess.run([str(BINARY), "--project", str(uninitialized), "skills", "connect",
                                 "--target", "codex-cli", "--apply"], env=self.env, text=True,
                                capture_output=True, timeout=25)
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((uninitialized / ".codex/config.toml").exists())

    def test_real_baseline_receipt_is_visible_in_doctor(self):
        applied = self.run_cli("skills", "connect", "--target", "codex-cli", "--apply", "--json")
        self.assertEqual(applied.returncode, 0, applied.stdout + applied.stderr)
        config = (self.project / ".codex/config.toml").read_text()
        command = json.loads(re.search(r'^command = (.+)$', config, re.MULTILINE).group(1))
        args = json.loads(re.search(r'^args = (.+)$', config, re.MULTILINE).group(1))
        call = subprocess.run([command, *args, "--call-tool", "discover_skills"],
                              env=self.env, text=True, capture_output=True, timeout=25,
                              input=json.dumps({"query": "Review a small CLI user workflow"}))
        self.assertEqual(call.returncode, 0, call.stdout + call.stderr)
        result = json.loads(call.stdout)
        self.assertIn("mode=baseline", result["routing_receipt"])
        self.assertIn("provider_calls=0", result["routing_receipt"])
        self.assertIn("log=recorded", result["routing_receipt"])
        doctor = self.run_cli("skills", "doctor", "--target", "codex-cli", "--json")
        self.assertEqual(doctor.returncode, 0, doctor.stdout + doctor.stderr)
        observed = json.loads(doctor.stdout)
        self.assertEqual(observed["routing"], "observed")
        self.assertEqual(observed["latest_discovery"]["provider_calls"], 0)
        self.assertEqual(observed["matching_discoveries"], 1)
        self.assertEqual(observed["agent_tools"], "unknown")

    def test_no_profile_printed_rollback_and_doctor(self):
        apply = self.run_cli("--no-profile", "skills", "connect", "--target", "claude-code",
                             "--apply", "--json")
        self.assertEqual(apply.returncode, 0, apply.stdout + apply.stderr)
        receipt = json.loads(apply.stdout)
        doctor = self.run_cli("skills", "doctor", "--target", "claude-code", "--json")
        self.assertEqual(doctor.returncode, 0, doctor.stdout + doctor.stderr)
        state = json.loads(doctor.stdout)
        self.assertEqual(state["registration"], "configured")
        self.assertEqual(state["host"], "unknown_registration_drift")
        self.assertEqual(state["registration_matches_requested_options"], False)
        matched = self.run_cli("--no-profile", "skills", "doctor", "--target", "claude-code", "--json")
        self.assertEqual(matched.returncode, 0, matched.stdout + matched.stderr)
        self.assertEqual(json.loads(matched.stdout)["host"], "ready")
        rollback = shlex.split(receipt["rollback"])
        self.assertEqual(rollback.pop(0), str(BINARY))
        removed = subprocess.run([str(BINARY), *rollback], env=self.env, text=True,
                                 capture_output=True, timeout=25)
        self.assertEqual(removed.returncode, 0, removed.stdout + removed.stderr)
        self.assertNotIn("metactl-skills", (self.project / ".mcp.json").read_text())

    def test_binary_and_python_drift_can_be_updated_and_removed(self):
        for target in ("codex-cli", "cursor"):
            with self.subTest(target=target):
                path = self.project / PATHS[target]
                applied = self.run_cli("skills", "connect", "--target", target, "--apply", "--json")
                self.assertEqual(applied.returncode, 0, applied.stdout + applied.stderr)
                applied_args = json.loads(applied.stdout)["args"]
                python_path = applied_args[applied_args.index("--python") + 1]
                content = path.read_text().replace(str(BINARY), "/old/metactl")
                content = content.replace(json.dumps(python_path), '"/old/python3"')
                path.write_text(content)
                doctor = self.run_cli("skills", "doctor", "--target", target, "--json")
                self.assertEqual(doctor.returncode, 0, doctor.stdout + doctor.stderr)
                state = json.loads(doctor.stdout)
                self.assertEqual(state["registration"], "configured")
                self.assertEqual(state["host"], "unknown_registration_drift")
                self.assertGreater(state["catalog_eligible_skills"], 0)
                self.assertEqual(state["registration_drift"], "command_differs")
                updated = self.run_cli("skills", "connect", "--target", target, "--apply", "--json")
                self.assertEqual(updated.returncode, 0, updated.stdout + updated.stderr)
                self.assertEqual(json.loads(updated.stdout)["action"], "updated")
                self.assertNotIn("/old/metactl", path.read_text())
                path.write_text(path.read_text().replace(str(BINARY), "/old/metactl"))
                removed = self.run_cli("skills", "connect", "--target", target, "--remove")
                self.assertEqual(removed.returncode, 0, removed.stdout + removed.stderr)
                self.assertNotIn("metactl-skills", path.read_text())

    def test_missing_python_refuses_without_writing(self):
        path = self.project / ".codex/config.toml"
        result = self.run_cli("skills", "connect", "--target", "codex-cli",
                              "--python", "/missing/python3", "--apply")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Python", result.stderr)
        self.assertFalse(path.exists())

    def test_tracked_config_and_symlink_are_refused(self):
        path = self.project / ".codex/config.toml"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text('model = "kept"\n')
        subprocess.run(["git", "init", "-q", str(self.project)], check=True, capture_output=True)
        subprocess.run(["git", "-C", str(self.project), "add", "-f", ".codex/config.toml"],
                       check=True, capture_output=True)
        tracked = self.run_cli("skills", "connect", "--target", "codex-cli", "--apply")
        self.assertNotEqual(tracked.returncode, 0)
        self.assertIn("tracked by Git", tracked.stderr)
        self.assertEqual(path.read_text(), 'model = "kept"\n')
        path.unlink()
        path.symlink_to(self.base / "outside.toml")
        linked = self.run_cli("skills", "connect", "--target", "codex-cli", "--apply")
        self.assertNotEqual(linked.returncode, 0)
        self.assertIn("symlink", linked.stderr)

    def test_quoted_codex_key_and_invalid_toml_are_refused(self):
        path = self.project / ".codex/config.toml"
        path.parent.mkdir(parents=True, exist_ok=True)
        original = '[mcp_servers."metactl-skills"]\ncommand = "other"\n'
        path.write_text(original)
        conflict = self.run_cli("skills", "connect", "--target", "codex-cli", "--apply")
        self.assertNotEqual(conflict.returncode, 0)
        self.assertIn("unmanaged", conflict.stderr)
        self.assertEqual(path.read_text(), original)
        removal = self.run_cli("skills", "connect", "--target", "codex-cli", "--remove")
        self.assertNotEqual(removal.returncode, 0)
        self.assertIn("unmanaged", removal.stderr)
        self.assertEqual(path.read_text(), original)
        path.write_text("model = [\n")
        invalid = self.run_cli("skills", "connect", "--target", "codex-cli", "--apply")
        self.assertNotEqual(invalid.returncode, 0)
        self.assertIn("invalid TOML", invalid.stderr)
        self.assertEqual(path.read_text(), "model = [\n")

    def test_codex_remove_refuses_reparenting_unrelated_settings(self):
        path = self.project / ".codex/config.toml"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text('[mcp_servers.other]\ncommand = "other"\n')
        applied = self.run_cli("skills", "connect", "--target", "codex-cli", "--apply")
        self.assertEqual(applied.returncode, 0, applied.stdout + applied.stderr)
        original = path.read_text()
        path.write_text(original + "enabled = false\n")
        changed = path.read_text()
        doctor = self.run_cli("skills", "doctor", "--target", "codex-cli", "--json")
        self.assertEqual(doctor.returncode, 0, doctor.stdout + doctor.stderr)
        self.assertEqual(json.loads(doctor.stdout)["registration"], "conflict")
        for flags in ((), ("--remove",)):
            with self.subTest(flags=flags):
                result = self.run_cli("skills", "connect", "--target", "codex-cli", *flags)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("outside the managed block", result.stderr)
                self.assertEqual(path.read_text(), changed)
        path.write_text(original)
        removed = self.run_cli("skills", "connect", "--target", "codex-cli", "--remove")
        self.assertEqual(removed.returncode, 0, removed.stdout + removed.stderr)
        self.assertIn('command = "other"', path.read_text())

    def test_codex_client_rewrite_keeps_managed_rollback_usable(self):
        path = self.project / ".codex/config.toml"
        applied = self.run_cli("skills", "connect", "--target", "codex-cli", "--apply")
        self.assertEqual(applied.returncode, 0, applied.stdout + applied.stderr)
        original = path.read_text()
        client_table = '[projects."/tmp/codex-trust"]\ntrust_level = "trusted"\n'
        rewritten = original.replace("# metactl-discovery:end", client_table + "# metactl-discovery:end")
        path.write_text(rewritten)
        doctor = self.run_cli("skills", "doctor", "--target", "codex-cli", "--json")
        self.assertEqual(doctor.returncode, 0, doctor.stdout + doctor.stderr)
        self.assertEqual(json.loads(doctor.stdout)["registration"], "configured")
        removed = self.run_cli("skills", "connect", "--target", "codex-cli", "--remove")
        self.assertEqual(removed.returncode, 0, removed.stdout + removed.stderr)
        self.assertNotIn("metactl-skills", path.read_text())
        self.assertIn(client_table, path.read_text())
        path.write_text(rewritten)
        updated = self.run_cli("skills", "connect", "--target", "codex-cli", "--apply")
        self.assertEqual(updated.returncode, 0, updated.stdout + updated.stderr)
        self.assertIn(client_table, path.read_text())
        self.assertIn("metactl-skills", path.read_text())

    def test_profile_change_requires_explicit_replace(self):
        path = self.project / ".cursor/mcp.json"
        applied = self.run_cli("--no-profile", "skills", "connect", "--target", "cursor", "--apply")
        self.assertEqual(applied.returncode, 0, applied.stdout + applied.stderr)
        original = path.read_text()
        refused = self.run_cli("skills", "connect", "--target", "cursor", "--apply")
        self.assertNotEqual(refused.returncode, 0)
        self.assertIn("--replace", refused.stderr)
        self.assertEqual(path.read_text(), original)
        preview = self.run_cli("skills", "connect", "--target", "cursor", "--replace", "--json")
        self.assertEqual(preview.returncode, 0, preview.stdout + preview.stderr)
        self.assertEqual(path.read_text(), original)
        changed = self.run_cli("skills", "connect", "--target", "cursor", "--apply", "--replace")
        self.assertEqual(changed.returncode, 0, changed.stdout + changed.stderr)
        self.assertNotEqual(path.read_text(), original)

    def test_user_scope_copyable_rollback(self):
        codex_home = self.base / "codex home"
        self.env["CODEX_HOME"] = str(codex_home)
        applied = self.run_cli("skills", "connect", "--target", "codex-cli",
                               "--scope", "user", "--apply", "--json")
        self.assertEqual(applied.returncode, 0, applied.stdout + applied.stderr)
        receipt = json.loads(applied.stdout)
        self.assertIn("metactl-skills", (codex_home / "config.toml").read_text())
        self.assertIn("CODEX_HOME=", receipt["rollback"])
        env = dict(self.env, PATH=f"{BINARY.parent}:{self.env.get('PATH', '')}")
        removed = subprocess.run(receipt["rollback"], shell=True, env=env, text=True,
                                 capture_output=True, timeout=25)
        self.assertEqual(removed.returncode, 0, removed.stdout + removed.stderr)
        self.assertNotIn("metactl-skills", (codex_home / "config.toml").read_text())

    def test_user_scope_rollback_after_project_is_deleted(self):
        codex_home = self.base / "codex-home"
        self.env["CODEX_HOME"] = str(codex_home)
        applied = self.run_cli("skills", "connect", "--target", "codex-cli",
                               "--scope", "user", "--apply", "--json")
        self.assertEqual(applied.returncode, 0, applied.stdout + applied.stderr)
        rollback = json.loads(applied.stdout)["rollback"]
        shutil.rmtree(self.project)
        removed = subprocess.run(rollback, shell=True, env=self.env, text=True,
                                 capture_output=True, timeout=25)
        self.assertEqual(removed.returncode, 0, removed.stdout + removed.stderr)
        self.assertNotIn("metactl-skills", (codex_home / "config.toml").read_text())

    def test_existing_config_permissions_are_preserved(self):
        path = self.project / ".cursor/mcp.json"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text('{"other":{"kept":true}}\n')
        path.chmod(0o640)
        applied = self.run_cli("skills", "connect", "--target", "cursor", "--apply")
        self.assertEqual(applied.returncode, 0, applied.stdout + applied.stderr)
        self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o640)

    def test_doctor_reports_states_when_python_or_config_is_unavailable(self):
        path = self.project / ".codex/config.toml"
        applied = self.run_cli("skills", "connect", "--target", "codex-cli", "--apply")
        self.assertEqual(applied.returncode, 0, applied.stdout + applied.stderr)
        no_python = self.run_cli("skills", "doctor", "--target", "codex-cli",
                                 "--python", "missing-python-for-test", "--json")
        self.assertEqual(no_python.returncode, 0, no_python.stdout + no_python.stderr)
        state = json.loads(no_python.stdout)
        self.assertEqual(state["registration"], "configured")
        self.assertEqual(state["host"], "unavailable_python")
        self.assertGreater(state["catalog_eligible_skills"], 0)
        target = self.base / "external-config.toml"
        path.rename(target)
        path.symlink_to(target)
        linked = self.run_cli("skills", "doctor", "--target", "codex-cli", "--json")
        self.assertEqual(linked.returncode, 0, linked.stdout + linked.stderr)
        self.assertEqual(json.loads(linked.stdout)["registration"], "symlink_refused")

    def test_preview_does_not_create_client_or_state_files(self):
        preview = self.run_cli("skills", "connect", "--target", "codex-cli")
        self.assertEqual(preview.returncode, 0, preview.stdout + preview.stderr)
        self.assertFalse((self.project / ".codex").exists())
        self.assertFalse((self.base / "state").exists())


if __name__ == "__main__":
    if not BINARY.exists():
        raise SystemExit(f"build metactl first: {BINARY}")
    unittest.main()
