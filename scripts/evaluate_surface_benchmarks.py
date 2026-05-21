#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import math
import os
import subprocess
import tempfile
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
STARTER_ROOT = ROOT / "library" / "starter"
DEFAULT_FIXTURE = ROOT / "tests" / "fixtures" / "surface_benchmarks" / "starter_cases.json"
MODES = ("minimal", "auto", "full")
METRIC_K = 3


def load_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def write_project_config(project_root: Path, fixture: dict[str, Any], mode: str) -> None:
    project = fixture["project"]
    pack_lines = "\n".join(f"- {pack}" for pack in project["packs"])
    config = (
        "api_version: metactl/v2alpha1\n"
        f"role: {project['role']}\n"
        f"policy: {project['policy']}\n"
        "packs:\n"
        f"{pack_lines}\n"
        "targets:\n"
        f"- {project['target']}\n"
        "starter_library:\n"
        f"- {STARTER_ROOT}\n"
        "defaults:\n"
        "  brownfield_mode: review_diff\n"
        "  discovery_mode: curated_only\n"
        f"  surface_selection_mode: {mode}\n"
    )
    (project_root / "metactl.yaml").write_text(config, encoding="utf-8")


def clean_env(project_root: Path) -> dict[str, str]:
    env = os.environ.copy()
    env.pop("METACTL_PROFILE", None)
    env.pop("XDG_CONFIG_HOME", None)
    env["HOME"] = str(project_root / ".test-home")
    return env


def run_metactl(bin_path: Path, project_root: Path, args: list[str]) -> str:
    result = subprocess.run(
        [
            str(bin_path),
            "--project",
            str(project_root),
            "--json",
            "--no-input",
            *args,
        ],
        cwd=str(ROOT),
        env=clean_env(project_root),
        check=True,
        capture_output=True,
        text=True,
    )
    return result.stdout


def metactl_version(bin_path: Path) -> str:
    result = subprocess.run(
        [str(bin_path), "version"],
        cwd=str(ROOT),
        check=True,
        capture_output=True,
        text=True,
    )
    return result.stdout.strip()


def output_size(project_root: Path, output: dict[str, Any]) -> int:
    path = project_root / output["path"]
    return path.stat().st_size


def compile_mode(bin_path: Path, fixture: dict[str, Any], mode: str) -> dict[str, Any]:
    with tempfile.TemporaryDirectory(prefix=f"metactl-surface-{mode}-") as tmp:
        project_root = Path(tmp)
        (project_root / ".test-home").mkdir(parents=True, exist_ok=True)
        write_project_config(project_root, fixture, mode)
        run_metactl(bin_path, project_root, ["compile", "--update-lock"])
        manifest_path = (
            project_root
            / ".metactl"
            / "generated"
            / fixture["project"]["target"]
            / "compile.manifest.json"
        )
        manifest = load_json(manifest_path)
        outputs = manifest["generated_outputs"]

        generated_surface_bytes = sum(output_size(project_root, output) for output in outputs)
        root_instruction_bytes = sum(
            output_size(project_root, output)
            for output in outputs
            if output.get("kind") == "instruction_file"
        )
        native_skill_bytes = sum(
            output_size(project_root, output)
            for output in outputs
            if output.get("kind") == "skill_folder"
        )
        slash_command_bytes = sum(
            output_size(project_root, output)
            for output in outputs
            if is_slash_command_output(output)
        )
        paths = sorted(output.get("destination_path") or output["path"] for output in outputs)
        surface_selection = manifest.get("surface_selection", [])
        return {
            "mode": mode,
            "generated_output_count": len(outputs),
            "generated_surface_bytes": generated_surface_bytes,
            "estimated_context_tokens": math.ceil(generated_surface_bytes / 4),
            "root_instruction_bytes": root_instruction_bytes,
            "native_skill_count": sum(1 for output in outputs if output.get("kind") == "skill_folder"),
            "native_skill_bytes": native_skill_bytes,
            "slash_command_count": sum(1 for output in outputs if is_slash_command_output(output)),
            "slash_command_bytes": slash_command_bytes,
            "mcp_searchable_count": len(fixture["project"]["packs"]),
            "suppressed_surface_count": sum(
                1 for item in surface_selection if item.get("emitted") is False
            ),
            "emitted_surface_count": sum(
                1 for item in surface_selection if item.get("emitted") is True
            ),
            "paths": paths,
        }


def is_slash_command_output(output: dict[str, Any]) -> bool:
    path = output.get("destination_path") or output.get("path") or ""
    return output.get("kind") == "resource_file" and path.startswith(".codex/commands/")


def search_project(bin_path: Path, fixture: dict[str, Any], query: str) -> list[dict[str, Any]]:
    with tempfile.TemporaryDirectory(prefix="metactl-surface-search-") as tmp:
        project_root = Path(tmp)
        (project_root / ".test-home").mkdir(parents=True, exist_ok=True)
        write_project_config(project_root, fixture, "auto")
        raw = run_metactl(bin_path, project_root, ["search", query])
    payload = json.loads(raw)
    return [
        {"pack_id": item["pack_ref"]["id"], "score": item["score"]}
        for item in payload["matches"]
    ]


