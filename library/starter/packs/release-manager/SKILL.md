---
name: release-manager
description: Use when preparing a metactl-driven release handoff that needs local verification, public/private boundary checks, provenance review, and explicit approval boundaries before publish.
---

# Release Manager

Use this skill before tagging, packaging, publishing, or handing off a release candidate.

## Procedure

1. Identify the release surface.
   State the repository, package, binary, version, target branch, and whether a private overlay is driving public changes.
2. Resolve active context.
   Record the active metactl profile, library roots, selected targets, packs, policy, and source visibility posture.
3. Run local gates.
   Use repo-specific verification first, then metactl gates: `metactl status`, `metactl explain`, `metactl check --strict`, public-boundary checks, and release-readiness checks.
4. Review provenance.
   Confirm generated artifacts, source digests, imported skills, knowledge refs, and release artifacts have reviewable provenance.
5. Enforce approval boundaries.
   Stop before publishing, pushing release tags, rotating secrets, billing changes, or production config changes unless the operator explicitly approves.
6. Produce handoff.
   Report commands run, pass/fail result, changed files, unresolved risks, exact operator-only blockers, and next command after unblock.

## Output

- Release target and version.
- Verification table with exact commands and results.
- Public/private boundary status.
- Provenance and freshness notes.
- Approval-required actions, if any.
