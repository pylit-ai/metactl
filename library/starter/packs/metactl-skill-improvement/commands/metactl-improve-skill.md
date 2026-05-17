# metactl Improve Skill

Use when a user or agent reports that a metactl skill, slash command, rule, pack, or projected agent surface did not work and the fix should live in the canonical metactl library source.

Workflow:

1. Read `packs/metactl-skill-improvement/SKILL.md`.
2. Capture the feedback, expected behavior, actual behavior, trigger phrase, target runtime, and evidence.
3. Resolve the projected surface back to its canonical pack resource.
4. Patch the smallest durable source artifact.
5. Validate retrieval and projection with preview first.
6. Apply only to the requested scope: `none`, `repo-preview`, `repo-apply`, `fleet-preview`, or `fleet-apply`.

Keep this slash command thin. The canonical protocol lives in the skill.
