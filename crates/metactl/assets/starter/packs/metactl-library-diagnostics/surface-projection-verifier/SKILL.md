---
name: surface-projection-verifier
description: Verify that metactl sync actually materialized expected command, skill, rule, and extension surfaces across configured targets and projects.
---

# Surface Projection Verifier

Use this skill after `metactl sync`, fleet sync, or a profile/library change when a pack should project commands, skills, rules, hooks, extensions, or instruction surfaces into one or more projects.

## Objective

Do not trust a high-level `ready` result alone. Verify expected runtime files exist and are selected from the effective pack configuration.

## Workflow

1. Gather sync or status evidence.
   - Use existing `metactl sync --json`, `metactl status --json`, or fleet status JSON when available.
   - Extract project paths, target statuses, selected packs, and runtime paths.
   - Treat status as necessary but not sufficient.

2. Determine expected surfaces.
   - Read effective pack ids for each project.
   - Read selected pack manifests and resources.
   - Map resource kinds to target runtime paths for the enabled targets.
   - Include commands, skills, rules, hooks, schemas, and extension files when relevant.

3. Check files directly.
   - Verify expected runtime files exist.
   - For high-risk changes, inspect a small sample for expected title, id, or skill name.
   - Report missing paths by project, target, pack id, and source resource.

4. Explain misses.
   Check common causes:
   - Project `packs:` overrides profile pack selection.
   - Runtime profile differs from repo-owned profile.
   - Pack manifest failed decode.
   - Operation lock blocked sync.
   - Brownfield adoption mode refused an overwrite.
   - Target does not support the resource kind.

5. Recommend repair.
   - Update profile or project config.
   - Rerun sync for one project or the whole fleet.
   - Clear stale locks only after confirming no active metactl writer exists.
   - Escalate to metactl core if projection logic is wrong.

## Output

```markdown
## Surface Projection Verification

Sync/status source:
Expected surface set:

## Surface Matrix
| Project | Target | Pack | Resource | Expected path | Present? |
|---|---|---|---|---|---|

## Missing Surface Analysis
| Project | Missing path | Likely cause | Evidence | Repair |
|---|---|---|---|---|

## Verdict
READY | READY_WITH_MISSING_NONCRITICAL_SURFACES | NOT_READY
```

## Acceptance Criteria

- Every expected command, skill, rule, or extension surface is checked directly.
- Missing surfaces are tied to a probable config, manifest, lock, target, or sync cause.
- A `ready` status is never treated as proof of projection by itself.
- Repair commands are scoped and safe.
