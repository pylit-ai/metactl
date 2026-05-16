# CLI Actionability Audit

Date: 2026-05-16
Repo: `pylit-ai/metactl`
Baseline commit: `3b8e005ce04899afc2c8cffb8a3106315e4867bb`
CLI version observed: `metactl 0.1.8`

## Summary

The private-source failure path is substantially better after the current changes:
`sync`, `validate`, `audit sources`, and `ignore status` now expose concrete
next commands or file-level remediation. Similar actionability gaps remain in
adjacent status, doctor, missing-config, missing-source, and missing-pack paths.
The most important pattern is consistent: if a command reports a recoverable
state, it should print the next command a normal user should run.

Repo state during audit was already dirty from the source-audit UX changes:

```text
M crates/metactl/src/main.rs
M crates/metactl/tests/cli_workflow.rs
```

No remote writes, publishing, destructive git operations, or production changes
were performed.

## Evidence

### Private-source paths now actionable

`metactl ignore status` against `opendream-private` now prints:

```text
private-sources   not-protected
next: metactl ignore install --scope local --include-private-sources
```

`metactl audit sources` against `opendream-private` now prints file-level
findings and remediation for:

```text
README.md
scripts/apply_public_overlay.sh
scripts/leak_check.sh
scripts/release_public.sh
scripts/validate_public_overlay.sh
```

`metactl sync` now refuses before apply on the same source-audit failure:

```text
Error: Sync refused because private source state may be tracked or exposed.
```

### Remaining compressed surfaces

`metactl status` against `opendream-private` reports:

```text
Source state: private_source_leak_risk
```

It does not show `metactl audit sources` or the first offending file.

`metactl doctor` against `opendream-private` reports:

```text
[FAIL] source-audit: Private source cache or private source lock may be tracked or exposed.
```

It does not show `metactl audit sources`, top findings, or remediation.

### Other recovery-path probes

Fresh project with no `metactl.yaml`:

```text
Error: project config .../metactl.yaml does not exist
```

Missing source:

```text
Error: Source 'missing-source' is not configured.
```

Invalid ignore target:

```text
Error: target bogus does not have generated-agent ignore rules; supported targets: codex-cli, claude-code, cursor, gemini-cli
```

Missing pack:

```text
Error: Pack(s) not found in library: missing-pack
  - Available packs: ...
```

The missing-pack path is technically informative but noisy and not guided.

## Issues

| ID | Issue | Area | Severity | Evidence | Suggested fix |
| --- | --- | --- | ---: | --- | --- |
| UX-001 | `status` reports `private_source_leak_risk` without next command or top finding. | UX/Diagnosability | 85 | `Source state: private_source_leak_risk` | If source-state has findings, print `next: metactl audit sources` and top 1-3 finding paths. |
| UX-002 | `doctor` source-audit failure hides findings in human output. | UX/Diagnosability | 82 | `[FAIL] source-audit: ... may be tracked or exposed.` | Include top source-audit findings and `next: metactl audit sources` in human doctor output. |
| UX-003 | Missing project config error does not tell user how to create or select config. | Onboarding | 72 | `project config .../metactl.yaml does not exist` | Add details: `Run metactl init --detect` or pass `--config PATH`; for fresh projects, suggest `metactl init -t codex-cli`. |
| UX-004 | `source sync <name>` missing-source error does not list configured sources or source-add/list commands. | UX/DevEx | 68 | `Source 'missing-source' is not configured.` | Add details: configured source names, `metactl source list`, and `metactl source add ...`. |
| UX-005 | Private-source stale preflight helper has empty `details`, so strict sync failures can stay generic. | UX/Diagnosability | 66 | `source_preflight_error` constructs `details: Vec::new()` | Populate details from `source_state.missing`, `source_state.unlocked`, and `source_state.stale`; print exact `metactl source sync <name>` commands. |
| UX-006 | Missing-pack error dumps all available packs instead of guiding discovery. | UX/DevEx | 61 | `Available packs: ...` long line | Show nearest matches and `next: metactl list packs` / `metactl search <term>`; keep full list in JSON. |
| UX-007 | Invalid ignore target lacks a correction command. | UX | 45 | supported target list only | Add `next: metactl ignore status` or `next: metactl ignore install --target all`. |
| DOC-001 | Public docs mention `metactl ignore install` but not `--include-private-sources` or `audit sources`. | Docs | 58 | README command table only lists basic ignore install | Add a short troubleshooting row/section for source-audit failures and private-source ignore posture. |

