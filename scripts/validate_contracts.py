#!/usr/bin/env python3
from __future__ import annotations

import json
import hashlib
from pathlib import Path
from typing import Any

try:
    from jsonschema import FormatChecker
    from jsonschema.validators import validator_for
    from referencing import Registry
    from referencing.jsonschema import DRAFT201909
except ModuleNotFoundError as exc:
    if exc.name in {"jsonschema", "referencing"}:
        raise SystemExit(
            "Missing Python dependency for scripts/validate_contracts.py. "
            "Install the dev requirements first: "
            "python3 -m pip install -r requirements-dev.txt"
        ) from exc
    raise

ROOT = Path(__file__).resolve().parents[1]
SCHEMA_ROOT = ROOT / "contracts" / "schemas" / "metactl"
FIXTURE_ROOT = ROOT / "fixtures" / "golden"
SKILL_AUDIT_FIXTURE_ROOT = ROOT / "fixtures" / "skills" / "audit"
HOST_FIXTURE_ROOT = ROOT / "fixtures" / "hosts"
REPO_JSONRPC_ROOT = ROOT / "contracts" / "jsonrpc" / "v1"


def load_json(path: Path) -> Any:
    with path.open("r", encoding="utf-8") as f:
        return json.load(f)


def schema_registry() -> Registry:
    registry = Registry()
    for path in SCHEMA_ROOT.rglob("*.json"):
        schema = load_json(path)
        resource = DRAFT201909.create_resource(schema)
        registry = registry.with_resource(path.resolve().as_uri(), resource)
        schema_id = schema.get("$id")
        if schema_id:
            registry = registry.with_resource(schema_id, resource)
    return registry


def validate_instance(instance: Any, schema_path: Path, registry: Registry) -> None:
    schema = load_json(schema_path)
    cls = validator_for(schema)
    cls.check_schema(schema)
    validator = cls(schema, registry=registry, format_checker=FormatChecker())
    errors = sorted(validator.iter_errors(instance), key=lambda e: list(e.absolute_path))
    if errors:
        lines = [f"Schema validation failed for {schema_path.relative_to(ROOT)}:"]
        for err in errors:
            path = "/".join(str(p) for p in err.absolute_path) or "<root>"
            lines.append(f"  - {path}: {err.message}")
        raise SystemExit("\n".join(lines))


def sha256_digest(path: Path) -> str:
    return "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()


def validate_compile_outputs(fixture_dir: Path) -> None:
    manifest = load_json(fixture_dir / "compile.manifest.json")
    for output in manifest["generated_outputs"]:
        rel = output["path"]
        path = ROOT / rel
        if not path.exists():
            raise SystemExit(f"Missing generated output referenced by compile manifest: {rel}")
        actual = sha256_digest(path)
        expected = output.get("digest")
        if expected and expected != actual:
            raise SystemExit(
                f"Digest mismatch for {rel}: expected {expected}, got {actual}"
            )


def validate_jsonrpc_pairs(fixture_dir: Path) -> None:
    jsonrpc_dir = fixture_dir / "jsonrpc"
    pairs = ["search", "resolve", "explain", "compile", "validate"]
    for name in pairs:
        req = load_json(jsonrpc_dir / f"{name}.request.json")
        resp = load_json(jsonrpc_dir / f"{name}.response.json")
        if req["id"] != resp["id"]:
            raise SystemExit(f"JSON-RPC id mismatch in {fixture_dir.name} for {name}")


def validate_skill_audit_fixtures(registry: Registry) -> None:
    for fixture_path, schema_path in SKILL_AUDIT_SCHEMA_MAP.items():
        validate_instance(load_json(fixture_path), schema_path, registry)
        print(f"validated: {fixture_path.relative_to(ROOT)}")

    for fixture_path, kind in SKILL_AUDIT_NEGATIVE_FILES.items():
        data = load_json(fixture_path)
        if kind in {"prompt_leak", "secret_leak"}:
            try:
                validate_instance(data, SCHEMA_ROOT / "skill_portfolio_audit.schema.json", registry)
            except SystemExit:
                continue
            raise SystemExit(f"Expected validation failure for {fixture_path.relative_to(ROOT)}")
        if kind == "unredacted_home_path":
            paths = []
            for item in data.get("inventory", []):
                if isinstance(item, dict) and item.get("path"):
                    paths.append(item["path"])
            if not any(str(path).startswith("/Users/") for path in paths):
                raise SystemExit(
                    f"Expected a home path leak in {fixture_path.relative_to(ROOT)}"
                )
        if kind == "unsupported_mutation_plan":
            if data.get("mutation_allowed") is not True:
                raise SystemExit(
                    f"Expected mutation_allowed=true in {fixture_path.relative_to(ROOT)}"
                )
            if any(action.get("action") == "apply" for action in data.get("actions", [])):
                continue
            raise SystemExit(
                f"Expected unsupported apply action in {fixture_path.relative_to(ROOT)}"
            )


