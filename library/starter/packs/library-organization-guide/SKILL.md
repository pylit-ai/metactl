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
4. Draft a small profile matrix.
   Start with `catalog`, `generalist`, and only the specialist profiles justified by real workflows such as `research`, `authoring`, or `verification`.
5. Explain context discipline explicitly.
   For every pack or profile, state what belongs in the default set, what stays opt-in, and why.
6. Emit concrete drafts and commands.
   Produce pack manifest drafts, profile YAML drafts, and exact metactl commands to apply the chosen layout.

## Output Format

- Current-state summary with: `library root`, `profile`, `project binding`, `local overrides`
- Artifact inventory with: `artifact`, `kind`, `target scope`, `base vs wrapper`, `recommended pack`
- Pack proposals with: `pack id`, `purpose`, `included artifacts`, `excluded artifacts`, `rationale`
- Profile matrix with: `profile`, `default packs`, `storage layer`, `why this stays small`
- Exact metactl command sequence when the user asks to apply the recommendation
- Risks or ambiguities that still need a human decision
- Exact manifest or YAML drafts when the user asks for them

## Guardrails

- Do not make pack boundaries mirror raw directory layout if that layout does not match how users enable work.
- Do not put target-specific wrappers into a generalist default unless they are broadly useful and low-noise.
- Do not copy private or user-specific artifacts into a public starter pack.
- Prefer a few small defaults plus explicit specialist profiles over one giant default profile.
- If two artifacts always move together at runtime, keep them together. If users often want one without the other, split them.
- Explain where information lives: global library, shared project config, and local overrides are separate decisions.
- Prefer explicit metactl operations and file locations over vague advice. If suggesting a binding or override, name the file that changes.
