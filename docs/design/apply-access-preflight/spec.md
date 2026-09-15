# Apply access preflight

## Problem and expected behavior

An inaccessible later destination or backup directory used to fail after earlier
outputs had changed. Compensation restored earlier bytes when possible, but this
avoidable failure still performed writes and could leave recovery work.

Before the first managed output changes, apply must check the effective access
needed for changed destinations, new backups, managed state, and journal storage.
Use actual private create/write/sync/rename/delete probes in the nearest existing
parent, not permission-bit guesses. Missing parents additionally require a
temporary directory and inherited-access check. Preserve the original OS cause.

Do not require write access to a no-op destination or to an existing backup that
will not be rewritten. Keep containment checks, snapshots, and compensation.
Clean probes on success and attempt cleanup on every error. No new dependencies.

## Limits

This is per-target apply preflight, not a transaction for an entire sync/fleet.
Compilation can update staging before apply. Probes can change directory metadata
and can be visible to watchers. They cannot prove future access, disk capacity for
full payloads, name-specific policies, target-specific ACLs/immutable flags,
permission inheritance at every future depth, or safety against concurrent
filesystem changes. Existing runtime checks and compensation remain necessary.

## Acceptance

Denied later destination, denied backup/state parent, and deterministic journal
obstructions fail without managed-file mutation. Real CLI sync errors preserve
files and permit successful retry after access is repaired. No-op destinations,
regular/symlink behavior, atomic replacement and rollback regressions pass.
