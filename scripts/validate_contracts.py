#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
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
STARTER_ROOT = ROOT / "library" / "starter"
KNOWLEDGE_FIXTURE_ROOT = ROOT / "fixtures" / "knowledge_sources"
KNOWLEDGE_SOURCE_SCHEMA = SCHEMA_ROOT / "knowledge_source_manifest.schema.json"
LIBRARY_STACK_FIXTURE_ROOT = ROOT / "fixtures" / "library_stack"
LIBRARY_STACK_SCHEMA = SCHEMA_ROOT / "library_stack_manifest.schema.json"
LIBRARY_SOURCE_SCHEMA = SCHEMA_ROOT / "library_source_manifest.schema.json"
LIBRARY_PROFILE_SCHEMA = SCHEMA_ROOT / "library_profile_manifest.schema.json"
LIBRARY_STACK_LOCK_SCHEMA = SCHEMA_ROOT / "library_stack_lock.schema.json"
V1_FIXTURE_ROOT = ROOT / "fixtures" / "v1"


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


def display_path(path: Path) -> str:
    candidate = path if path.is_absolute() else ROOT / path
    try:
        return candidate.relative_to(ROOT).as_posix()
    except ValueError:
        return path.as_posix()


def validate_instance(
    instance: Any,
    schema_path: Path,
    registry: Registry,
    instance_path: Path | None = None,
) -> None:
    schema = load_json(schema_path)
    cls = validator_for(schema)
    cls.check_schema(schema)
    validator = cls(schema, registry=registry, format_checker=FormatChecker())
    errors = sorted(validator.iter_errors(instance), key=lambda e: list(e.absolute_path))
    if errors:
        subject = instance_path or schema_path
        lines = [
            "Schema validation failed for "
            f"{display_path(subject)} against {display_path(schema_path)}:"
        ]
        for err in errors:
            pointer = "/" + "/".join(str(p) for p in err.absolute_path)
            if pointer == "/":
                pointer = "/<root>"
            lines.append(f"  - {pointer}: {err.message}")
        lines.append(
            "Remediation: remove the field, use an x- extension key where the "
            "schema allows extensions, or update the schema and fixture together."
        )
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
    V1_FIXTURE_ROOT / "conformance.matrix.json": SCHEMA_ROOT / "conformance_matrix.schema.json",
    V1_FIXTURE_ROOT / "sanitized-export.sample.json": SCHEMA_ROOT / "sanitized_export.schema.json",
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
        validate_instance(load_json(artifact_path), schema_path, registry, artifact_path)
        print(f"validated: {artifact_path.relative_to(ROOT)}")


def starter_schema_for(path: Path) -> Path | None:
    rel = path.relative_to(STARTER_ROOT)
    parts = rel.parts
    if rel == Path("library.json"):
        return SCHEMA_ROOT / "starter_library_manifest.schema.json"
    if len(parts) == 2 and parts[0] == "roles":
        return SCHEMA_ROOT / "role_manifest.schema.json"
    if len(parts) == 2 and parts[0] == "policies":
        return SCHEMA_ROOT / "policy_manifest.schema.json"
    if len(parts) == 2 and parts[0] == "targets":
        return SCHEMA_ROOT / "target_capability_matrix.schema.json"
    if len(parts) == 2 and parts[0] == "packs":
        return SCHEMA_ROOT / "pack_manifest.schema.json"
    if len(parts) == 2 and parts[0] == "knowledge_sources":
        return KNOWLEDGE_SOURCE_SCHEMA
    if len(parts) == 2 and parts[0] == "provenance":
        return SCHEMA_ROOT / "provenance_envelope.schema.json"
    if len(parts) >= 4 and parts[0] == "packs" and parts[2] == "hooks":
        return SCHEMA_ROOT / "hook_wiring.schema.json"
    if len(parts) >= 4 and parts[0] == "packs" and parts[2] == "plugins":
        return SCHEMA_ROOT / "plugin_manifest.schema.json"
    return None


def validate_starter_json(path: Path, registry: Registry) -> None:
    schema_path = starter_schema_for(path)
    if schema_path is None:
        raise SystemExit(
            "No public schema mapping for "
            f"{path.relative_to(ROOT)}. Remediation: add a schema mapping or "
            "move implementation-only JSON outside library/starter."
        )
    data = load_json(path)
    if schema_path == KNOWLEDGE_SOURCE_SCHEMA:
        validate_knowledge_source(data, path, registry)
    else:
        validate_instance(data, schema_path, registry, path)
    print(f"validated: {path.relative_to(ROOT)}")


