# Private projection protection

## Contract

Before publishing private projections, every destination must be effectively
ignored by native Git and absent from the index. Ignore migration evaluates the
complete proposed rules, retaining authored prefixes, suffixes and negations.
Reserved local names are candidates for this check, never exemptions.

An unknown path losing protection requires affirmative shared synthesis evidence
and matching installed bytes. Shared aggregate outputs and materializer-owned
links can migrate. Protected private skills and their resources remain supported.
No private payloads enter the isolated Git probe repository.

## Verification sequence

1. Capture effective Git excludes, nested ignore files, path shapes and index.
2. Prove original-state parity between native source Git and the isolated probe.
3. Evaluate all proposed ignore writes together using native Git.
4. Check destination privacy and shared byte provenance before persistent writes.
5. Recheck private publication before compile staging and apply.

Proof covers the captured state. Later external changes to rules invalidate it;
no filesystem transaction with unrelated editors or Git processes is promised.

Regression coverage includes preserved negations, native Git parity, shared
migrations, private resources, runtime contributions and altered manifests.
Windows CI additionally exercises native replacement and rollback. Exact-release
test counts and old-binary upgrade evidence belong in the release verification
record, rather than serving as permanent claims about later source revisions.

## Runtime aggregates

Native runtime templates retain all active hook contributions. When full output
differs from the shared-only output, the entire native destination is private
and must be ignored and untracked before compilation or apply. A private pack
with no native contribution does not make an otherwise shared runtime private.
MCP active-pack metadata retains private selections under the same protection.

Compile manifests record resolution inputs. Apply replays synthesis against the
current library, checks visibility against actual pack manifests, and compares
staged paths and byte digests before accepting shared output. Changed inputs or
digests require recompilation. Older aggregate manifests without replay inputs
can only apply to ignored, untracked destinations; recompile to establish shared
evidence. A missing or edited attribution field cannot grant sharing.

Private selection uses `metactl use PACK --local`. Ignore rules belong in the
local Git exclude file when their paths reveal private identifiers. A tracked
mixed aggregate refuses publication; inspect and remove it from tracking while
retaining the working file before compiling again. No hook executes during
compilation, apply, or the preservation tests.

Saved Auto selections are local metadata even after a pack is deselected or
unavailable on another target. Shared instruction diagnostics report a generic
`auto_surface_selection` warning without surface identifiers. Detailed selection
IDs remain in protected local state, whose `.metactl/` directory must remain
ignored and untracked. This applies to selected, pinned, and blocked surfaces.

Requested and suppressed packs remain privacy-relevant even when no private
projection is emitted. Before compilation, apply, or publication of an apply
review plan, every persisted graph reference must be known to the current
registry as shared. Unknown packs, incompatible private packs, and private packs
withheld by role or policy therefore still require `.metactl/` to be ignored and
untracked. Free-text diagnostics and provenance are accepted as shared only when
rederived from known shared library metadata; opaque values require protection.
CLI preflight uses the command's actual role, policy, and target overrides.

The native ignore probe refuses symlink-containing repositories on Windows and
non-UTF-8 filenames on Linux. It also refuses `GIT_DIR`, `GIT_INDEX_FILE`, and
inline Git configuration overrides, including hook environments that supply
these variables. These conservative refusals publish no private content; early
bootstrap may create empty directories. Native Windows private copy workflows
are covered separately from this unsupported symlink case.
