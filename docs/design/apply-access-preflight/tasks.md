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
- [x] Multi-target checkpoint `cargo test --offline --workspace`: 360 tests
  passed, exit 0.
- [x] Combined-branch tests identified a project-root symlink alias regression;
  reproduced through actual CLI sync before the fix. Parent access inspection
  now follows the root alias after existing relative-path containment checks.
- [x] Final `cargo test --offline --workspace`, including the root-alias
  regression: 361 tests passed, exit 0.
- [x] `cargo fmt --check`, public-boundary and `git diff --check`: exit 0.
- [x] Independent review of final runtime change: no actionable findings.
  Checked multi-target plan freshness, conflict precedence, probe cleanup,
  root-alias compatibility, containment and documented limitations.

Exact remote head and PR state are verified in the integration receipt at handoff.

An initial overlapping build/test run encountered one subprocess `NotFound`;
the isolated test and exclusive full reruns passed. Test/build execution now
uses one process at a time per Cargo target directory.

The standalone architecture gate retains its pre-existing failure:
`main.rs` has 12,085 lines against 11,750 and 311 functions against 300.
The separate fleet extraction change must land before claiming that gate passes.

Unix permission fixtures detect runners that bypass mode restrictions and emit a
skip message in that case. Deterministic obstruction and cleanup tests run on
all platforms. The development run exercised actual denied writes as a normal
user; it did not skip permission fixtures. Windows execution remains CI evidence.
