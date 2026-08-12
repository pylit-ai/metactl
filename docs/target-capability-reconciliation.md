# Target capability and reconciliation v1

## Codex discovery contract

- Canonical repository skill root: `.agents/skills`.
- Legacy read root: `.codex/skills`.
- metactl writes only the canonical root and never deletes legacy artifacts.
- The target records its verification date and official source in target metadata.

## Review and apply contract

Preview creates a complete JSON plan under `.metactl/plans/`. The semantic SHA-256
digest covers project identity, tool and target versions, manifest and managed-state
digests, apply mode, every ordered action, before/desired digests, classification,
reason, consequence, approval state, and legacy evidence.

An explicit `sync --apply` or `apply` accepts `--plan-digest` and fails closed when
the current plan differs. Rendered subsets are non-authorizing. A receipt is written
under `.metactl/receipts/` with private permissions after success, no-op, or conflict.
Every apply also uses a private append-only JSONL action journal. A later action failure
restores all preflight snapshots when safe and reports `compensated_failure`; an
incomplete restoration reports `failed_partial` and never writes success state.

## Collision taxonomy

`legacy_only`, `canonical_only`, `identical_duplicate`, `content_conflict`,
`unmanaged_canonical`, `unsafe_alias`, and `platform_name_collision` are stable
classifications. Content conflicts and unmanaged canonical bytes require an explicit
repair/adoption decision; neither legacy nor unmanaged bytes are silently replaced.

## Safety invariants

- Generated paths must be relative, non-empty ASCII paths with no parent, absolute,
  backslash, repeated-separator, or ambiguous components.
- Existing ancestor symlinks are rejected. Managed leaf symlinks remain supported and
  are bound by both their project-relative target identity and content digest. External
  leaf targets are rejected.
- Staged files must remain below `.metactl/generated/<target>/`.
- Backups, journals, plans, and receipts use private permissions on Unix. Rollback
  refuses any managed path whose current digest differs from the applied receipt state.
- No network, telemetry, authenticated probe, real user-root read, or deletion is added.

## Evidence limits

Passing fixtures prove deterministic local filesystem behavior. They do not prove
authenticated Codex activation, cross-fleet safety, broad human usability, or causal
task benefit.
