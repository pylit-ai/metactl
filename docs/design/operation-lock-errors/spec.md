# Operation lock failure contract

Status: implemented; verification in progress.

## Problem

The CLI maps every operation-lock acquisition failure to active contention and
suggests lock removal, even when the filesystem rejected directory or file
creation. This loses the OS cause and directs users toward an unrelated remedy.

## Required behavior

- Keep exclusive creation and exit status 10 for lock acquisition failures.
- Classify existing locks by typed errors, preserving `operation_lock_active`
  and `operation_lock_stale` for compatibility. Neither code proves a live or
  dead owner; the stale threshold is only an age heuristic.
- Never automatically remove a pre-existing lock. Recovery advice must require
  confirming that no writer owns it, including when it is old.
- Report permission failures as `operation_lock_permission_denied`; report other
  filesystem failures as `operation_lock_io`. Include operation, path, error
  kind, raw OS error when available, and the original cause. Never recommend
  waiting for a writer or deleting locks for these errors.
- Return equivalent actionable guidance in text, JSON, and agent output.
- Clean up a lock created by this attempt if payload initialization fails.
- Preserve managed content and pre-existing lock bytes on refusal.

## Scope

No dependency updates, lock-file format changes, automatic takeover, release,
or global user-state changes. Destination/recovery preflight and real fleet
preview are separate changes, not claims made by this diagnostic repair.

## Acceptance

CLI regressions cover real denied writes, invalid state paths, active/stale
locks, and refusal without content mutation. Library tests cover initialization
failure cleanup and OS error classification. Existing concurrency tests pass.
