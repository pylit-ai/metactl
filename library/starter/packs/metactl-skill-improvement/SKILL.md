---
name: metactl-skill-improvement
description: Use when a user or agent reports that a metactl skill, slash command, rule, pack, or projected agent surface did not work and wants the fix made in the canonical metactl library source before optionally syncing one repo or a fleet.
---

# metactl Skill Improvement

Use this skill to convert feedback about a metactl skill, slash command, rule, workflow, or generated agent surface into a durable library improvement.

The core rule: improve the canonical library source, then project it. Do not hand-edit projected agent outputs as the durable fix.

## Inputs

Accept any of these:

- The user's feedback in plain language.
- A failed command, screenshot summary, transcript excerpt, or copied output.
- A path to a projected surface such as an agent skill, command, rule, or root instruction file.
- The affected pack id, skill name, command name, target runtime, repo path, or fleet controller.
- An apply preference: `none`, `repo-preview`, `repo-apply`, `fleet-preview`, or `fleet-apply`.

## Required Workflow

1. Capture the feedback.
   - Record what the user expected, what happened instead, the exact trigger phrase or command, and the affected target runtime.
   - Preserve short evidence snippets and file paths. Redact secrets, account identifiers, customer data, private URLs, and proprietary instructions before writing public artifacts.
   - If the report is vague, make the smallest useful local inspection first. Ask only when the missing fact is an operator-only product decision.

2. Resolve the canonical source.
   - Use `metactl explain`, `metactl search`, pack manifests, and local library roots to map projected output back to the owning pack resource.
   - Treat target-specific directories and generated root instruction files as compiled output unless local docs say otherwise.
   - If the failure belongs to metactl kernel behavior, tests, or projection semantics, update the metactl repo issue/spec/test path instead of hiding it in a skill workaround.

3. Classify the improvement.
   - Update a skill when task selection, workflow, context loading, guardrails, examples, output shape, or platform deltas are wrong.
   - Update a command when the user needs a better slash entry point. Keep command text thin and point back to the skill.
   - Update a rule only for durable invariants that should affect every relevant task.
   - Add a reference, template, evaluator, or test fixture when the feedback needs examples, preservation, or repeatable verification.

4. Design the patch.
   - Prefer the smallest durable change that would have prevented the failure.
   - Keep descriptions trigger-oriented: use words users actually said, such as "improve skill", "skill feedback", "slash command failed", or "projected copy is stale".
   - Prefix public metactl meta-skills and user-invoked commands with `metactl-` unless the artifact is a domain skill whose pack namespace already gives enough context.
   - Keep the portable core free of target-specific paths. Put tool-specific material in a platform or projection section.
   - Preserve carefully worded source material in a reference file when it matters.

5. Verify before projection.
   - Validate the changed library manifests and schemas.
   - Check retrieval with the user's phrase, for example `metactl search "improve skill"`.
   - Preview projection before applying: `metactl sync --preview --project /path/to/repo`.
   - Run target-specific smoke checks when the change affects projected commands, rules, or skills.

## Apply Modes

Default to `repo-preview` when the user asks for immediate use but does not specify scope.

| Mode | Action | Permission posture |
| --- | --- | --- |
| `none` | Write or propose only the canonical library improvement. | Safe default for design review. |
| `repo-preview` | Run `metactl sync --preview --project /path/to/repo`. | Safe, no runtime file mutation. |
| `repo-apply` | Run `metactl sync --apply --project /path/to/repo` after a clean preview. Use `--adopt patch` for brownfield roots when needed. | Requires explicit user request or existing repo policy allowing apply. |
| `fleet-preview` | Run `metactl fleet sync --preview` from the controller or with `--project /path/to/controller`. | Safe, no linked project mutation. |
| `fleet-apply` | Run fleet apply only after reviewing linked projects, dirty worktrees, and preview output. | Requires explicit user request. Multi-repo writes are not the default. |

Do not use `fleet-apply` as a convenience shortcut for one repo. Fleet applies are for an explicitly maintained controller with reviewed `linked_projects`.

## Output Format

Return:

- `feedback_summary`
- `canonical_source`
- `classification`
- `files_changed`
- `projection_scope`
- `verification`
- `apply_result`
- `operator_only_blockers`
- `next_step`

If blocked, name the exact minimal unblocker and whether it is truly operator-only.

## Guardrails

- Do not copy private source text into a public library without explicit permission.
- Do not bury kernel defects in skill prose.
- Do not treat generated target output as the root fix.
- Do not apply to a fleet without explicit user request and clean preview evidence.
- Do not shrink the user's report into a cosmetic wording tweak when the failure is about routing, source resolution, projection, or verification.

## Platform Deltas

- Codex and Claude project skills are projected under tool-specific skill folders; fix the library pack resource first, then sync.
- Cursor command and rule projections should stay thin. Canonical behavior remains in the skill.
- Plugin-aware clients may display a plugin namespace such as `superpowers:brainstorming`; portable installs may expose only the skill name. Keep the `metactl-` prefix in public metactl meta-skills so they remain clear outside plugin-aware clients.
- Gemini and OpenClaw projections may have reduced surface support. Preserve the same behavior in the portable core and document target-specific degradation in the manifest or validation result.
