---
name: library-organization-guide
description: Use when curating a metactl library, starter pack set, or profile stack and deciding how to cluster artifacts without bloating default context.
---

# metactl Library Steward

Use this skill when you need an agent-facing interface for maintaining metactl libraries, pack boundaries, profile defaults, or project bindings.

## Workflow

1. Inspect the current metactl state first.
   Identify the active library roots, project binding, local overrides, and current pack set before proposing changes.
2. Classify each artifact by management layer.
   Decide whether it belongs in a user profile, shared `metactl.yaml`, or gitignored `metactl.local.yaml`.
3. Cluster artifacts into packs by how users enable work.
   Keep base capabilities separate from wrappers, harnesses, or target-specific helpers when users may want the core without the wrapper.
4. Name public metactl meta-artifacts for portable discovery.
   Prefix public metactl-specific meta-skills, packs, and user-invoked commands with `metactl-` when they manage metactl itself, improve metactl libraries, or operate on metactl projection state. Do not rely on plugin namespaces as the only disambiguator; some clients expose only the portable skill name.
5. Draft a small profile matrix.
   Start with `catalog`, `generalist`, and only the specialist profiles justified by real workflows such as `research`, `authoring`, or `verification`.
6. Explain context discipline explicitly.
   For every pack or profile, state what belongs in the default set, what stays opt-in, and why.
7. Emit concrete drafts and commands.
   Produce pack manifest drafts, profile YAML drafts, and exact metactl commands to apply the chosen layout.
8. Place Fleet controllers separately from packs.
   If the work spans multiple linked local projects, put the Fleet controller in `/path/to/metactl-library/fleet/<name>`, `~/.config/metactl/fleet/<name>`, or a dedicated private repo. Create it with `metactl fleet controller init <name>` or `metactl fleet controller init <name> --path /path/to/controller`. Do not put controller registries under `packs/`.

## Output Format

- Current-state summary with: `library root`, `profile`, `project binding`, `local overrides`
- Artifact inventory with: `artifact`, `kind`, `target scope`, `base vs wrapper`, `recommended pack`
- Pack proposals with: `pack id`, `purpose`, `included artifacts`, `excluded artifacts`, `rationale`
- Profile matrix with: `profile`, `default packs`, `storage layer`, `why this stays small`
- Exact metactl command sequence when the user asks to apply the recommendation
- Fleet controller recommendation when the user is coordinating multiple repos
- Risks or ambiguities that still need a human decision
- Exact manifest or YAML drafts when the user asks for them

## Guardrails

- Do not make pack boundaries mirror raw directory layout if that layout does not match how users enable work.
- Do not put target-specific wrappers into a generalist default unless they are broadly useful and low-noise.
- Do not copy private or user-specific artifacts into a public starter pack.
- Do not give public metactl meta-skills generic ids such as `skill-improvement` when `metactl-skill-improvement` explains ownership and avoids cross-library collisions.
- Prefer a few small defaults plus explicit specialist profiles over one giant default profile.
- If two artifacts always move together at runtime, keep them together. If users often want one without the other, split them.
- Explain where information lives: global library, shared project config, and local overrides are separate decisions.
- Explain where Fleet information lives: `linked_projects` belongs in the controller project's `metactl.yaml`; the user-global config stores only the selected controller pointer.
- Prefer explicit metactl operations and file locations over vague advice. If suggesting a binding or override, name the file that changes.
- Do not use user-specific private paths in reusable docs or starter packs. Use placeholders such as `/path/to/metactl-library/fleet/<name>`.