def validate_host_fixtures() -> None:
    for host_dir, paths in HOST_FIXTURE_MAP.items():
        source = load_json(paths["source"])
        if source.get("host") != host_dir.name:
            raise SystemExit(f"Host fixture mismatch in {paths['source'].relative_to(ROOT)}")
        required = {"host", "source_url", "source_checked_at", "verified_by_test", "confidence"}
        if set(source) != required:
            raise SystemExit(f"Unexpected source.json keys in {paths['source'].relative_to(ROOT)}")
        expected = load_json(paths["expected_visibility"])
        required_visibility = {
            "target_id",
            "scope",
            "discovery_confidence",
            "visibility_confidence",
            "duplicate_name_behavior",
        }
        if set(expected) != required_visibility:
            raise SystemExit(
                f"Unexpected expected_visibility.json keys in {paths['expected_visibility'].relative_to(ROOT)}"
            )


FIXTURE_SCHEMA_MAP = {
    "role.manifest.json": SCHEMA_ROOT / "role_manifest.schema.json",
    "policy.manifest.json": SCHEMA_ROOT / "policy_manifest.schema.json",
    "target.capability.json": SCHEMA_ROOT / "target_capability_matrix.schema.json",
    "config.json": SCHEMA_ROOT / "config.schema.json",
    "overlay.json": SCHEMA_ROOT / "invocation_overlay.schema.json",
    "search.result.json": SCHEMA_ROOT / "search_result.schema.json",
    "resolve.graph.json": SCHEMA_ROOT / "resolve_graph.schema.json",
    "explain.result.json": SCHEMA_ROOT / "explain_result.schema.json",
    "compile.manifest.json": SCHEMA_ROOT / "compile_manifest.schema.json",
    "policy.enforcement.report.json": SCHEMA_ROOT / "policy_enforcement_report.schema.json",
    "validation.report.json": SCHEMA_ROOT / "validation_report.schema.json",
}

SKILL_AUDIT_SCHEMA_MAP = {
    SKILL_AUDIT_FIXTURE_ROOT / "positive" / "inventory.sample.json": SCHEMA_ROOT / "skill_inventory.schema.json",
    SKILL_AUDIT_FIXTURE_ROOT / "positive" / "relations.sample.json": SCHEMA_ROOT / "skill_relation.schema.json",
    SKILL_AUDIT_FIXTURE_ROOT / "positive" / "report.sample.json": SCHEMA_ROOT / "skill_portfolio_audit.schema.json",
    SKILL_AUDIT_FIXTURE_ROOT / "positive" / "action_plan.sample.json": SCHEMA_ROOT / "skill_action_plan.schema.json",
}

SKILL_AUDIT_NEGATIVE_FILES = {
    SKILL_AUDIT_FIXTURE_ROOT / "negative" / "prompt_leak.json": "prompt_leak",
    SKILL_AUDIT_FIXTURE_ROOT / "negative" / "secret_leak.json": "secret_leak",
    SKILL_AUDIT_FIXTURE_ROOT / "negative" / "unredacted_home_path.json": "unredacted_home_path",
    SKILL_AUDIT_FIXTURE_ROOT / "negative" / "unsupported_mutation_plan.json": "unsupported_mutation_plan",
}

HOST_FIXTURE_MAP = {
    HOST_FIXTURE_ROOT / "codex-cli": {
        "source": HOST_FIXTURE_ROOT / "codex-cli" / "source.json",
        "expected_visibility": HOST_FIXTURE_ROOT / "codex-cli" / "expected_visibility.json",
    },
    HOST_FIXTURE_ROOT / "metactl-generated": {
        "source": HOST_FIXTURE_ROOT / "metactl-generated" / "source.json",
        "expected_visibility": HOST_FIXTURE_ROOT / "metactl-generated" / "expected_visibility.json",
    },
}

