# Verification ledger

- [x] Reproduce CLI failure classification on the baseline (both regressions failed with `operation_lock_active`).
- [x] Implement typed diagnostics and failed-initialization cleanup.
- [x] Pass focused regression and contention tests (`cargo test -p metactl operation_lock`: seven passed).
- [x] Document recovery and prioritized core follow-ups.
- [x] Run repository gates and disclose the baseline failure below.
- [x] Independent read-only code and documentation review: no blocking findings.
- [x] Open [PR #27](https://github.com/pylit-ai/metactl/pull/27); verify its remote head at handoff.

## Completed checks

- `cargo test --offline --workspace`: 349 passed, zero failed or ignored.
- `cargo check -p metactl -p metactld`: passed.
- `cargo fmt --check` and `git diff --check`: passed.
- `bash scripts/check_public_boundary.sh`: passed.
- `scripts/validate_contracts.py --include-starter-library --include-targets --include-knowledge-fixtures --library-stack-fixtures`: passed with the configured validation Python environment.
- `python3 scripts/verify_docs_links.py`: passed.
- `CARGO_NET_OFFLINE=true bash scripts/smoke_cli.sh`: passed, including installing the release binary and init/compile/apply/validate/revert.
- `scripts/verify_v1_release_gate.py`: passed; its packaged Docker smoke was explicitly skipped because Docker was unavailable.

Permission regression fixtures ran without privilege-based skips on the local
Unix runner. Non-Unix runners omit the mode-bit fixture; typed classification
and injected initialization-failure tests remain portable.

## Baseline release-gate finding

`bash scripts/check_architecture_metrics.sh` fails on the unchanged base:
`main.rs` has 12,066 lines and 311 functions against limits of 11,750 and 300.
This diagnostic patch does not resolve that pre-existing architecture debt.
Do not treat the PR as proof that every release gate is green.

The CLI smoke installer now uses `cargo install --locked` so it verifies the
committed dependency set instead of resolving a newer, untested set.
