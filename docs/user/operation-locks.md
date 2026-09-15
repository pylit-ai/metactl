# Operation locks and filesystem failures

MetaCTL creates `.metactl/state/operation.lock` exclusively before a mutating
command. A second writer must fail rather than overwrite another command's
work. This temporary operation lock is different from `metactl.lock.json`, which
records resolved configuration.

All operation-lock acquisition failures exit with status **10**. JSON and agent
output contain a specific `code` and `path`; the general `error_code` remains
`state` for compatibility.

| Code | Meaning | Recovery |
| --- | --- | --- |
| `operation_lock_active` | Exclusive creation found an existing lock below the age threshold, or of unknown age. This does not prove the owner is running. | Wait for the writer. If interrupted, confirm no writer owns the lock before considering removal. |
| `operation_lock_stale` | An existing lock is at least six hours old. Age does not prove abandonment. | Check the owning command, including long-running work, before recovery. |
| `operation_lock_permission_denied` | The OS denied an acquisition operation. | Check permissions and sandbox access to the reported path; retry with authorized write access. |
| `operation_lock_io` | Another filesystem failure prevented acquisition or initialization. | Resolve the reported OS cause, such as an invalid state-directory path or unavailable storage. |

Filesystem failures also include `operation`, `io_kind`, `raw_os_error` (null if
unavailable), and `cause`. Operations are `create_state_directory`, `create_lock`,
and `initialize_lock`. Classify automation using `code`, not localized OS messages
or platform-specific error numbers. Text output retains the same OS cause and
recovery guidance.

MetaCTL never automatically removes a pre-existing lock. Do not delete a lock
just because it is old or a command reports permission denied. Inspect the lock's
PID, command and start time, check for the owning writer, and inspect repository
state. Only remove an abandoned lock after confirming no writer owns it. Prevent
other writers from starting during manual recovery.

Normal completion releases the owned lock. Failure to write or flush its initial
payload also attempts to release the newly created lock; if the filesystem
rejects cleanup, it may remain and require inspection. A process crash can also
leave a lock behind. No automatic takeover or process-liveness guarantee is
provided by this file-based protocol.

Flushing the initial payload is now checked: a failed flush refuses the command
instead of silently proceeding with an incompletely initialized lock.

## Scope and further core work

This change repairs diagnostics and failed-initialization cleanup. It does not
make filesystem writes transactional or implement fleet-wide preflight.

Recommended next improvements, in order:

1. **Destination and recovery preflight.** Validate all intended destinations,
   backup paths, and authoritative state paths before the first managed-file
   mutation. Test denial on a later action and prove earlier content remains
   unchanged. Access can change after a check, so preserve rollback and report
   compensation failures; do not promise that preflight eliminates partial writes.
2. **Real fleet preview.** Current fleet preview reports selection and readiness;
   it does not execute each project's sync preview. Run per-project planning with
   the effective profile and expose conflicts, expected output paths, and selected
   skill coverage. Define whether staging is permitted and distinguish planned
   coverage from post-apply verification of actual files.
3. **Fleet controller self-contention.** Fleet apply holds the controller lock
   while spawning child sync commands. Detect a controller also registered as a
   linked project and avoid acquiring the same lock recursively. Test this case
   without weakening protection against independent writers.

The [behavior contract](../design/operation-lock-errors/spec.md),
[implementation plan](../design/operation-lock-errors/plan.md), and
[verification ledger](../design/operation-lock-errors/tasks.md) describe this patch.