def validate_starter_library(registry: Registry, include_targets: bool) -> None:
    for path in sorted(STARTER_ROOT.rglob("*.json")):
        rel = path.relative_to(STARTER_ROOT)
        if not include_targets and rel.parts[:1] == ("targets",):
            continue
        validate_starter_json(path, registry)


def validate_starter_targets(registry: Registry) -> None:
    for path in sorted((STARTER_ROOT / "targets").glob("*.json")):
        validate_starter_json(path, registry)


EXPECTED_KNOWLEDGE_SCHEMES = {
    "filesystem_markdown": {"file"},
    "llms_txt_index": {"https", "file"},
    "mcp_resource": {"mcp"},
}

KNOWLEDGE_PATH_FIELDS = {
    "filesystem_markdown": ["base_path"],
    "llms_txt_index": ["static_index_path"],
    "mcp_resource": [],
}


def fail_knowledge(code: str, path: Path, message: str) -> None:
    raise SystemExit(f"{code}: {display_path(path)}: {message}")


def check_relative_knowledge_path(path: Path, field: str, value: str) -> None:
    candidate = Path(value)
    if candidate.is_absolute() or ".." in candidate.parts:
        fail_knowledge(
            "METACTL_KS_PATH_ESCAPE",
            path,
            f"/adapter/{field} must be relative and stay below the source root: {value}",
        )


def check_knowledge_uri_prefix(path: Path, scheme: str, prefix: str) -> None:
    if not prefix.startswith(f"{scheme}:"):
        fail_knowledge(
            "METACTL_KS_UNSUPPORTED_SCHEME",
            path,
            f"allowed URI prefix {prefix!r} does not use declared scheme {scheme!r}",
        )
    if scheme == "file":
        rel = prefix[len("file:") :]
        if rel.startswith("//") or rel.startswith("/") or ".." in Path(rel).parts:
            fail_knowledge(
                "METACTL_KS_PATH_ESCAPE",
                path,
                f"file URI prefix must be relative and stay below the source root: {prefix}",
            )


def validate_knowledge_source(data: Any, path: Path, registry: Registry) -> None:
    freshness = data.get("freshness")
    if isinstance(freshness, dict) and not freshness.get("owner"):
        fail_knowledge(
            "METACTL_KS_MISSING_OWNER",
            path,
            "/freshness/owner is required and must be non-empty",
        )
    validate_instance(data, KNOWLEDGE_SOURCE_SCHEMA, registry, path)
    kind = data["source_kind"]
    scheme = data["uri_scheme"]
    expected = EXPECTED_KNOWLEDGE_SCHEMES[kind]
    if scheme not in expected:
        fail_knowledge(
            "METACTL_KS_UNSUPPORTED_SCHEME",
            path,
            f"source_kind {kind!r} does not support uri_scheme {scheme!r}",
        )
    adapter = data.get("adapter", {})
    for field in KNOWLEDGE_PATH_FIELDS[kind]:
        value = adapter.get(field)
        if value is not None:
            check_relative_knowledge_path(path, field, value)
    for prefix in adapter.get("allowed_uri_prefixes", []):
        check_knowledge_uri_prefix(path, scheme, prefix)
    propose_update = data["operations"]["propose_update"]
    if propose_update["enabled"] and propose_update["mode"] == "disabled":
        fail_knowledge(
            "METACTL_KS_MUTATION_BOUNDARY",
            path,
            "enabled propose_update must use draft_only, pull_request_only, or request_only",
        )


def knowledge_source_negative_self_tests(registry: Registry) -> None:
    base_path = KNOWLEDGE_FIXTURE_ROOT / "filesystem-markdown.json"
    base = load_json(base_path)

    def expect_failure(label: str, data: Any, required_code: str) -> None:
        try:
            validate_knowledge_source(data, base_path, registry)
        except SystemExit as exc:
            if required_code not in str(exc):
                raise SystemExit(
                    f"knowledge source negative fixture {label} failed with unexpected error: {exc}"
                )
            return
        raise SystemExit(f"knowledge source negative fixture {label} unexpectedly passed")

    unsupported = json.loads(json.dumps(base))
    unsupported["uri_scheme"] = "http"
    expect_failure("unsupported-scheme", unsupported, "METACTL_KS_UNSUPPORTED_SCHEME")

    absolute = json.loads(json.dumps(base))
    absolute["adapter"]["base_path"] = "/etc"
    expect_failure("absolute-path", absolute, "METACTL_KS_PATH_ESCAPE")

    traversal = json.loads(json.dumps(base))
    traversal["adapter"]["allowed_uri_prefixes"] = ["file:../private/"]
    expect_failure("path-traversal", traversal, "METACTL_KS_PATH_ESCAPE")

    missing_owner = json.loads(json.dumps(base))
    del missing_owner["freshness"]["owner"]
    expect_failure("missing-owner", missing_owner, "METACTL_KS_MISSING_OWNER")
    print("knowledge-source-negative-fixtures: OK")


