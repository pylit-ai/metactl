#!/usr/bin/env python3
"""Offline development benchmark; no model calls and no inferred token prices."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import platform
import statistics
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("fixtures", ROOT / "tests/test_skill_discovery_host.py")
fixtures = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixtures)


def percentile(values, fraction):
    values = sorted(values)
    return values[min(len(values)-1, int((len(values)-1)*fraction))]


def run(repeats=3, distractors=200):
    raw = (ROOT / "fixtures/skill-discovery-cases.json").read_bytes()
    cases = json.loads(raw)
    fixture = fixtures.Fixture()
    try:
        for skill in cases["skills"]:
            fixture.add(skill["name"], skill["description"])
        for i in range(distractors):
            fixture.add(f"specialist-{i:04}", f"Handle synthetic topic zzz{i:04} only. " * 3)
        catalog = fixture.run(["catalog"])
        # Native-style plain metadata view, not a captured host/system prompt.
        metadata = [{k: s[k] for k in ("name", "description")} for s in catalog["skills"]]
        native_bytes = len(fixtures.host.compact(metadata).encode())
        front_door_bytes = len(fixtures.host.BOOTSTRAP.encode()) + len(fixtures.host.compact(fixtures.host.tools()).encode())
        host = fixtures.host.Host(str(fixtures.BINARY), str(fixture.project), runner=fixture.run)
        rows = []
        for repetition in range(repeats):
            for case in cases["cases"]:
                # Alternate arm order to reduce startup/order bias; every CLI
                # call is a cold process, while OS filesystem cache may be warm.
                arms = ["existing_route", "deterministic_host"]
                if repetition % 2:
                    arms.reverse()
                for arm in arms:
                    start = time.perf_counter()
                    if arm == "existing_route":
                        result = fixture.run(["route", "--limit", "5", "--", case["query"]])
                        names = [s["skill_name"] for s in result["candidates"]]
                    else:
                        result = host.call("discover_skills", {"query": case["query"]})
                        names = [s["name"] for s in result["result"]["skills"]]
                    elapsed = (time.perf_counter() - start) * 1000
                    expected = set(case["relevant"])
                    hits = len(expected.intersection(names))
                    ranks = [i+1 for i,n in enumerate(names) if n in expected]
                    rows.append({"case": case["id"], "repetition": repetition, "arm": arm,
                                 "returned": names, "relevant": case["relevant"], "elapsed_ms": elapsed,
                                 "recall_at_5": hits / len(expected) if expected else None,
                                 "precision_at_5": hits / 5 if expected else None,
                                 "reciprocal_rank": 1/min(ranks) if ranks else 0 if expected else None,
                                 "no_match_correct": not names if not expected else None,
                                 "result_bytes": len(fixtures.host.compact(result).encode())})
        summaries = {}
        for arm in ("existing_route", "deterministic_host"):
            data = [r for r in rows if r["arm"] == arm]
            summaries[arm] = {"runs":len(data), "latency_p50_ms":statistics.median(r["elapsed_ms"] for r in data),
                              "latency_p95_ms":percentile([r["elapsed_ms"] for r in data], .95),
                              "recall_at_5":statistics.mean(r["recall_at_5"] for r in data if r["recall_at_5"] is not None),
                              "precision_at_5":statistics.mean(r["precision_at_5"] for r in data if r["precision_at_5"] is not None),
                              "mrr":statistics.mean(r["reciprocal_rank"] for r in data if r["reciprocal_rank"] is not None),
                              "no_match_accuracy":statistics.mean(r["no_match_correct"] for r in data if r["no_match_correct"] is not None)}
        return {"schema":"metactl.discovery_benchmark.v1", "fixture_sha256":hashlib.sha256(raw).hexdigest(),
                "platform":platform.platform(), "python":platform.python_version(),
                "binary_sha256":hashlib.sha256(fixtures.BINARY.read_bytes()).hexdigest(),
                "repeats":repeats, "unique_cases":len(cases["cases"]), "catalog_size":len(metadata),
                "native_style_metadata_bytes":native_bytes, "discovery_front_door_bytes":front_door_bytes,
                "metadata_byte_reduction_fraction":1-front_door_bytes/native_bytes,
                "summary":summaries, "rows":rows,
                "claims":{"host_prompt_tokens":None,"end_to_end_task_success":None,"dollar_savings":None,
                          "live_jev": "not run", "native_catalog_suppression": "not tested",
                          "limitations":["Authored development set, not held-out", "Synthetic distractors",
                                          "Cold CLI process; filesystem cache uncontrolled", "No coding-model requests",
                                          "Metadata bytes are not actual host prompt tokens"]}}
    finally:
        fixture.close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--distractors", type=int, default=200)
    args = parser.parse_args()
    if not 1 <= args.repeats <= 20 or not 0 <= args.distractors <= 1000:
        parser.error("benchmark bounds exceeded")
    print(json.dumps(run(args.repeats, args.distractors), indent=2))
