---
name: agentic-artifact-forge
description: Use when given a prompt, instruction file, skill, command, rule, or workflow and asked to turn it into useful Metactl agent artifacts while preserving carefully researched source language.
---

# Agentic Artifact Forge

Use this skill to convert source instructions into the smallest useful set of Metactl artifacts: skills, slash commands, rules, workflows, prompt references, examples, scripts, or evaluators.

The core job is preservation first, projection second. Treat the source as evidence, not raw material to paraphrase away.

## Inputs

Accept any of these:

- A file path or set of file paths.
- Pasted instructions, prompts, rules, command text, or skill text.
- An existing skill that needs command or rule projections.
- An existing command or rule that should become a reusable skill.
- A library request such as "add this skill to my Metactl library."

## Required Workflow

1. Inspect the target library.
   - Find existing artifacts with the same purpose before creating new ids.
   - Read the relevant library manifest, pack manifests, grouping docs, and local instructions.
   - Respect public/private boundaries. Do not move private source text into a public library without explicit permission.

2. Preserve the source.
   - If the source is reusable and allowed in the target library, store a verbatim source reference beside the canonical artifact, such as `references/source-prompt.md`.
   - If the source cannot be copied, store provenance and a short preservation note instead.
   - Keep careful wording intact in reference files. Adapt wrappers, frontmatter, names, and routing text only as needed.

3. Classify artifact needs.
   - Create a skill when the source is a reusable capability, protocol, review method, research workflow, or procedure.
   - Create a slash command when users need a short trigger, guided entry point, or repeatable invocation. Keep it thin and point back to the skill.
   - Create a rule when the source contains durable invariants that should affect every relevant task. Keep rules short and non-procedural.
   - Create a workflow when the source is a multi-stage process with state, handoffs, rollback, or stop criteria.
   - Create prompt, reference, template, script, or evaluator files when the source includes long copy, reusable examples, setup code, rubrics, or verification checks.
   - Do not duplicate full protocols across every projection. One canonical source, thin projections.

4. Select placement.
   - Put general reusable skills in the library pack that matches how users enable the work.
   - Prefix public metactl-specific meta-skills and user-invoked commands with `metactl-` when they manage metactl itself, improve metactl libraries, or operate on metactl projection state.
   - Leave domain skills unprefixed when the task language is naturally domain-specific and the pack namespace already provides enough context.
   - Put target-specific commands and rules in command/rule harness packs when the library uses that pattern.
   - Keep long prompts or source captures as references or prompt resources, not always-on instruction text.
   - Update manifests, grouping docs, tag docs, and provenance required by the library.

5. Validate.
   - Run structural checks for manifests and frontmatter.
   - Run the library's native validation command when available.
   - Search for the new artifact by natural-language intent and by id.
   - Report exact commands and failures. Do not claim the artifact is installed or usable without evidence.

## Reverse Projection

When starting from an existing skill:

- Add a slash command only if a short user-facing trigger is useful.
- Add rules only for stable constraints, not step-by-step workflow text.
- Extract prompt references or templates only when users need to paste or reuse the exact language.
- Keep generated commands and rules visibly subordinate to the skill.

When starting from a command or rule:

- Promote to a skill if the behavior is reusable, multi-step, or needs context that would bloat the command/rule surface.
- Leave the original command/rule as a thin invocation layer if users already know it.

## Output Contract

Return:

- Source material inspected.
- Existing artifact reused or new artifact ids created.
- Artifact classification and why.
- Files created or modified.
- Preservation choice: verbatim reference, provenance-only, or adapted source with reason.
- Validation commands and results.
- Follow-up needed, if any.

## Guardrails

- Do not hide source loss. If wording was condensed, say what moved to a reference and what was adapted.
- Do not publish private paths, account names, credentials, customer details, or proprietary instructions into a public library.
- Do not create a new command for every skill by default. Commands need a real invocation use case.
- Do not create always-on rules from long prompts. Rules should be stable constraints.
- Do not overwrite a better existing artifact; update or extend it when the purpose matches.
- Do not rely on plugin-only namespace display for portable skill clarity. Some clients expose only the skill name.