def validate_knowledge_fixtures(registry: Registry) -> None:
    for path in sorted(KNOWLEDGE_FIXTURE_ROOT.glob("*.json")):
        validate_knowledge_source(load_json(path), path, registry)
        print(f"validated: {path.relative_to(ROOT)}")
    knowledge_source_negative_self_tests(registry)


LIBRARY_STACK_EXPECTED_FAILURES = {
    "locked-conflict": "METACTL_STACK_LOCKED_OVERRIDE",
    "accidental-collision": "METACTL_STACK_ACCIDENTAL_COLLISION",
}


def ref_key(ref: dict[str, Any]) -> str:
    return f"{ref.get('kind')}:{ref.get('id')}"


def source_by_id(stack: dict[str, Any]) -> dict[str, dict[str, Any]]:
    sources: dict[str, dict[str, Any]] = {}
    for source in stack["sources"]:
        source_id = source["id"]
        if source_id in sources:
            raise SystemExit(f"METACTL_STACK_DUPLICATE_SOURCE: source {source_id}")
        sources[source_id] = source
    return sources


def active_stack_profile(stack: dict[str, Any]) -> dict[str, Any]:
    active = stack["active_profile_ref"]
    matches = [profile for profile in stack["profiles"] if profile["id"] == active]
    if len(matches) != 1:
        raise SystemExit(f"METACTL_STACK_PROFILE_NOT_FOUND: active profile {active}")
    return matches[0]


def resolve_stack_artifacts(stack: dict[str, Any]) -> list[dict[str, Any]]:
    sources = source_by_id(stack)
    profile = active_stack_profile(stack)
    overlay_id = profile["overlay_ref"]
    if overlay_id not in sources:
        raise SystemExit(f"METACTL_STACK_OVERLAY_NOT_FOUND: overlay {overlay_id}")
    overlay = sources[overlay_id]
    if overlay["source_role"] != "overlay" or overlay["read_only"] or not overlay["writable"]:
        raise SystemExit(f"METACTL_STACK_INVALID_OVERLAY: {overlay_id} must be the single writable overlay")

    order = []
    for baseline_id in profile.get("baseline_refs", []):
        baseline = sources.get(baseline_id)
        if baseline is None:
            raise SystemExit(f"METACTL_STACK_BASELINE_NOT_FOUND: baseline {baseline_id}")
        if baseline["source_role"] != "baseline" or not baseline["read_only"] or baseline["writable"]:
            raise SystemExit(f"METACTL_STACK_INVALID_BASELINE: {baseline_id} must be read-only")
        if not baseline["pinned"]:
            raise SystemExit(f"METACTL_STACK_UNPINNED_BASELINE: baseline {baseline_id} must be pinned")
        order.append(baseline)
    order.append(overlay)

    resolved: dict[str, dict[str, Any]] = {}
    for source in order:
        for artifact in source.get("artifacts", []):
            key = ref_key(artifact["artifact_ref"])
            existing = resolved.get(key)
            if existing is None:
                resolved[key] = {"source": source, "artifact": artifact, "override_status": "none"}
                continue
            existing_source = existing["source"]
            existing_artifact = existing["artifact"]
            if source["source_role"] == "overlay":
                if existing_artifact.get("locked", False):
                    raise SystemExit(f"METACTL_STACK_LOCKED_OVERRIDE: {key} from {existing_source['id']} cannot be overridden by {source['id']}")
                if existing_artifact.get("override_policy", "none") == "allow_overlay":
                    resolved[key] = {"source": source, "artifact": artifact, "override_status": "overrode_baseline"}
                    continue
                raise SystemExit(f"METACTL_STACK_ACCIDENTAL_COLLISION: {key} from {existing_source['id']} conflicts with {source['id']}")
            if existing_source["source_role"] == "baseline" and source["source_role"] == "baseline":
                if profile.get("baseline_precedence") == "explicit" and existing_artifact.get("override_policy") == "allow_baseline_precedence":
                    existing["override_status"] = "baseline_precedence"
                    continue
                raise SystemExit(f"METACTL_STACK_BASELINE_CONFLICT: {key} from {existing_source['id']} conflicts with {source['id']}")
            raise SystemExit(f"METACTL_STACK_ACCIDENTAL_COLLISION: {key}")
    return list(resolved.values())


