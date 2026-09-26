# Apply access failures

Before changing managed output files, MetaCTL checks write access for changed
destinations, new backups, managed state and the apply journal. It creates small,
randomly named `.metactl-access-*` probes in existing parent directories and
removes them. Missing destination parents are not created by this preflight.
CLI apply checks all selected targets upfront and checks each again immediately
before applying it. Preview does not run access probes.

An `access preflight` error names the affected path and retains the operating
system cause. Check that the account running MetaCTL can create and rename files
and, where needed, create directories in that location. If a file occupies an
expected directory, preserve or move it deliberately before retrying. Changing
or deleting an operation lock does not repair destination permissions.

After correcting the named path, retry the original command. No-op output files
do not require directory write access, but managed state and journal writes still
do. On Unix, a read-only file can still be replaced when its parent permits an
atomic rename; preflight does not reject it merely for its mode bits.

These checks prevent common avoidable failures, not every partial application.
Concurrent changes, full disks, target-specific restrictions and later runtime
errors can still fail. MetaCTL retains snapshots and compensation for those
cases. Compilation/staging, earlier projects in a fleet operation, or earlier
targets before a later non-access failure may already have changed. See the
[behavior contract](../design/apply-access-preflight/spec.md).
