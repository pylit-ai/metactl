# StrategyBrief: CLI Actionability Recovery UX

Status: active
Mode: brownfield
Date: 2026-05-16
Repo: `pylit-ai/metactl`
Input audit: `docs/audits/2026-05-16-cli-actionability-audit.md`

## Goal

Make recoverable `metactl` CLI failures self-directing: when a command reports a
known bad state, human output should include the exact next command or bounded
file/action needed to recover. JSON output should remain stable and additive.

## Existing Evidence

- `cmd_status` already computes `source_audit_findings` and stores them under
  `source_state["findings"]`, but human status only prints the state label.
  Evidence: `crates/metactl/src/main.rs:4983`.
- `cmd_doctor` already stores source-audit findings in the JSON check object,
  but human doctor renders only `[status] id: message`.
  Evidence: `crates/metactl/src/main.rs:6738` and
  `crates/metactl/src/main.rs:6869`.
- Missing config errors flow through `load_required_context_for_path` into
  `state_error`, so the output currently inherits a low-context filesystem
  message.
  Evidence: `crates/metactl/src/main.rs:4674`.
- `cmd_source_sync` has direct access to configured sources when a requested
  source name is missing, but returns only `Source '<name>' is not configured`.
  Evidence: `crates/metactl/src/main.rs:9554`.
- Missing packs already provide a full pack list, but the list is noisy and does
  not guide discovery.
  Evidence: `crates/metactl/src/main.rs:3807`.
- `source_preflight_error` carries full `source_state` in JSON but uses empty
  human details.
  Evidence: `crates/metactl/src/main.rs:3337`.

## Ownership Boundaries

- Product CLI behavior: `crates/metactl/src/main.rs`
- CLI workflow regression tests: `crates/metactl/tests/cli_workflow.rs`
- User docs: `README.md`, `docs/user/WORKFLOWS.md`, and possibly
  `docs/user/GETTING_STARTED.md`
- Audit source of truth: `docs/audits/2026-05-16-cli-actionability-audit.md`

Do not change public/private boundary checks, release workflows, packaging, or
library pack content in this plan.

## Data Contracts

- Human output may gain extra `next:` and finding lines.
- JSON output must remain backward-compatible. Additive fields are allowed.
- Exit codes must not change.
- Existing commands must remain non-interactive unless already interactive.
- No command should mutate repo state merely to compute a hint.

## Strategies

### Strategy A: Targeted Actionability Pass

Implement focused fixes for the audited gaps:

1. `status`: when `source_state.state == private_source_leak_risk`, print
   `next: metactl audit sources` and the first one to three finding paths.
2. `doctor`: when a check contains `findings`, print top findings and
   `next: metactl audit sources` for `source-audit`.
3. Missing config: wrap missing `metactl.yaml` errors with initialization and
   config-selection hints.
4. `source sync`: list configured sources and next commands on missing source.
5. `source_preflight_error`: derive per-source `metactl source sync <name>`
   details from `source_state`.
6. Missing pack: show nearest matches plus `metactl list packs` /
   `metactl search <term>` while preserving full detail in JSON.
7. Docs: add source-audit recovery flow and private-source ignore posture.

Score:

- User impact: 5
- Implementation risk: 2
- Testability: 5
- Scope control: 5
- Total: 17

### Strategy B: Shared Recovery-Hint Framework First

Create a reusable recovery-hint abstraction, then migrate the audited cases.

Possible shape:

- `RecoveryHint { next_commands, findings, docs }`
- helpers for human rendering and JSON fields
- command-specific adapters for source audit, source preflight, missing config,
  missing source, and missing pack

Score:

- User impact: 5
- Implementation risk: 4
- Testability: 4
- Scope control: 2
- Total: 13

### Strategy C: Docs-First, Code-Later

Document all recovery flows now and leave CLI behavior mostly unchanged.

Score:

- User impact: 2
- Implementation risk: 1
- Testability: 3
- Scope control: 5
- Total: 9

## Recommendation

Choose Strategy A.

Reason: the audited failures already have localized code paths and test
fixtures. A small targeted pass improves real user recovery without introducing
a new abstraction while the CLI behavior is still evolving. Strategy B may be
worth doing later if more commands need the same pattern, but it is premature
for the current backlog.

## Plan Skeptic Objections

- Risk: adding human output lines can break brittle downstream scripts.
  Response: keep JSON stable; only add human lines after existing labels.
  Confirm tests for `--json` outputs where present.
