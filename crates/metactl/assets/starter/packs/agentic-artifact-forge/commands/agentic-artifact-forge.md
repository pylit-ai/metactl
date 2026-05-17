# Agentic Artifact Forge

Use when the user provides a prompt, instruction file, skill, command, rule, or workflow and asks to turn it into Metactl artifacts.

Workflow:

1. Read `packs/agentic-artifact-forge/SKILL.md`.
2. Inspect the target library and existing artifacts.
3. Preserve the source language before adapting it.
4. Create the smallest useful artifact set.
5. Update manifests, grouping docs, tags, and provenance.
6. Validate and report exact files plus commands run.

Keep slash commands thin. The canonical behavior lives in the skill.
