# Verification ledger

- [x] Reproduce CLI failure classification on the baseline (both regressions failed with `operation_lock_active`).
- [x] Implement typed diagnostics and failed-initialization cleanup.
- [x] Pass focused regression and contention tests (`cargo test -p metactl operation_lock`: seven passed).
- [x] Document recovery and prioritized core follow-ups.
- [ ] Pass repository gates and independent review.
- [ ] Open PR with exact remote head and verification evidence.

## Baseline release-gate finding

`bash scripts/check_architecture_metrics.sh` fails on the unchanged base:
`main.rs` has 12,066 lines and 311 functions against limits of 11,750 and 300.
This diagnostic patch does not resolve that pre-existing architecture debt.
Do not treat the PR as proof that every release gate is green.

The CLI smoke installer now uses `cargo install --locked` so it verifies the
committed dependency set instead of resolving a newer, untested set.
