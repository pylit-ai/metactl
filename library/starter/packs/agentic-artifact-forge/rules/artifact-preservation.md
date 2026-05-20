# Artifact Preservation

- Classify scope before creating or updating an agent skill, rule, command,
  prompt, or reusable workflow.
- If the artifact is not explicitly repo-only and meant to be versioned only in
  the current repository, use metactl as the canonical authoring and sync path.
- Preserve carefully worded source instructions before rewriting them.
- Prefer one canonical skill plus thin command or rule projections.
- Keep long prompts in references, prompt resources, or templates instead of always-on rules.
- Record provenance and validation evidence for new library artifacts.
- Keep private source material out of public libraries unless explicitly approved.
