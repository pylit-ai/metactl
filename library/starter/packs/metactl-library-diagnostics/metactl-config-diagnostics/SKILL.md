---
name: metactl-config-diagnostics
description: Explain effective metactl pack selection, profile/config precedence, source drift, and why a pack or surface did or did not project.
---

# metactl Config Diagnostics

Use this skill when `metactl sync`, profile activation, or projected agent surfaces behave differently than expected.

## Objective

Explain the effective metactl configuration from local files, runtime profiles, project overrides, lock files, selected packs, and target support. The output should answer: why did this pack or surface project, not project, or project from an unexpected source?

## Workflow

1. Identify config sources.
   - Project `metactl.yaml`.
   - Fleet or workspace config when present.
   - Repo-owned profile files.
   - Runtime profile files under the metactl config directory.
   - `metactl.lock.json`.
   - Pack manifests and provenance.

2. Resolve precedence.
   - Record active profile name and profile path.
   - Detect whether project `packs:` replaces, narrows, or supplements profile pack selection.
   - Detect profile drift between repo-owned profile files and runtime profile files.
   - List starter libraries, private sources, and local source layers.

3. Build effective pack selection.
   - List selected pack ids.
   - List expected but absent pack ids.
   - List suppressed, incompatible, or decode-failed packs with reason.
   - Show which config source introduced each selected pack when possible.

4. Explain target projection.
   - List enabled targets.
   - For the pack or surface being debugged, map resources to expected runtime paths.
   - Record target capability gaps or brownfield adoption decisions.

5. Recommend the smallest repair.
   - Update project `metactl.yaml`.
   - Update repo-owned profile.
   - Sync runtime profile.
   - Fix manifest resource kind or provenance.
   - Rerun one-project sync before whole-workspace sync.

## Output

```markdown
## metactl Config Explanation

Question:
Project:
Active profile:
Runtime profile path:
Repo profile path:

## Effective Pack Selection
| Pack id | Source | Selected? | Reason |
|---|---|---|---|

## Profile / Project Precedence
| Config source | Field | Behavior | Evidence |
|---|---|---|---|

## Target Projection
| Target | Resource | Expected path | Status |
|---|---|---|---|

## Diagnosis
Cause:
Smallest repair:
Verification command:
```

## Acceptance Criteria

- The explanation names the active profile path and project config path.
- Effective pack ids are listed rather than inferred from memory.
- Project pack overrides and profile drift are explicit.
- The repair is the smallest safe change, not a broad resync by default.
