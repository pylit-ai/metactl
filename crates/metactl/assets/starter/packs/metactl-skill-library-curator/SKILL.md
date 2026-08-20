---
name: metactl-skill-library-curator
description: Use when several skills are hard to distinguish, a skill library needs a naming or routing audit, or you need a review plan for aliases, relations, pack placement, merges, splits, or retirement.
---

# MetaCTL Skill Library Curator

Use this skill to improve how a MetaCTL skill library is organized and found.
It is for library-wide ambiguity, not an isolated rewrite of one skill.

This skill is read-only by default. It produces a review plan; it does not
rename, merge, publish, install, or edit canonical skill sources without the
separate approval required by the owning workflow.

## Use when

- several skills could plausibly answer the same request;
- an agent cannot find a skill from the words a user naturally uses;
- aliases, positive intents, negative intents, compatibility, or reviewed
  relations need a deliberate audit;
- a proposed skill may belong in an existing pack instead of a new pack; or
- a library needs a bounded merge, split, lifecycle, or shared-resource plan.

Do not use this skill for a one-file wording edit with no library-level
ambiguity, applying an already approved patch, or proving downstream task
quality. Those are separate authoring, implementation, and evaluation tasks.

## Workflow

1. Freeze a baseline: record the library digest, current routing evidence, and
   any existing tests. Do not infer real-world usage from missing telemetry.
2. Choose a small competing neighborhood: the confusing skill, plausible
   siblings, and any rare-but-important specialist.
3. Check selection information separately from procedure: name, aliases,
   intents, facets, compatibility, and reviewed relations decide discovery;
   the skill body decides what happens after selection.
4. Route representative requests with `metactl skills route <query> --json`.
   Include plausible-but-wrong requests that must not select a neighboring
   skill.
5. Classify the relationship: keep, clarify boundary, add alias, compose,
   extract shared material, merge, split, or retire. Do not merge skills that
   differ in authority, trust, target compatibility, or lifecycle.
6. Write a proposal with exact subjects, expected routing effect, evidence,
   approval required, rollback condition, and verification command.

## Decision guide

| Discovery overlap | Procedure overlap | Default response |
| --- | --- | --- |
| High | High | Merge, deduplicate, or retire one surface. |
| High | Low | Clarify the boundary; add aliases/intents or rerank. |
| Low | High | Extract shared references or deterministic helpers. |
| Low | Low but commonly paired | Add a reviewed relation or composition. |
| Broad catch-all | Any | Narrow, split, or make it on-demand. |
| Rare but critical | Any | Preserve it and improve the route/gate. |

## Required output

```markdown
# Skill Library Curation Review

Scope: <skills and packs>
Baseline: <digest or revision>

| Priority | Subjects | Diagnosis | Proposed action | Evidence | Approval |

## Positive routing probes

## Hard negatives

## Verification and rollback
```

## Guardrails

- Routing success proves retrieval behavior, not that the selected skill
  improves the downstream task. Use a paired evaluation before making a large
  claim about task benefit.
- Keep source edits approval-gated. A recommendation is not permission to
  mutate a library or deploy its projections.
- Keep public starter content portable: do not include local paths, customer
  data, account details, credentials, or private instructions.
