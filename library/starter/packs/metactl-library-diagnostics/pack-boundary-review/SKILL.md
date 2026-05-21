---
name: pack-boundary-review
description: Review metactl pack changes for over-fragmentation and decide whether new artifacts belong in an existing pack or require a distinct pack boundary.
---

# Pack Boundary Review

Use this skill before adding or approving a new metactl pack, especially when a change introduces a single-skill pack, command-only pack, rule-only pack, or a pack whose resources point at an existing capability.

## Objective

Prevent pack sprawl. Keep commands, rules, prompts, references, and helper files attached to the owning capability pack unless there is a real install, ownership, dependency, source, trust, or target-runtime boundary.

## Workflow

1. Inventory the proposed change.
   - List new or modified pack manifests.
   - List new skills, commands, rules, prompts, scripts, examples, schemas, hooks, assets, and provenance files.
   - Identify the canonical capability owner for each resource.

2. Check for existing ownership.
   - Search existing packs for the same capability, workflow, or domain.
   - If a skill already lives in a capability pack, its command and rule surfaces normally belong in that same pack.
   - Longform prompts should normally be grouped by the library's prompt policy, not made into one-off packs.

3. Test for a real pack boundary.
   A separate pack is justified only when at least one condition is true:
   - Different owner or review authority.
   - Different source ecosystem, vendor, or license boundary.
   - Different trust tier or confirmation policy.
   - Different install or activation profile.
   - Different target runtime requirement that would be harmful for the owning pack.
   - Different dependency set, generated adapter, hook, binary, model, or external service.
   - Distinct release cadence or public/private boundary.

4. Reject weak boundaries.
   These are not enough by themselves:
   - One skill needs a slash command.
   - One skill needs an editor rule.
   - A prompt was converted into a skill.
   - The new artifact has a memorable name.
   - Projection is easier if the pack is separate.
   - A generation script already supports static packs.

5. Recommend the placement.
   - `FOLD_INTO_EXISTING_PACK`: move resources into the owning capability pack.
   - `KEEP_DISTINCT_PACK`: keep the new pack and state the boundary.
   - `NEEDS_BOUNDARY_JUSTIFICATION`: pause before merge because the boundary is unclear.

6. Verify the result.
   - Validate JSON manifests and provenance.
   - Search by natural-language intent and by pack id.
   - Check effective profile selection if the change affects projection.
   - Confirm no removed pack id remains in profiles, docs, scripts, or project configs.

## Output

```markdown
## Pack Boundary Review

Verdict: FOLD_INTO_EXISTING_PACK | KEEP_DISTINCT_PACK | NEEDS_BOUNDARY_JUSTIFICATION

## Resource Inventory
| Resource | Kind | Proposed pack | Owning capability | Recommended pack |
|---|---|---|---|---|

## Boundary Test
| Boundary criterion | Evidence | Satisfied? |
|---|---|---|

## Placement Decision
Decision:
Rationale:
Required edits:

## Verification
| Check | Result | Evidence |
|---|---|---|
```

## Acceptance Criteria

- Every new pack has an explicit boundary justification.
- One-skill command/rule packs are folded into the owning pack unless a real boundary exists.
- Prompt resources follow the library's prompt policy.
- Profiles and project configs do not retain obsolete pack ids.
- The resulting pack graph is easier to explain than the proposed one.
