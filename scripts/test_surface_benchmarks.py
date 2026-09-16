"""Regression checks for benchmark evidence, independent of the Rust build."""
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import evaluate_surface_benchmarks as benchmark


class SurfaceBenchmarkEvidenceTests(unittest.TestCase):
    def route_result(self, paths):
        fixture = {"task_cases": [{"id": "demo-route", "query": "demo", "expected_pack": "demo"}]}
        with patch.object(benchmark, "search_project", return_value=[{"pack_id": "demo", "score": 1.0}]):
            return benchmark.evaluate_task_cases(Path("unused"), fixture, set(paths))[0]

    def test_canonical_skill_body_is_a_read_route(self):
        result = self.route_result([".agents/skills/demo/task/SKILL.md"])
        self.assertTrue(result["body_read_route_available"])
        self.assertFalse(result["false_negative"])

    def test_legacy_or_support_paths_do_not_mask_a_missing_canonical_body(self):
        for path in [
            ".codex/skills/demo/task/SKILL.md",
            ".agents/skills/demo/task/references/testing.md",
            ".agents/skills/demo-other/task/SKILL.md",
            ".agents/skills/demo/task/SKILL.md.backup",
        ]:
            with self.subTest(path=path):
                result = self.route_result([path])
                self.assertFalse(result["body_read_route_available"])
                self.assertTrue(result["false_negative"])

    def test_body_bytes_are_read_and_counted(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            body = root / "body.md"
            body.write_bytes("# Actual body λ\n".encode())
            output = {"path": "body.md", "kind": "skill_folder", "destination_path": ".agents/skills/demo/task/SKILL.md"}
            self.assertEqual(benchmark.output_size(root, output), len(body.read_bytes()))
            body.unlink()
            with self.assertRaises(FileNotFoundError):
                benchmark.output_size(root, output)

    def test_blank_bodies_do_not_count_as_available(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = {"path": "body.md", "kind": "skill_folder", "destination_path": ".agents/skills/demo/task/SKILL.md"}
            for contents in [b"", b" \n\t"]:
                with self.subTest(contents=contents):
                    (root / "body.md").write_bytes(contents)
                    with self.assertRaisesRegex(ValueError, "empty skill body"):
                        benchmark.output_size(root, output)

    def test_unreadable_output_cannot_pass_using_stat_metadata(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "body.md").write_text("present but unreadable")
            output = {"path": "body.md", "kind": "skill_folder"}
            with patch.object(Path, "read_bytes", side_effect=PermissionError("denied")):
                with self.assertRaises(PermissionError):
                    benchmark.output_size(root, output)

    def test_missing_body_still_fails_unchanged_zero_false_negative_threshold(self):
        fixture = benchmark.load_json(benchmark.DEFAULT_FIXTURE)
        metrics = {
            "auto_generated_surface_reduction": 1.0,
            "auto_skill_body_reduction": 1.0,
            "expected_pack_recall_at_3": 1.0,
            "expected_command_availability": 1.0,
            "false_negative_count": int(self.route_result([])["false_negative"]),
        }
        verdict = benchmark.verdict(metrics, fixture["thresholds"])
        self.assertEqual(verdict["status"], "fail")
        self.assertIn("false negative count above threshold", verdict["reasons"])


if __name__ == "__main__":
    unittest.main()
