"""Real CLI first-run checks for target-native discovery configuration."""

import json
import os
import pathlib
import re
import shlex
import shutil
import stat
import subprocess
import sys
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
                before_preview = {str(item.relative_to(self.base)): item.read_bytes()
                                  for item in self.base.rglob("*") if item.is_file()}
                preview = self.run_cli("skills", "connect", "--target", target, "--json")
                self.assertEqual(preview.returncode, 0, preview.stdout + preview.stderr)
                self.assertEqual(path.read_text(), original)
                after_preview = {str(item.relative_to(self.base)): item.read_bytes()
                                 for item in self.base.rglob("*") if item.is_file()}
                self.assertEqual(after_preview, before_preview)
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
        settings = json.loads(applied.stdout)
        config = (self.project / ".codex/config.toml").read_text()
        command = json.loads(re.search(r'^command = (.+)$', config, re.MULTILINE).group(1))
        args = json.loads(re.search(r'^args = (.+)$', config, re.MULTILINE).group(1))
        wire = "\n".join(json.dumps(request) for request in (
            {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}},
            {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}},
        )) + "\n"
        mcp = subprocess.run([command, *args], env=self.env, text=True,
                             input=wire, capture_output=True, timeout=25)
        self.assertEqual(mcp.returncode, 0, mcp.stdout + mcp.stderr)
        responses = [json.loads(line) for line in mcp.stdout.splitlines()]
        self.assertEqual(responses[0]["result"]["serverInfo"]["name"], "metactl-skill-discovery")
        tool_names = {tool["name"] for tool in responses[1]["result"]["tools"]}
        self.assertTrue({"discover_skills", "load_skill"}.issubset(tool_names))
        call = subprocess.run([command, *args, "--call-tool", "discover_skills"],
                              env=self.env, text=True, capture_output=True, timeout=25,
                              input=json.dumps({"query": "Review a small CLI user workflow"}))
        self.assertEqual(call.returncode, 0, call.stdout + call.stderr)
        result = json.loads(call.stdout)
        self.assertIn("mode=baseline", result["routing_receipt"])
        self.assertIn("provider_calls=0", result["routing_receipt"])
        self.assertIn("log=recorded", result["routing_receipt"])
        ledger = pathlib.Path(settings["event_log"]).read_text()
        self.assertNotIn("Review a small CLI user workflow", ledger)
        self.assertTrue(all("query" not in record and "instructions" not in record
                            for record in map(json.loads, ledger.splitlines())))
        if os.name == "posix":
            log_path = pathlib.Path(settings["event_log"])
            self.assertEqual(stat.S_IMODE(log_path.parent.stat().st_mode), 0o700)
            self.assertEqual(stat.S_IMODE(log_path.stat().st_mode), 0o600)
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
                self.assertEqual(state["registration_drift"], "registered_command_missing")
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
        path.write_text("model = [private-token-canary\n")
        invalid = self.run_cli("skills", "connect", "--target", "codex-cli", "--apply")
        self.assertNotEqual(invalid.returncode, 0)
        self.assertIn("invalid TOML", invalid.stderr)
        self.assertNotIn("private-token-canary", invalid.stderr)
        self.assertEqual(path.read_text(), "model = [private-token-canary\n")

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

    def test_codex_comment_rewrite_keeps_semantic_rollback_usable(self):
        path = self.project / ".codex/config.toml"
        applied = self.run_cli("skills", "connect", "--target", "codex-cli", "--apply")
        self.assertEqual(applied.returncode, 0, applied.stdout + applied.stderr)
        original = path.read_text()
        for rewritten in (original.replace("# metactl-discovery:begin\n", "")
                                   .replace("# metactl-discovery:end\n", ""),
                          original.replace("# metactl-discovery:begin\n", "")):
            with self.subTest(rewritten=rewritten.count("metactl-discovery:")):
                path.write_text(rewritten)
                doctor = self.run_cli("skills", "doctor", "--target", "codex-cli", "--json")
                self.assertEqual(doctor.returncode, 0, doctor.stdout + doctor.stderr)
                self.assertEqual(json.loads(doctor.stdout)["registration"], "configured")
                removed = self.run_cli("skills", "connect", "--target", "codex-cli", "--remove")
                self.assertEqual(removed.returncode, 0, removed.stdout + removed.stderr)
                self.assertNotIn("metactl-skills", path.read_text())

    def test_profile_change_requires_explicit_replace(self):
        path = self.project / ".cursor/mcp.json"
        applied = self.run_cli("--no-profile", "skills", "connect", "--target", "cursor", "--apply")
        self.assertEqual(applied.returncode, 0, applied.stdout + applied.stderr)
        original = path.read_text()
        doctor = self.run_cli("skills", "doctor", "--target", "cursor")
        self.assertEqual(doctor.returncode, 0, doctor.stdout + doctor.stderr)
        self.assertIn("--apply --replace", doctor.stdout)
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
        doctor = self.run_cli("skills", "doctor", "--target", "codex-cli",
                              "--scope", "user", "--json")
        self.assertEqual(doctor.returncode, 0, doctor.stdout + doctor.stderr)
        self.assertEqual(json.loads(doctor.stdout)["registration"], "configured")
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

    def test_user_scope_wrong_project_names_owner(self):
        codex_home = self.base / "codex-home"
        self.env["CODEX_HOME"] = str(codex_home)
        applied = self.run_cli("skills", "connect", "--target", "codex-cli",
                               "--scope", "user", "--apply")
        self.assertEqual(applied.returncode, 0, applied.stdout + applied.stderr)
        other = self.base / "other-project"
        other.mkdir()
        wrong = subprocess.run([str(BINARY), "--project", str(other), "skills", "connect",
                                "--target", "codex-cli", "--scope", "user", "--remove"],
                               env=self.env, text=True, capture_output=True, timeout=25)
        self.assertNotEqual(wrong.returncode, 0)
        self.assertIn("pinned to project", wrong.stderr)
        self.assertIn(str(self.project.resolve()), wrong.stderr)
        self.assertIn("metactl-skills", (codex_home / "config.toml").read_text())

    def test_user_scope_refuses_tracked_dotfile_config(self):
        dotfiles = self.base / "dotfiles"
        codex_home = dotfiles / ".codex"
        codex_home.mkdir(parents=True)
        config = codex_home / "config.toml"
        config.write_text('model = "kept"\n')
        subprocess.run(["git", "init", "-q", str(dotfiles)], check=True)
        subprocess.run(["git", "-C", str(dotfiles), "add", ".codex/config.toml"], check=True)
        self.env["CODEX_HOME"] = str(codex_home)
        refused = self.run_cli("skills", "connect", "--target", "codex-cli",
                               "--scope", "user", "--apply")
        self.assertNotEqual(refused.returncode, 0)
        self.assertIn("tracked by Git", refused.stderr)
        self.assertEqual(config.read_text(), 'model = "kept"\n')

    def test_doctor_distinguishes_host_failure_and_missing_registered_paths(self):
        fake_python = self.base / "old-python"
        fake_python.write_text("#!/bin/sh\nexit 3\n")
        fake_python.chmod(0o755)
        refused = self.run_cli("skills", "connect", "--target", "codex-cli",
                               "--python", str(fake_python), "--apply")
        self.assertNotEqual(refused.returncode, 0)
        self.assertIn("host_failed", refused.stderr)
        failed = self.run_cli("skills", "doctor", "--target", "codex-cli",
                              "--python", str(fake_python), "--json")
        self.assertEqual(failed.returncode, 0, failed.stdout + failed.stderr)
        self.assertEqual(json.loads(failed.stdout)["host"], "host_failed")
        applied = self.run_cli("skills", "connect", "--target", "codex-cli",
                               "--apply", "--json")
        self.assertEqual(applied.returncode, 0, applied.stdout + applied.stderr)
        receipt = json.loads(applied.stdout)
        config = self.project / ".codex/config.toml"
        original = config.read_text()
        missing_command = str(self.base / "missing/metactl")
        config.write_text(original.replace(json.dumps(receipt["command"]),
                                           json.dumps(missing_command)))
        missing = self.run_cli("skills", "doctor", "--target", "codex-cli", "--json")
        state = json.loads(missing.stdout)
        self.assertEqual(state["registered_command_state"], "missing")
        self.assertEqual(state["registration_drift"], "registered_command_missing")
        python = receipt["args"][receipt["args"].index("--python") + 1]
        config.write_text(original.replace(json.dumps(python),
                                           json.dumps(str(self.base / "missing/python3"))))
        missing = self.run_cli("skills", "doctor", "--target", "codex-cli", "--json")
        state = json.loads(missing.stdout)
        self.assertEqual(state["registered_python_state"], "missing")
        self.assertEqual(state["registration_drift"], "registered_python_missing")

    def test_existing_config_permissions_are_preserved(self):
        path = self.project / ".cursor/mcp.json"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text('{"other":{"kept":true}}\n')
        path.chmod(0o640)
        applied = self.run_cli("skills", "connect", "--target", "cursor", "--apply")
        self.assertEqual(applied.returncode, 0, applied.stdout + applied.stderr)
        self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o640)

    def test_json_unrelated_values_and_64bit_integer_are_preserved(self):
        path = self.project / ".cursor/mcp.json"
        path.parent.mkdir(parents=True, exist_ok=True)
        number = 12345678901234567890
        path.write_text('{"z":1,"a":{"large":' + str(number) + '},"mcpServers":{"other":{"command":"other"}}}\n')
        applied = self.run_cli("skills", "connect", "--target", "cursor", "--apply")
        self.assertEqual(applied.returncode, 0, applied.stdout + applied.stderr)
        result = path.read_text()
        self.assertEqual(json.loads(result)["z"], 1)
        self.assertEqual(json.loads(result)["a"]["large"], number)
        self.assertEqual(json.loads(result)["mcpServers"]["other"]["command"], "other")

    def test_json_integer_outside_64_bits_refuses_lossy_rewrite(self):
        path = self.project / ".cursor/mcp.json"
        path.parent.mkdir(parents=True, exist_ok=True)
        original = '{"other":{"large":18446744073709551616,"label":"123456789012345678901"}}\n'
        path.write_text(original)
        refused = self.run_cli("skills", "connect", "--target", "cursor", "--apply")
        self.assertNotEqual(refused.returncode, 0)
        self.assertIn("outside the exact 64-bit range", refused.stderr)
        self.assertEqual(path.read_text(), original)

    def test_jsonc_configs_refuse_with_manual_guidance(self):
        gemini = self.project / ".gemini/settings.json"
        gemini.parent.mkdir(parents=True, exist_ok=True)
        gemini.write_text('{"mcpServers": {}} // retained comment\n')
        refused = self.run_cli("skills", "connect", "--target", "gemini-cli", "--apply")
        self.assertNotEqual(refused.returncode, 0)
        self.assertIn("JSONC comments", refused.stderr)
        self.assertIn("retained comment", gemini.read_text())
        opencode = self.project / "opencode.jsonc"
        opencode.write_text('{"mcp": {}} // retained comment\n')
        refused = self.run_cli("skills", "connect", "--target", "opencode", "--apply")
        self.assertNotEqual(refused.returncode, 0)
        self.assertIn("opencode.jsonc", refused.stderr)
        self.assertFalse((self.project / "opencode.json").exists())

    @unittest.skipUnless(os.name == "posix", "directory symlink setup requires POSIX")
    def test_symlinked_client_directory_is_refused(self):
        elsewhere = self.base / "elsewhere"
        elsewhere.mkdir()
        (self.project / ".cursor").symlink_to(elsewhere, target_is_directory=True)
        refused = self.run_cli("skills", "connect", "--target", "cursor", "--apply")
        self.assertNotEqual(refused.returncode, 0)
        self.assertIn("directory is a symlink", refused.stderr)
        self.assertFalse((elsewhere / "mcp.json").exists())

    def test_git_unavailable_refuses_project_config_write(self):
        subprocess.run(["git", "init", "-q", str(self.project)], check=True)
        empty_path = self.base / "empty-path"
        empty_path.mkdir()
        self.env["PATH"] = str(empty_path)
        refused = self.run_cli("skills", "connect", "--target", "cursor", "--apply",
                               "--python", sys.executable)
        self.assertNotEqual(refused.returncode, 0)
        self.assertIn("Cannot verify whether the target config is tracked", refused.stderr)
        self.assertFalse((self.project / ".cursor/mcp.json").exists())

    def test_unignored_git_project_requires_explicit_opt_in(self):
        subprocess.run(["git", "init", "-q", str(self.project)], check=True)
        preview = self.run_cli("skills", "connect", "--target", "claude-code", "--json")
        self.assertEqual(preview.returncode, 0, preview.stdout + preview.stderr)
        self.assertEqual(json.loads(preview.stdout)["git_visibility"], "unignored")
        refused = self.run_cli("skills", "connect", "--target", "claude-code", "--apply")
        self.assertNotEqual(refused.returncode, 0)
        self.assertIn("--allow-unignored", refused.stderr)
        self.assertFalse((self.project / ".mcp.json").exists())
        allowed = self.run_cli("skills", "connect", "--target", "claude-code",
                               "--apply", "--allow-unignored")
        self.assertEqual(allowed.returncode, 0, allowed.stdout + allowed.stderr)
        self.assertIn("metactl-skills", (self.project / ".mcp.json").read_text())

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

    def test_doctor_reads_recent_events_after_large_or_partial_log(self):
        applied = self.run_cli("skills", "connect", "--target", "codex-cli", "--apply", "--json")
        self.assertEqual(applied.returncode, 0, applied.stdout + applied.stderr)
        ledger = pathlib.Path(json.loads(applied.stdout)["event_log"])
        event = {"schema": "metactl.discovery_trial.v1", "runtime": "codex-cli",
                 "kind": "discover", "arm": "baseline", "provider_calls": 0,
                 "session_id": "recent-session", "run_id": "recent-run"}
        ledger.write_text("x" * (2 * 1024 * 1024 + 100) + "\n" +
                          json.dumps({"schema": "other.v1"}) + "\n" +
                          json.dumps(event) + "\npartial\n")
        doctor = self.run_cli("skills", "doctor", "--target", "codex-cli", "--json")
        self.assertEqual(doctor.returncode, 0, doctor.stdout + doctor.stderr)
        state = json.loads(doctor.stdout)
        self.assertEqual(state["log_status"], "partial")
        self.assertEqual(state["routing"], "observed")
        self.assertEqual(state["latest_discovery"]["run_id"], "recent-run")
        self.assertTrue(state["log_window_truncated"])
        self.assertEqual(state["invalid_log_lines"], 1)

    def test_preview_does_not_create_client_or_state_files(self):
        preview = self.run_cli("skills", "connect", "--target", "codex-cli")
        self.assertEqual(preview.returncode, 0, preview.stdout + preview.stderr)
        self.assertFalse((self.project / ".codex").exists())
        self.assertFalse((self.base / "state").exists())


if __name__ == "__main__":
    if not BINARY.exists():
        raise SystemExit(f"build metactl first: {BINARY}")
    unittest.main()
