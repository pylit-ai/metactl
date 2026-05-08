# Support Matrix

Support tiers describe what this repository verifies today.

| Target | Tier | Evidence |
| --- | --- | --- |
| Codex CLI | Tier 1, conformance-covered | Public fixtures and smoke tests cover generated Codex surfaces. |
| Claude Code | Tier 1, conformance-covered | Public fixtures and smoke tests cover generated Claude surfaces. |
| Cursor | Tier 2, preview | Public fixtures cover the target, but compatibility is not yet a release promise. |
| Filesystem Agent | Experimental | Generic descriptor fixture for agents that read files from a project tree. |
| Gemini CLI | Tier 2, preview | Public fixtures cover the target, but compatibility is not yet a release promise. |

## Tier Definitions

- Tier 1: release-blocking fixtures and smoke tests exist.
- Tier 2: fixtures exist, but failures may not block a release.
- Experimental: examples may exist, but compatibility is not claimed.

Compatibility statements are valid only for the released `metactl` version and the target versions tested in CI.
