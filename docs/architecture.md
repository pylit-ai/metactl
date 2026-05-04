# Architecture

metactl is a local deterministic context control plane for AI coding agents.

Core nouns:

- `Role`: the intended operating posture.
- `Pack`: reusable instructions and target-native resources.
- `Policy`: constraints and enforcement posture.
- `Target`: an agent/runtime surface such as Codex CLI, Claude Code, Cursor, Gemini CLI, or OpenClaw.

The core loads local libraries, resolves compatible packs against role/policy/target constraints, explains the decision, validates the result, and materializes target-owned files. `metactld` is a local stdio/JSON-RPC/MCP shim over the same kernel.