## Backlog

### UX-001: Make `metactl status` source-audit states actionable

Problem: `status` surfaces `private_source_leak_risk` but not the recovery path.

User story: As a user checking repo health, I can run `metactl status` and see
the next command needed to diagnose or fix a failing source state.

Acceptance criteria:

- Given source-audit findings, human `status` prints `next: metactl audit sources`.
- Human `status` includes at least the first failing path.
- JSON status remains backward-compatible and keeps full findings.
- Regression test covers `private_source_leak_risk`.

Priority: P1
Effort: S

### UX-002: Make `metactl doctor` source-audit failures actionable

Problem: human `doctor` reports `[FAIL] source-audit` but hides the specific
findings that JSON already carries.

User story: As a user running doctor, I can see the failing files and the next
command without re-running with `--json`.

Acceptance criteria:

- Human `doctor` includes top source-audit findings under the failing check.
- Human `doctor` includes `next: metactl audit sources`.
- JSON doctor remains unchanged except for optional additive fields.
- Regression test covers a tracked private source state.

Priority: P1
Effort: S

### UX-003: Improve missing-config recovery

Problem: commands that require `metactl.yaml` only state that the file does not
exist.

User story: As a new user or user in the wrong directory, I can recover from a
missing config error without knowing the initialization commands.

Acceptance criteria:

- Missing config errors include `next: metactl init --detect` and
  `next: metactl init -t codex-cli` or equivalent.
- Error mentions `--config PATH` when a custom config was intended.
- Regression test covers `sync --preview` in a fresh temp directory.

Priority: P1
Effort: S

### UX-004: Improve source-sync missing-source recovery

Problem: `metactl source sync missing-source` does not show valid source names
or how to add/list sources.

User story: As a user managing private sources, I can see available source names
and the next source management command when I mistype a source id.

Acceptance criteria:

- Missing-source error lists configured source ids when present.
- Error includes `next: metactl source list`.
- If no sources are configured, error includes `next: metactl source add ...`.
- Regression test covers both no-source and wrong-source cases.

Priority: P2
Effort: S

### UX-005: Add details to stale private-source preflight

Problem: `source_preflight_error` returns no human details even though
`source_state` contains machine-readable state.

User story: As a user blocked by stale source state, I can copy the exact
`metactl source sync <name>` command from the error.

Acceptance criteria:

- Strict source preflight errors include one `metactl source sync <name>` line
  per stale/unlocked source.
- Missing private sources include the relevant config path or source id.
- Regression test covers stale source lock refusal.

Priority: P2
Effort: M

### UX-006: Replace missing-pack dump with guided discovery

Problem: missing-pack errors dump a long list of packs but do not suggest search
or nearest matches.

User story: As a user adding a pack, I can recover from a typo by seeing likely
matches or a search/list command.

Acceptance criteria:

- Human output shows up to five nearest pack names.
- Human output includes `next: metactl list packs` or `metactl search <term>`.
- JSON keeps complete available-pack data for automation.
- Regression test covers a misspelled pack id.

Priority: P2
Effort: M

### DOC-001: Document source-audit recovery flow

Problem: docs do not teach `audit sources` or `--include-private-sources`.

User story: As a user reading docs after a failure, I can find the source-audit
recovery flow with the same commands printed by the CLI.

Acceptance criteria:

- README command table references `metactl audit sources`.
- User workflow docs include `metactl ignore status` and
  `metactl ignore install --scope local --include-private-sources`.
- Docs distinguish local ignore posture from shared repo ignore posture.

Priority: P2
Effort: S

## Quick Wins

1. Add `next: metactl audit sources` to `status` and `doctor`.
2. Add missing-config next steps.
3. Add `audit sources` and `--include-private-sources` to README/user docs.

## Strategic Fixes

1. Create a small shared helper for recoverable CLI errors:
   message, findings, next commands, and JSON payload stay aligned.
2. Standardize human output convention:
   status label, one-line reason, `next:` command, then optional top findings.
3. Add regression tests for every recoverable failure class that currently
   depends on a user knowing a hidden command.
