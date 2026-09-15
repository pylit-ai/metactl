# Combined core safety verification

This validation branch combines the production changes proposed in PR #29
(refactoring and architecture gate), PR #28 (apply access preflight), and PR #30
(fleet preview and lock ownership). Land the individual PRs in that order; this
branch exists to make the combined test evidence and fixture reproducible.

## Inputs

- Refactor: `57f6a8d760232aed428ad73bbbf53fa6b8b0d436`.
- Fleet behavior: `bc2c0620e9db1e943642410aadc31f72c4080298`.
- Preflight production changes through `7d2f37ca808b675a22c5fd6922ad979ab8e22521`.

## Results

- `cargo test --offline --workspace`: **372 passed, zero failures**, exit 0.
- `python3 scripts/verify_fleet_apply_preflight.py --binary target/debug/metactl`:
  passed, exit 0. A controller is its own fleet member with two targets. Denied
  access prevents managed files and target state from being applied; correcting
  access permits retry. Full project path/type/mode/byte snapshots match before
  and after both initial and repeated managed previews.
- `bash scripts/smoke_cli.sh`: installed CLI smoke passed, exit 0.
- `cargo check --offline --workspace --all-targets`: passed, exit 0.
- `cargo fmt --check`, architecture budgets, public boundary and documentation
  link verification: passed, exit 0.
- Release gate passed, exit 0. Optional Docker packaging smoke was skipped
  because Docker was unavailable locally; packaging was not changed.
- Independent review found no actionable defects in the final changes.

## Regression found during combination

The first combined run failed the fleet controller symlink-alias test: access
preflight incorrectly rejected a symlink used as the project root. The fix
follows that supported root alias only after existing containment checks reject
symlink traversal beneath the root. A direct CLI root-alias regression was added
to PR #28; the original fleet alias test remains unchanged and now passes.

## Limits

Access probes do not guarantee future access: concurrent changes and restrictions
on a specific filename may still fail during apply. Existing rollback remains.
Fleet preview checks current local inputs and destinations without changing
project files; it does not fetch remote source freshness, reserve files, or
authorize a later apply. Initial bundled-library cache creation outside project
trees can still occur. These tests do not claim exhaustive platform coverage.
