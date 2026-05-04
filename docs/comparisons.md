# Comparisons

## Why Not Just AGENTS.md?

`AGENTS.md` is a useful instruction surface, but it is one target-owned file. `metactl` keeps source artifacts separate from generated target files, validates the projection, and can produce multiple target-native surfaces from the same inputs.

## Why Not Just MCP?

MCP is a protocol surface. `metactl` is a local compiler and validation layer for agent-facing repository artifacts. The two can work together: `metactld` exposes local JSON-RPC/MCP behavior over the same kernel.

## Why Not Editor-Specific Rules?

Editor-specific rules are useful at runtime, but each tool has different file layouts, precedence, and capabilities. `metactl` makes those differences explicit and testable.