def validate_library_stack_case(case_dir: Path, registry: Registry) -> None:
    stack_path = case_dir / "stack.json"
    stack = load_json(stack_path)
    validate_instance(stack, LIBRARY_STACK_SCHEMA, registry, stack_path)
    for source in stack["sources"]:
        validate_instance(source, LIBRARY_SOURCE_SCHEMA, registry, stack_path)
    for profile in stack["profiles"]:
        validate_instance(profile, LIBRARY_PROFILE_SCHEMA, registry, stack_path)

    expected_failure = LIBRARY_STACK_EXPECTED_FAILURES.get(case_dir.name)
    try:
        resolved = resolve_stack_artifacts(stack)
    except SystemExit as exc:
        if expected_failure and expected_failure in str(exc):
            print(f"validated: {case_dir.relative_to(ROOT)} ({expected_failure})")
            return
        raise
    if expected_failure:
        raise SystemExit(f"{expected_failure}: {case_dir.relative_to(ROOT)} unexpectedly resolved")

    lock_path = case_dir / "lock.json"
    if not lock_path.exists():
        raise SystemExit(f"METACTL_STACK_LOCK_MISSING: {lock_path.relative_to(ROOT)}")
    lock = load_json(lock_path)
    validate_instance(lock, LIBRARY_STACK_LOCK_SCHEMA, registry, lock_path)
    expected = {
        ref_key(item["artifact"]["artifact_ref"]): (item["source"]["id"], item["artifact"]["digest"], item["override_status"])
        for item in resolved
    }
    actual = {
        ref_key(item["artifact_ref"]): (item["source_id"], item["artifact_digest"], item["override_status"])
        for item in lock["resolved_artifacts"]
    }
    if actual != expected:
        raise SystemExit(
            f"METACTL_STACK_LOCK_MISMATCH: {lock_path.relative_to(ROOT)} expected {expected} got {actual}"
        )
    print(f"validated: {case_dir.relative_to(ROOT)}")


def validate_library_stack_fixtures(registry: Registry) -> None:
    for case_dir in sorted(p for p in LIBRARY_STACK_FIXTURE_ROOT.iterdir() if p.is_dir()):
        validate_library_stack_case(case_dir, registry)


def validate_fixture_dir(fixture_dir: Path, registry: Registry) -> None:
    # pack manifests
    for path in fixture_dir.glob("pack.*.json"):
        validate_instance(load_json(path), SCHEMA_ROOT / "pack_manifest.schema.json", registry, path)
    # provenance bundle items
    prov_bundle = load_json(fixture_dir / "provenance.bundle.json")
    for item in prov_bundle:
        validate_instance(item, SCHEMA_ROOT / "provenance_envelope.schema.json", registry)
    # other fixture artifacts
    for filename, schema_path in FIXTURE_SCHEMA_MAP.items():
        path = fixture_dir / filename
        validate_instance(load_json(path), schema_path, registry, path)
    # jsonrpc examples
    for filename, schema_path in JSONRPC_SCHEMA_MAP.items():
        path = fixture_dir / "jsonrpc" / filename
        validate_instance(load_json(path), schema_path, registry, path)
    validate_compile_outputs(fixture_dir)
    validate_jsonrpc_pairs(fixture_dir)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Validate metactl public contracts and fixtures.")
    parser.add_argument(
        "--include-starter-library",
        action="store_true",
        help="Validate every JSON file under library/starter against public schemas.",
    )
    parser.add_argument(
        "--include-targets",
        action="store_true",
        help="Validate starter target descriptors against the target schema.",
    )
    parser.add_argument(
        "--include-knowledge-fixtures",
        action="store_true",
        help="Validate KnowledgeSource manifests, bounded fixture adapters, and negative adapter checks.",
    )
    parser.add_argument(
        "--library-stack-fixtures",
        action="store_true",
        help="Validate LibraryStack/Baseline/Overlay schemas and semantic conflict fixtures.",
    )
    args = parser.parse_args()

    registry = schema_registry()
    validate_repo_jsonrpc_copies()
    validate_auxiliary_artifacts(registry)
    if args.include_starter_library:
        validate_starter_library(registry, include_targets=True)
    elif args.include_targets:
        validate_starter_targets(registry)
    if args.include_knowledge_fixtures:
        validate_knowledge_fixtures(registry)
    if args.library_stack_fixtures:
        validate_library_stack_fixtures(registry)
    validate_skill_audit_fixtures(registry)
    validate_host_fixtures()
    for fixture_dir in sorted(p for p in FIXTURE_ROOT.iterdir() if p.is_dir()):
        validate_fixture_dir(fixture_dir, registry)
        print(f"validated: {fixture_dir.relative_to(ROOT)}")
    print("all contracts and fixtures validated")
