# Release reproducibility and maintenance priorities

## Audit baseline

Audited after v0.1.21, source commit
`efa46cbd09d27889467a19d0335d18c72abdc74e`. The release passed its tests,
package-install checks, checksums and GitHub build-provenance verification.
Those establish tested behavior and artifact origin. They do not establish that
a future rebuild will produce identical binary bytes.

The archive command was reproduced with identical payload bytes and different
filesystem modification times. The resulting SHA-256 hashes differed. The
workflow also used its tag as an archive label without comparing that tag to
the crate version, and relied on the runner's native architecture matching its
advertised target.

## Implemented packaging contract

`scripts/package_release.py` preserves the existing archive names and layout:
one version/target directory containing `metactl`, `metactld`, `README.md`,
`LICENSE` and `NOTICE`, plus an adjacent SHA-256 checksum file.

- Tagged builds must match the crate version and the existing version-consistency
  checks. Manual branch builds use the package version, not the branch name.
- Cargo builds the explicit advertised target; packaging reads that target's
  output directory.
- Every payload must be a nonempty regular file. Missing files, directory inputs
  and symlinks fail before an existing archive is replaced.
- Entries have a fixed order, numeric owner/group zero, empty owner/group names,
  and explicit modes (0755 binaries/directory; 0644 documentation).
- Tar timestamps use `SOURCE_DATE_EPOCH`, or the source commit time when unset.
  The gzip header contains no filename and has timestamp zero.
- Same payload bytes, version, target, source epoch and Python/zlib implementation
  produce identical archive bytes regardless of source file modes, modification
  times or output directory. Payload changes change the archive hash.

Run `python3 scripts/test_package_release.py` for positive and negative consumer
checks. `make verify-release-consumers` includes them, so the existing release
gate runs them in pull-request and tagged-build verification.

This changes packaging for future builds. It does not replace existing v0.1.21
assets or move its tag.

## Remaining reproducibility work

These are bounded follow-ups, not claims that bit-identical Rust rebuilds work:

| Priority | Evidence and user risk | Next slice | Acceptance |
| --- | --- | --- | --- |
| Next | Release uses `stable`, `ubuntu-latest`, `macos-latest` and mutable action major tags. A later rebuild can use different compiler, SDK and builder code. | Record the exact compiler, Cargo, OS/SDK, action revisions and lockfile digest with artifacts; choose reviewed pins and an update policy. | Two independent clean builds of one commit compare binaries and report any difference; provenance identifies both builders. |
| Next | `requirements-dev.txt` specifies a version range, and packaged smoke uses a floating Rust container. Validation inputs can change independently of source. | Lock the validation dependency graph and smoke image after testing supported hosts. | A fresh environment uses the recorded dependency/image digests; a controlled update reruns the full gate. |
| Later | Compiler paths, linker/SDK inputs and compression implementation are not fixed by archive normalization. | Investigate binary reproducibility after input recording; apply path remapping or build environment changes only when a two-build comparison demonstrates their need. | Byte comparison and an actionable difference report, including platform-specific limitations. |

Do not represent a signed provenance record as proof of reproducibility. Keep
origin verification, behavior tests and repeat-build comparison as separate
checks. The current audit deliberately adds no new build service or dependency.

## Maintainability: prioritize failure behavior

File size alone is not the next refactoring criterion. The extracted fleet and
instruction modules already provide useful boundaries. The next work should
make partial success and recovery understandable to callers.

| Order | Concrete failure | Ownership and smallest change | Required regression evidence |
| --- | --- | --- | --- |
| Immediate, separate fix | Inline instruction snippets cut at byte 197 can panic when that offset splits a UTF-8 character. | Keep byte-budget truncation inside `library_instruction_format::inline_snippet`, ending at a character boundary. | Real CLI compilation with CJK, emoji and mixed text crossing the cutoff; unchanged ASCII behavior and output byte limit. |
| Next | CLI apply writes history, receipts and the managed-path index after materialization. A bookkeeping failure can report a generic error after user files changed. | Give command orchestration a typed outcome that preserves applied targets and identifies the failed bookkeeping operation. Preflight predictable obstructions; distinguish later failures from early refusal. | Obstruct each bookkeeping destination; verify early refusals leave user files unchanged, injected late failures retain applied outcomes in JSON and text, and retry guidance is safe. |
| Same next slice | Materializer action, state and journal failure paths discard the original I/O error while reporting compensation status. | Carry the triggering operation, path and error alongside the existing rollback outcome, preserving current codes and recovery safeguards. | Inject destination/state/journal failures; original cause survives in command output and receipts; partial compensation and intervening user edits remain correctly reported. |

### Dependency and verification plan

1. Reproduce each bookkeeping/diagnostic failure using real command fixtures.
2. Define the observable result contract before moving code: no write, fully
   applied, or partially applied; original failure; compensation outcome;
   affected targets; next safe action. Preserve existing machine contracts.
3. Implement the narrow shared outcome/error owner. Extract further functions
   only where this eliminates duplicated invariants or ambiguous ownership.
4. Run focused injected-failure tests, full workspace tests, machine-contract
   fixtures and architecture budgets, followed by independent review.
5. If the result needs a contract change, stop integration and revise the public
   contract and migration plan first. Do not hide errors or weaken tests to make
   a structural refactor pass.

This plan is the canonical local record for these findings. The audit found no
matching existing GitHub issue; no remote issue was created. The implemented
archive correction and separate Unicode fix do not claim the remaining outcome
work is complete.
