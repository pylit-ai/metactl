# Verification ledger

- [x] Baseline failure reproduced: denied second destination returned
  `compensated_failure`, restoring one earlier mutation. Regression failed with
  exit 101 before implementation.
- [x] Destination/recovery probes implemented without new dependencies.
- [x] Real CLI denial, permission repair/retry, repeated sync and journal
  obstruction tests pass in text, JSON and agent modes.
- [x] Focused materializer permission, recovery, no-op and rollback tests pass.
- [x] First per-target checkpoint full workspace suite passes, exit 0.
- [x] Reproduced cross-target failure before the upfront pass: later target
  denial left the first target's `AGENTS.md` written. Added the CLI-wide pass.
- [ ] Final full workspace, formatting and public-boundary checks.
- [ ] Independent review and remote-head verification.

Unix permission fixtures detect runners that bypass mode restrictions and emit a
skip message in that case. Deterministic obstruction and cleanup tests run on
all platforms. The development run exercised actual denied writes as a normal
user; it did not skip permission fixtures. Windows execution remains CI evidence.
