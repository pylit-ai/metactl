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