- Risk: nearest-match logic for packs can become overbuilt.
  Response: use simple bounded substring/edit-distance scoring, cap output to
  five suggestions, and keep the full list out of human stderr.
- Risk: missing-config hints might recommend the wrong init mode.
  Response: offer two alternatives: `metactl init --detect` for existing repos
  and `metactl init -t codex-cli` for fresh/single-target projects.
- Risk: source-audit findings can be sensitive.
  Response: print only relative paths and existing redacted messages; do not
  print source URLs, secrets, or raw lock contents.

## Worktree Decomposition

One executor can complete this safely in the public repo. Parallel workers are
not needed because all changes touch `main.rs` and `cli_workflow.rs`.

Optional split if needed:

- Worker 1: `status`, `doctor`, source-preflight rendering, tests.
- Worker 2: missing-config, missing-source, missing-pack, docs.

Avoid concurrent edits to `crates/metactl/src/main.rs` unless the write ranges
are coordinated.

## Milestones

### M1: Source-Audit Status and Doctor

Files:

- `crates/metactl/src/main.rs`
- `crates/metactl/tests/cli_workflow.rs`

Acceptance:

- `metactl status` prints `next: metactl audit sources` when source-audit
  findings exist.
- `metactl doctor` prints top source-audit finding paths and the same next
  command.
- JSON stays additive or unchanged.

Verification:

```bash
cargo fmt --check
cargo test -p metactl cli_status_shows_project_state
cargo test -p metactl doctor_reports_source_audit_failure_for_tracked_private_source_state
```

### M2: Missing Config and Missing Source Recovery

Files:

- `crates/metactl/src/main.rs`
- `crates/metactl/tests/cli_workflow.rs`

Acceptance:

- Fresh directory `sync --preview` / `validate` errors include init/config
  hints.
- `metactl source sync <bad-name>` lists configured sources when present.
- If no sources are configured, output points to source-add/list help.

Verification:

```bash
cargo fmt --check
cargo test -p metactl missing_config_errors_include_init_hints
cargo test -p metactl source_sync_missing_source_lists_recovery_commands
```

### M3: Source Preflight and Missing Pack Guidance

Files:

- `crates/metactl/src/main.rs`
- `crates/metactl/tests/cli_workflow.rs`

Acceptance:

- Stale/unlocked source preflight includes concrete
  `metactl source sync <name>` commands.
- Missing-pack human output shows bounded likely matches and discovery commands.
- JSON retains complete source/pack data for automation.

Verification:

```bash
cargo fmt --check
cargo test -p metactl sync_refuses_stale_git_source_cache_until_source_sync
cargo test -p metactl add_missing_pack_suggests_search_and_nearest_matches
```

### M4: Docs and End-to-End Check

Files:

- `README.md`
- `docs/user/WORKFLOWS.md`
- optional `docs/user/GETTING_STARTED.md`

Acceptance:

- Docs mention `metactl audit sources`.
- Docs mention `metactl ignore status`.
- Docs mention
  `metactl ignore install --scope local --include-private-sources`.
- Docs distinguish local checkout ignores from repo-shared ignore posture.

Verification:

```bash
cargo fmt --check
cargo test -p metactl
bash scripts/check_public_boundary.sh
```

## Migration Risks

- Human-output snapshot tests may need updates.
- Some downstream scripts may parse human stderr. Do not remove existing text.
- Missing-config handling may be shared by many commands; keep changes in the
  conversion from project-context errors to `CliError` rather than deep project
  loading internals unless necessary.
- Docs must avoid private paths and should use neutral examples.

## Stop Conditions

Stop and ask before:

- changing exit codes,
- changing JSON field names or deleting fields,
- adding dependencies for fuzzy matching,
- modifying release workflows,
- publishing or pushing,
- weakening source-audit/privacy checks to make UX easier.

## Final Verification Gate

Before calling the implementation done:

```bash
cargo fmt --check
cargo test -p metactl
bash scripts/check_public_boundary.sh
```

Also run one live repro against a scratch project for missing config and one
against `opendream-private` or a synthetic source-audit fixture to confirm human
output contains the intended `next:` commands.

## Operator Decisions

No operator-only authority is required for implementation or local tests.

Operator decision needed before execution only if the implementer wants to:

- commit,
- push,
- publish,
- modify private sibling repos,
- change source-audit policy semantics rather than output.