def evaluate_task_cases(
    bin_path: Path, fixture: dict[str, Any], auto_paths: set[str]
) -> list[dict[str, Any]]:
    results = []
    for case in fixture["task_cases"]:
        matches = search_project(bin_path, fixture, case["query"])
        ranked = [item["pack_id"] for item in matches]
        expected_pack = case["expected_pack"]
        rank = ranked.index(expected_pack) + 1 if expected_pack in ranked else None
        recall_at_1 = rank == 1
        recall_at_3 = rank is not None and rank <= METRIC_K
        mrr = round(1 / rank, 4) if rank else 0.0
        expected_command = case.get("expected_command")
        expected_command_available = (
            True if not expected_command else expected_command in auto_paths
        )
        body_read_route_available = any(
            path.startswith(f".codex/skills/{expected_pack}/") for path in auto_paths
        )
        false_negative = (
            not recall_at_3 or not expected_command_available or not body_read_route_available
        )
        result = {
            "id": case["id"],
            "query": case["query"],
            "expected_pack": expected_pack,
            "rank": rank,
            "recall_at_1": recall_at_1,
            "recall_at_3": recall_at_3,
            "mrr": mrr,
            "expected_command_available": expected_command_available,
            "body_read_route_available": body_read_route_available,
            "false_negative": false_negative,
            "top_matches": matches[:METRIC_K],
        }
        if expected_command:
            result["expected_command"] = expected_command
        results.append(result)
    return results


def ratio(numerator: int, denominator: int) -> float:
    if denominator <= 0:
        return 0.0
    return round(numerator / denominator, 4)


def summarize(
    modes: dict[str, dict[str, Any]], task_cases: list[dict[str, Any]]
) -> dict[str, Any]:
    full = modes["full"]
    auto = modes["auto"]
    command_cases = [case for case in task_cases if "expected_command" in case]
    return {
        "auto_generated_surface_reduction": ratio(
            full["generated_surface_bytes"] - auto["generated_surface_bytes"],
            full["generated_surface_bytes"],
        ),
        "auto_skill_body_reduction": ratio(
            full["native_skill_bytes"] - auto["native_skill_bytes"],
            full["native_skill_bytes"],
        ),
        "expected_pack_recall_at_1": ratio(
            sum(1 for case in task_cases if case["recall_at_1"]), len(task_cases)
        ),
        "expected_pack_recall_at_3": ratio(
            sum(1 for case in task_cases if case["recall_at_3"]), len(task_cases)
        ),
        "mrr": round(sum(case["mrr"] for case in task_cases) / len(task_cases), 4),
        "expected_command_availability": ratio(
            sum(1 for case in command_cases if case["expected_command_available"]),
            len(command_cases),
        )
        if command_cases
        else 1.0,
        "body_read_route_availability": ratio(
            sum(1 for case in task_cases if case["body_read_route_available"]),
            len(task_cases),
        ),
        "false_negative_count": sum(1 for case in task_cases if case["false_negative"]),
    }


def verdict(metrics: dict[str, Any], thresholds: dict[str, Any]) -> dict[str, Any]:
    reasons = []
    if metrics["auto_generated_surface_reduction"] < thresholds["auto_generated_surface_reduction_min"]:
        reasons.append("auto generated surface reduction below threshold")
    if metrics["auto_skill_body_reduction"] < thresholds["auto_skill_body_reduction_min"]:
        reasons.append("auto skill body reduction below threshold")
    if metrics["expected_pack_recall_at_3"] < thresholds["expected_pack_recall_at_3_min"]:
        reasons.append("expected pack recall@3 below threshold")
    if metrics["expected_command_availability"] < thresholds["expected_command_availability_min"]:
        reasons.append("expected slash command availability below threshold")
    if metrics["false_negative_count"] > thresholds["false_negative_count_max"]:
        reasons.append("false negative count above threshold")
    return {"status": "fail" if reasons else "pass", "reasons": reasons}


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--metactl-bin", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--fixture", default=str(DEFAULT_FIXTURE))
    args = parser.parse_args()

    bin_path = Path(args.metactl_bin).resolve()
    output = Path(args.output).resolve()
    fixture_path = Path(args.fixture).resolve()
    fixture = load_json(fixture_path)
    output.parent.mkdir(parents=True, exist_ok=True)

    modes = {mode: compile_mode(bin_path, fixture, mode) for mode in MODES}
    task_cases = evaluate_task_cases(bin_path, fixture, set(modes["auto"]["paths"]))
    metrics = summarize(modes, task_cases)
    thresholds = fixture["thresholds"]
    result_verdict = verdict(metrics, thresholds)

    artifact = {
        "api_version": "metactl/v2alpha1",
        "generated_at": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "evaluation_kind": "surface_budget_and_route_retention",
        "benchmark_scope": "local_deterministic_projection",
        "notes": (
            "Local deterministic projection benchmark. It measures generated "
            "surface budget and route retention; it is not provider-backed trace proof."
        ),
        "library_root": str(STARTER_ROOT),
        "metactl_version": metactl_version(bin_path),
        "repo": str(ROOT),
        "profile": "fixture:starter_cases",
        "target": fixture["project"]["target"],
        "project": fixture["project"],
        "modes": modes,
        "task_cases": task_cases,
        "metrics": metrics,
        "thresholds": thresholds,
        "verdict": result_verdict,
    }

    output.write_text(json.dumps(artifact, indent=2) + "\n", encoding="utf-8")
    print(
        "surface benchmark: "
        f"{result_verdict['status']} "
        f"auto_reduction={metrics['auto_generated_surface_reduction']} "
        f"recall@3={metrics['expected_pack_recall_at_3']} "
        f"false_negatives={metrics['false_negative_count']} "
        f"output={output}"
    )
    if result_verdict["status"] != "pass":
        raise SystemExit("surface benchmark failed: " + "; ".join(result_verdict["reasons"]))


if __name__ == "__main__":
    main()
