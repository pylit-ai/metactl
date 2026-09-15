# Architecture

metactl is a local deterministic context control plane for AI coding agents.

The canonical v1 scope is [docs/v1/charter.md](v1/charter.md). metactl v1 is a private-by-default deterministic resolver/compiler/validator. Its source of truth is a private library stack: 0..N pinned read-only baseline libraries selected by active project/profile, exactly one writable overlay per active profile, then generated project projections.

Existing runtime nouns:

- `Role`: the intended operating posture.
- `Pack`: reusable instructions and target-native resources.
- `Policy`: constraints and enforcement posture.
- `Target`: an agent/runtime surface such as Codex CLI, Claude Code, Cursor, Gemini CLI, or OpenClaw.

The core loads local libraries, resolves compatible packs against role/policy/target constraints, explains the decision, validates the result, and materializes target-owned files. `metactld` is a local stdio/JSON-RPC/MCP shim over the same kernel.

Library-stack nouns:

- `Baseline`: a pinned read-only library source selected by the active profile.
- `Overlay`: the single writable private library for the active profile.
- `Profile`: the explicit selector for baseline order, overlay location, and projection policy.
- `Projection`: generated target-native files in a project, never the canonical source.
- `Public example`: generic OSS material authored or generated from safe fixtures.
- `Sanitized export`: an explicit reviewed export from private source material with dropped fields and provenance recorded.

## Internal ownership and checks

The CLI entry point owns argument parsing and dispatch. Focused command modules
own workflows: `cli_fleet.rs` contains fleet selection, controller resolution,
execution, and reporting, alongside the existing source/profile/pack modules.
Shared CLI helpers remain with their existing consumers; module interfaces are
internal to the binary.

`library_registry.rs` owns library loading and resolution/compile orchestration.
`library_discovery.rs` owns candidate normalization and search evidence;
`library_instruction_format.rs` owns instruction formatting and byte-budget
rules. Filesystem application and recovery remain in the materializer.

`scripts/check_architecture_metrics.sh` enforces explicit file/function budgets
for the orchestration files and these extracted helpers. CI runs it on pull
requests. Limits are guardrails, not proof of sound design: changes must also
preserve observable contracts and have tests for user-facing failure paths.
See the [fleet extraction contract](design/fleet-module/spec.md) and
[registry boundaries](design/registry-boundary.md).
