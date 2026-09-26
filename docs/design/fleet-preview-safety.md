# Fleet preview and lock ownership design

## Problem and contract

Fleet preview previously rendered a command without resolving or compiling the
member. Missing inputs and adoption conflicts appeared only during apply. A
controller listed as its own member spawned a second process which contended
with the controller's correctly held lock. Finally, a failed log append discarded
the successful member results from the command's error response.

The change preserves writer exclusion and the existing sync implementation.
Preview compiles from the original `ProjectContext` into a disposable directory;
the read-only materializer planner accepts separate staging and destination roots.
Relative-path validation and containment apply independently to each root.
Normal apply still uses the same-root wrapper and unchanged lock acquisition.
This avoids copying repositories or weakening containment for temporary paths.

`cli_fleet_sync` owns iteration and member invocation; `cli_fleet_plan` owns
read-only preparation and expected coverage. The controller module retains
discovery, presentation and settings. No dependencies or generic filesystem
abstraction are introduced.

## Verification obligations

- Invoke the real binary on disposable projects with first/middle/last outcomes.
- Compare complete path/type/bytes/mode/symlink snapshots before and after preview.
- Exercise missing pack, target, profile, malformed config and refuse collisions.
- Compare expected skills and files with actual applied member outputs.
- Repeat preview on managed projects; refuse destination symlink escapes.
- Exercise self-members through direct, symlink and parent-path aliases.
- Preserve external lock bytes and continue after locked non-controller members.
- Force log failure and retain applied member outcomes in the error payload.

Remote freshness, write probes and concurrent changes remain apply-time checks;
the preview JSON explicitly records that remote freshness was not checked.
