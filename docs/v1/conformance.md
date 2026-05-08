# metactl v1 Conformance

The v1 conformance matrix lives in `fixtures/v1/conformance.matrix.json` and validates against `contracts/schemas/metactl/conformance_matrix.schema.json`. It covers Claude Code, Codex CLI, Cursor, Gemini CLI, OpenClaw, and the generic filesystem-agent target.

A target claim is release-ready only when its descriptor, generated outputs, apply modes, and listed gates agree. Experimental targets may ship as fixtures, but their degraded or generic behavior must be explicit.
