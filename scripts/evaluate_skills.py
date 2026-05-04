#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import subprocess
import tempfile
from datetime import datetime, timezone
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
STARTER_ROOT = ROOT / "library" / "starter"
TARGET = "codex-cli"

CASES = [
    {
        "id": "python-refactor-projection",
        "title": "Python refactor skill projects a Codex skill folder",
        "role": "builder",
        "policy": "brownfield-safe-builder",
        "target": TARGET,
        "baseline_packs": ["migration-guard"],
        "skill_enabled_packs": ["migration-guard", "python-refactor"],
        "expected_skill_pack": "python-refactor",
    },
    {
        "id": "unit-test-loop-projection",
        "title": "Unit test loop projects a Codex skill folder",
        "role": "release-manager",
        "policy": "release-policy",
        "target": TARGET,
        "baseline_packs": ["release-guard"],
        "skill_enabled_packs": ["release-guard", "unit-test-loop"],
        "expected_skill_pack": "unit-test-loop",
    },
]


def load_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def write_project_config(project_root: Path, case: dict, packs: list[str]) -> None:
    (project_root / ".metactl" / "private").mkdir(parents=True, exist_ok=True)
    (project_root / ".metactl" / "generated").mkdir(parents=True, exist_ok=True)
    pack_lines = "\n".join(f"- {pack}" for pack in packs)
    config = (
        "api_version: metactl/v2alpha1\n"
        f"role: {case['role']}\n"
        f"policy: {case['policy']}\n"
        "packs:\n"
        f"{pack_lines}\n"
        "targets:\n"
        f"- {case['target']}\n"
        "starter_library:\n"
        f"- {STARTER_ROOT}\n"
        "defaults:\n"
        "  brownfield_mode: review_diff\n"
        "  discovery_mode: curated_only\n"
    )
    (project_root / "metactl.yaml").write_text(config, encoding="utf-8")


def compile_projection(bin_path: Path, case: dict, packs: list[str]) -> dict:
    with tempfile.TemporaryDirectory(prefix="metactl-skill-eval-") as tmp:
        project_root = Path(tmp)
        test_home = project_root / ".test-home"
        test_home.mkdir(parents=True, exist_ok=True)
        write_project_config(project_root, case, packs)

        env = os.environ.copy()
        env.pop("METACTL_PROFILE", None)
        env.pop("XDG_CONFIG_HOME", None)
        env["HOME"] = str(test_home)
        subprocess.run(
            [
                str(bin_path),
                "--project",
                str(project_root),
                "--json",
                "--no-input",
                "compile",
                "--update-lock",
            ],
            cwd=str(ROOT),
            env=env,
            check=True,
            capture_output=True,
            text=True,
        )
        manifest_path = (
            project_root
            / ".metactl"
            / "generated"
            / case["target"]
            / "compile.manifest.json"
        )
        manifest = load_json(manifest_path)

    outputs = manifest["generated_outputs"]
    output_paths = sorted(
        output.get("destination_path") or output["path"] for output in outputs
    )
    skill_pack_ids = sorted(
        {
            output["pack_ref"]["id"]
            for output in outputs
            if output.get("kind") == "skill_folder" and output.get("pack_ref")
        }
    )
    expected_pack = case["expected_skill_pack"]
    return {
        "packs": packs,
        "generated_output_count": len(outputs),
        "generated_output_paths": output_paths,
        "skill_pack_ids": skill_pack_ids,
        "contains_expected_skill": expected_pack in skill_pack_ids,
    }


def evaluate_case(bin_path: Path, case: dict) -> dict:
    baseline = compile_projection(bin_path, case, case["baseline_packs"])
    skill_enabled = compile_projection(bin_path, case, case["skill_enabled_packs"])
    new_paths = sorted(
        set(skill_enabled["generated_output_paths"])
        - set(baseline["generated_output_paths"])
    )
    expected_added = (
        skill_enabled["contains_expected_skill"]
        and not baseline["contains_expected_skill"]
    )
    return {
        "id": case["id"],
        "title": case["title"],
        "role": case["role"],
        "policy": case["policy"],
        "target": case["target"],
        "expected_skill_pack": case["expected_skill_pack"],
        "expected_output_kind": "skill_folder",
        "baseline": baseline,
        "skill_enabled": skill_enabled,
        "delta": {
            "expected_skill_added": expected_added,
            "new_output_paths": new_paths,
        },
        "passed": expected_added,
    }


def summarize(cases: list[dict]) -> dict:
    failed_case_ids = [case["id"] for case in cases if not case["passed"]]
    return {
        "case_count": len(cases),
        "pass_count": len(cases) - len(failed_case_ids),
        "failed_case_ids": failed_case_ids,
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--metactl-bin", required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()

    bin_path = Path(args.metactl_bin).resolve()
    output = Path(args.output).resolve()
    output.parent.mkdir(parents=True, exist_ok=True)

    cases = [evaluate_case(bin_path, case) for case in CASES]
    artifact = {
        "api_version": "metactl/v2alpha1",
        "generated_at": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "library_root": str(STARTER_ROOT),
        "evaluation_kind": "paired_projection_effectiveness",
        "benchmark_scope": "local_deterministic_projection",
        "notes": "Paired local compile projections measure whether the expected skill surface is emitted. They do not measure live model task quality.",
        "summary": summarize(cases),
        "cases": cases,
    }

    output.write_text(json.dumps(artifact, indent=2) + "\n", encoding="utf-8")
    if artifact["summary"]["failed_case_ids"]:
        raise SystemExit(
            "skill eval failed for: " + ", ".join(artifact["summary"]["failed_case_ids"])
        )


if __name__ == "__main__":
    main()
