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

Shared migration requires exact projection bytes in both the working file and
any staged Git blob. Git line-ending conversion or clean filters can change the
staged bytes; such a mismatch refuses migration, including when the working
file has been removed. Review and reconcile the staged representation before
retrying; filtered bytes are not automatically accepted as shared provenance.

Routine sync, compile and apply check the requested private destinations and
their relevant ignore sources. They inspect the cached Git index, including
native filesystem aliases, but do not walk unrelated ignored dependency or
build trees. Ignore migration retains full-worktree enumeration because changing
rules can affect previously hidden content anywhere in the repository. Migration
planning therefore has a broader cost than routine private publication.
If a tracked Unicode path is absent from the working tree and could alias a
private destination, publication refuses until the ambiguity is resolved.

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
digests require recompilation. All older manifests without replay inputs can
only apply to ignored, untracked destinations, with `.metactl/` protected too.
Recompile before applying legacy shared outputs to establish synthesis evidence.
A category or missing or edited attribution field cannot grant sharing.

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

The native ignore probe supports ordinary symlink leaves on Windows using empty
probe files and native Git parity checks. Windows junction directories that Git
actually enumerates are modeled as directories without copying payloads. Their
link and resolved target are rechecked before accepting the proof. A requested
private destination at or beneath a junction, or any proposed ignore write
through one, still refuses, including when the junction is empty. Other symlink
parents remain unsupported. Native Windows tests cover private compile, apply,
repeated sync, ignore install/fix, and Git staging with these supported links.

Worktree `.gitignore` symlinks, including nested ones, are skipped as native Git
skips them; their target contents are not read. A configured global exclude
symlink is followed only to a regular file. Its link selection, resolved target,
and bytes are captured and rechecked. Publication destinations themselves still
must be ordinary files; source-link support does not authorize writing through
them.

The source index is the index Git actually selects, including the normal index,
the temporary index used by `git commit -a`, and an alternate `GIT_INDEX_FILE`.
Source enumerations use that selection; probe commands use an isolated
context. The selected path, resolved context, file type, and contents are
rechecked after enumeration and before accepting the proof. Symlink, reparse, or
nonregular index files refuse. An inherited `GIT_DIR` is accepted only when it
resolves to the repository independently discovered from the project, including
an operational linked worktree. Foreign redirection and unsupported
`GIT_WORK_TREE` or `GIT_COMMON_DIR` overrides refuse.

Inherited inline Git configuration, including `git -c` and the
`GIT_CONFIG_COUNT` family, is resolved in the source repository. The effective
ignore settings are captured for the isolated probe and rechecked before proof
acceptance. Privacy-probe Git command errors do not echo inline configuration
values. Unrelated non-UTF-8 names do not prevent routine private publication;
unsupported names that affect the proof, and full migrations containing such
names, still refuse. These refusals publish no private content; early bootstrap
may create empty directories. Installed Git hooks remain best-effort sync and
do not turn a privacy refusal into a failed Git commit.

Shared diagnostic names come from known target capabilities and the enforced
policy; arbitrary supplied graph strings remain local. MCP policy metadata names
only the policy actually enforced by compilation. Registry-backed compilation
requires a registered target, matching resolution. Compile privacy preflight uses
the requested surface and apply modes; apply preflight validates the selected
stored manifests and their effective modes before publishing any review plan.