JSONRPC_SCHEMA_MAP = {
    "search.request.json": SCHEMA_ROOT / "jsonrpc" / "search.request.schema.json",
    "search.response.json": SCHEMA_ROOT / "jsonrpc" / "search.response.schema.json",
    "resolve.request.json": SCHEMA_ROOT / "jsonrpc" / "resolve.request.schema.json",
    "resolve.response.json": SCHEMA_ROOT / "jsonrpc" / "resolve.response.schema.json",
    "explain.request.json": SCHEMA_ROOT / "jsonrpc" / "explain.request.schema.json",
    "explain.response.json": SCHEMA_ROOT / "jsonrpc" / "explain.response.schema.json",
    "compile.request.json": SCHEMA_ROOT / "jsonrpc" / "compile.request.schema.json",
    "compile.response.json": SCHEMA_ROOT / "jsonrpc" / "compile.response.schema.json",
    "validate.request.json": SCHEMA_ROOT / "jsonrpc" / "validate.request.schema.json",
    "validate.response.json": SCHEMA_ROOT / "jsonrpc" / "validate.response.schema.json",
}

REPO_JSONRPC_COPY_MAP = {
    "metactl.common_jsonrpc.schema.json": SCHEMA_ROOT / "jsonrpc" / "common_jsonrpc.schema.json",
    "metactl.search.request.schema.json": SCHEMA_ROOT / "jsonrpc" / "search.request.schema.json",
    "metactl.search.response.schema.json": SCHEMA_ROOT / "jsonrpc" / "search.response.schema.json",
    "metactl.resolve.request.schema.json": SCHEMA_ROOT / "jsonrpc" / "resolve.request.schema.json",
    "metactl.resolve.response.schema.json": SCHEMA_ROOT / "jsonrpc" / "resolve.response.schema.json",
    "metactl.explain.request.schema.json": SCHEMA_ROOT / "jsonrpc" / "explain.request.schema.json",
    "metactl.explain.response.schema.json": SCHEMA_ROOT / "jsonrpc" / "explain.response.schema.json",
    "metactl.compile.request.schema.json": SCHEMA_ROOT / "jsonrpc" / "compile.request.schema.json",
    "metactl.compile.response.schema.json": SCHEMA_ROOT / "jsonrpc" / "compile.response.schema.json",
    "metactl.validate.request.schema.json": SCHEMA_ROOT / "jsonrpc" / "validate.request.schema.json",
    "metactl.validate.response.schema.json": SCHEMA_ROOT / "jsonrpc" / "validate.response.schema.json",
}

ACTIVATION_TRACE_SAMPLE = ROOT / "fixtures/library/evals/activation-trace.sample.json"

AUXILIARY_SCHEMA_MAP = {
    ACTIVATION_TRACE_SAMPLE: SCHEMA_ROOT / "activation_trace.schema.json",
}


def validate_repo_jsonrpc_copies() -> None:
    for filename, source_path in REPO_JSONRPC_COPY_MAP.items():
        copied_path = REPO_JSONRPC_ROOT / filename
        if load_json(copied_path) != load_json(source_path):
            raise SystemExit(
                f"Schema copy mismatch for {copied_path.relative_to(ROOT)}"
            )


def validate_auxiliary_artifacts(registry: Registry) -> None:
    for artifact_path, schema_path in AUXILIARY_SCHEMA_MAP.items():
        validate_instance(load_json(artifact_path), schema_path, registry)
        print(f"validated: {artifact_path.relative_to(ROOT)}")


def validate_fixture_dir(fixture_dir: Path, registry: Registry) -> None:
    # pack manifests
    for path in fixture_dir.glob("pack.*.json"):
        validate_instance(load_json(path), SCHEMA_ROOT / "pack_manifest.schema.json", registry)
    # provenance bundle items
    prov_bundle = load_json(fixture_dir / "provenance.bundle.json")
    for item in prov_bundle:
        validate_instance(item, SCHEMA_ROOT / "provenance_envelope.schema.json", registry)
    # other fixture artifacts
    for filename, schema_path in FIXTURE_SCHEMA_MAP.items():
        validate_instance(load_json(fixture_dir / filename), schema_path, registry)
    # jsonrpc examples
    for filename, schema_path in JSONRPC_SCHEMA_MAP.items():
        validate_instance(load_json(fixture_dir / "jsonrpc" / filename), schema_path, registry)
    validate_compile_outputs(fixture_dir)
    validate_jsonrpc_pairs(fixture_dir)


if __name__ == "__main__":
    registry = schema_registry()
    validate_repo_jsonrpc_copies()
    validate_auxiliary_artifacts(registry)
    validate_skill_audit_fixtures(registry)
    validate_host_fixtures()
    for fixture_dir in sorted(p for p in FIXTURE_ROOT.iterdir() if p.is_dir()):
        validate_fixture_dir(fixture_dir, registry)
        print(f"validated: {fixture_dir.relative_to(ROOT)}")
    print("all contracts and fixtures validated")
