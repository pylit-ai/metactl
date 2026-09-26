# Fleet command ownership

Extract the existing fleet workflow into one internal command module. Preserve
command arguments, defaults, exit codes, human/JSON/agent output, profile and
controller selection, child-process arguments, locking lifetime and file effects.
This change does not repair preview or self-contention behavior; those changes
follow separately so their tests cannot conceal a refactoring regression.

`main.rs` remains the entry point and dispatcher. Fleet-specific helpers belong
with fleet commands; shared helpers stay with existing consumers. No new public
API, dependency, configuration option, or generic command framework.

The architecture check must pass unchanged limits and run in CI. Add a budget
for the new module so extraction does not simply relocate unbounded growth.
