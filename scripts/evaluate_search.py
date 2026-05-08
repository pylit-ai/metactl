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
METRIC_K = 3

CASES = [
    {
        "query": "python refactor",
        "role": "builder",
        "policy": "brownfield-safe-builder",
        "target": "codex-cli",
        "expected_shortlist": ["python-refactor"],
    },
    {
        "query": "migration safety",
        "role": "builder",
        "policy": "brownfield-safe-builder",
        "target": "codex-cli",
        "expected_shortlist": ["migration-guard"],
    },
    {
        "query": "tests verification",
        "role": "release-manager",
        "policy": "release-policy",
        "target": "codex-cli",
        "expected_shortlist": ["unit-test-loop", "release-guard", "release-manager"],
    },
]


def load_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def metadata_only_matches(query: str, role: str, target: str):
    terms = [term.lower() for term in query.split() if term.strip()]
    matches = []
    for path in sorted((STARTER_ROOT / "packs").glob("*.json")):
        manifest = load_json(path)
        haystack = " ".join(
            [
                manifest["id"],
                manifest["title"],
                manifest.get("description", ""),
                " ".join(manifest.get("task_tags", [])),
            ]
        ).lower()
        score = 0.0
        for term in terms:
            if term in haystack:
                score += 0.18
        roles = manifest.get("compatible_roles", [])
        if not roles or role in roles:
            score += 0.15
        targets = manifest.get("compatible_targets", [])
        if not targets or target in targets:
            score += 0.10
        if score > 0:
            matches.append({"pack_id": manifest["id"], "score": round(score, 2)})
    matches.sort(key=lambda item: (-item["score"], item["pack_id"]))
    return matches


def enhanced_matches(bin_path: Path, query: str, role: str, policy: str, target: str):
    with tempfile.TemporaryDirectory(prefix="metactl-search-eval-") as tmp:
        project_root = Path(tmp)
        test_home = project_root / ".test-home"
        test_home.mkdir(parents=True, exist_ok=True)
        config = (
            "api_version: metactl/v2alpha1\n"
            f"role: {role}\n"
            f"policy: {policy}\n"
            "targets:\n"
            f"- {target}\n"
            "starter_library:\n"
            f"- {STARTER_ROOT}\n"
        )
        (project_root / "metactl.yaml").write_text(config, encoding="utf-8")
        env = os.environ.copy()
        env.pop("METACTL_PROFILE", None)
        env.pop("XDG_CONFIG_HOME", None)
        env["HOME"] = str(test_home)
        raw = subprocess.check_output(
            [
                str(bin_path),
                "--project",
                str(project_root),
                "--json",
                "search",
                query,
            ],
            cwd=str(ROOT),
            env=env,
            text=True,
        )
    payload = json.loads(raw)
    return [
        {"pack_id": item["pack_ref"]["id"], "score": item["score"]}
        for item in payload["matches"]
    ]


def freshness_entries():
    entries = []
    for path in sorted((STARTER_ROOT / "packs").glob("*.json")):
        manifest = load_json(path)
        lifecycle = manifest.get("lifecycle")
        if lifecycle:
            entries.append({"pack_id": manifest["id"], "lifecycle": lifecycle})
    return entries


def pack_ids(matches):
    return [item["pack_id"] for item in matches]


def retrieval_metrics(matches, expected_shortlist, k=METRIC_K):
    ranked = pack_ids(matches)
    expected = list(expected_shortlist)
    expected_set = set(expected)
    top_k = ranked[:k]
    found = [pack_id for pack_id in expected if pack_id in top_k]
    missing = [pack_id for pack_id in expected if pack_id not in top_k]
    return {
        "k": k,
        "hit_at_1": bool(ranked[:1] and ranked[0] in expected_set),
        "recall_at_k": round(len(found) / len(expected_set), 4) if expected_set else 0.0,
        "expected_found": found,
        "expected_missing": missing,
    }


def summarize_cases(cases, k=METRIC_K):
    case_count = len(cases)
    if case_count == 0:
        raise SystemExit("search eval has no cases")

    def mean_metric(name, field):
        return round(sum(case[name][field] for case in cases) / case_count, 4)

    baseline_hit = mean_metric("baseline_metrics", "hit_at_1")
    enhanced_hit = mean_metric("enhanced_metrics", "hit_at_1")
    baseline_recall = mean_metric("baseline_metrics", "recall_at_k")
    enhanced_recall = mean_metric("enhanced_metrics", "recall_at_k")
    return {
        "case_count": case_count,
        "k": k,
        "baseline_hit_at_1": baseline_hit,
        "enhanced_hit_at_1": enhanced_hit,
        "baseline_recall_at_k": baseline_recall,
        "enhanced_recall_at_k": enhanced_recall,
        "enhanced_underperforms_baseline": (
            enhanced_hit < baseline_hit or enhanced_recall < baseline_recall
        ),
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--metactl-bin", required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()

    bin_path = Path(args.metactl_bin).resolve()
    output = Path(args.output).resolve()
    output.parent.mkdir(parents=True, exist_ok=True)

    artifact = {
        "api_version": "metactl/v2alpha1",
        "generated_at": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "library_root": str(STARTER_ROOT),
        "metric_k": METRIC_K,
        "cases": [],
        "freshness": freshness_entries(),
    }

    for case in CASES:
        baseline_matches = metadata_only_matches(
            case["query"], case["role"], case["target"]
        )
        current_matches = enhanced_matches(
            bin_path,
            case["query"],
            case["role"],
            case["policy"],
            case["target"],
        )
        artifact["cases"].append(
            {
                **case,
                "baseline_matches": baseline_matches,
                "baseline_metrics": retrieval_metrics(
                    baseline_matches, case["expected_shortlist"]
                ),
                "enhanced_matches": current_matches,
                "enhanced_metrics": retrieval_metrics(
                    current_matches, case["expected_shortlist"]
                ),
            }
        )

    artifact["summary"] = summarize_cases(artifact["cases"])

    output.write_text(json.dumps(artifact, indent=2) + "\n", encoding="utf-8")
    if artifact["summary"]["enhanced_underperforms_baseline"]:
        raise SystemExit("enhanced search metrics underperform metadata-only baseline")


if __name__ == "__main__":
    main()
