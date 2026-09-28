# Discovery in unconfigured folders

Status: proposed design, not implemented. PR #57 fixes missing-configuration
diagnostics; it does not provide a catalog for an unconfigured folder.

## Decision

Provide a persistent user skill catalog for folders without project configuration.
Keep three decisions separate: which workspace is active, which catalog supplies
skills, and whether that workspace may send the task and candidate descriptions
to Jev. Never borrow another project's identity to make discovery work.

Configured projects retain their current behavior. A missing project config can
use an explicitly selected user catalog. An invalid project config remains an
error. User-catalog selection does not initialize projects, enroll roots, change
gateway limits, or suppress the agent's native skill menu.

## Intended workflows

| User | Expected workflow |
| --- | --- |
| First-time user | Run one guided setup, select a trusted local library and agent, preview visibility and provider settings, then save once. No manual agent-config editing. |
| Experienced user | Reuse a named catalog, preview/apply a user-wide connection, inspect effective settings, and override or disable discovery per project. |
| Coding agent | Call discovery when specialist guidance is useful, show one compact receipt, then load a returned ID and digest. Continue with local guidance on setup/provider failure. |
| Existing enrolled project | Keep saved advisory Jev preferences and exact workspace authorization; origin alone grants nothing, while the selected metadata's classification can require local ranking. |
| New unconfigured folder | Use the user catalog locally. Jev runs only if the resolved workspace identity and candidate metadata already satisfy the provider grant. Otherwise report the precise local-only reason. |

Proposed command syntax below is illustrative and is not available today:

```sh
# Proposed guided, persistent setup; preview before writing.
metactl skills setup --scope user --target codex-cli

# Proposed explicit connection mode; apply after preview.
metactl skills connect --scope user --target codex-cli \
  --workspace-mode exact --catalog-mode project-or-user --use-preferences

# Proposed: diagnose the effective catalog and provider decision, with no call.
metactl --project /absolute/workspace skills doctor --target codex-cli \
  --catalog-mode project-or-user --json
```

Setup should show: selected library, agent target, eligible skill count, visibility
across workspaces, provider data class, existing Jev preference, and how to turn
it off. Save these as product preferences, not a per-task approval prompt. Catalog
setup and provider enrollment remain separate actions with distinct receipts.
The single setup save covers catalog, connection and metadata classification;
it never creates workspace enrollment.

The generated agent instruction stays small: discover when useful, load by ID and
digest, display the receipt, and continue locally when unavailable. Put changing
settings and limits in structured configuration, not AGENTS.md.

## Workspace identity

Resolve identity once and use the same value for catalog lookup, preferences,
gateway directory binding, logs, and load verification:

```text
WorkspaceIdentity { canonical_root, resolution_method: exact | git_worktree }
CatalogContext { origin: project | user, catalog_digest, metadata_policy }
ProviderDecision { allowed, reason, workspace_identity, catalog_digest,
                   gateway_project, data_class }
```

An explicit `--project` selects the exact canonical root and takes precedence
over a connection's `git_worktree` mode; doctor reports that choice. Preserve current public
CLI exact-root behavior by default. An explicit `git_worktree` connection mode
may normalize a launch directory to its containing Git worktree, preserving the
existing external launcher's behavior without divergent Python/Rust heuristics.
It must respect nested repositories and nested project configurations; ambiguity
requires an explicit root, not an enrolled ancestor. The shared Git directory is
never a workspace identity. New worktrees do not inherit sibling enrollment.

Connection migration must preserve the existing identity mode. In particular,
an existing Git-normalizing launcher migrates to explicit `git_worktree`, not
silently to the new-connection `exact` default. Show the resolved root and mode
in doctor so a nested-folder launch can be distinguished from its parent project.

Each target adapter must prove what launch directory it receives. If the runtime
cannot supply reliable workspace identity, use a fixed explicit root and report
that limitation. Do not infer identity from transcripts or discovery queries.

## Catalog precedence

| Condition | Result |
| --- | --- |
| Explicit `--config` | Load exactly that config; any failure remains an error. |
| Project config exists | Existing project/profile/local-override behavior; no catalog union. An empty eligible project catalog stays empty. |
| Project config malformed, inaccessible, dangling or unsupported | Sanitized error; no user fallback. Distinguish absence from metadata/read errors. |
| Project config absent; user fallback enabled and catalog valid | User catalog under the actual workspace identity. |
| Project config absent; explicit project profile/overlay flags supplied | Explain conflict/setup error; do not silently discard flags. |
| User fallback disabled or unconfigured | Existing actionable missing-config receipt; no automatic project creation. |
| Declared user source invalid/missing | Explain user-catalog failure; do not substitute bundled or another project's skills. |

Add a discovery-only resolver. Do not feed a user catalog through the ordinary
project loader as a fabricated project: that would import unrelated locks,
overrides, profile defaults and output paths. Compilation remains unchanged.

Store a versioned user catalog beside, but separate from, provider preferences.
Fields: explicit ordered local sources, role, policy, permitted targets,
exclusions, fallback-enabled flag and candidate-metadata classification. Resolve
relative sources against this file. Reuse existing registry/source validation.
Do not automatically scan skill directories, download libraries, or harvest an
enrolled project's profile. Offer the bundled starter catalog as an explicit
choice. Preview eligible counts and visibility before saving source changes.

## Provider behavior and privacy

Catalog origin and provider authority are independent. For either origin, Jev is
allowed only when saved preferences are enabled, the actual canonical workspace
has valid exact enrollment, query and candidate metadata fit the approved data
class, gateway directory/project binding succeeds, and existing budgets permit.
Recheck preferences per request; never authorize via a cached discovery result.

A new user catalog defaults to local-only until its candidate-metadata policy is
configured. Setup offers public, private-owned, or local-only classification for
operator-selected sources, explaining that descriptions may leave the device.
Existing project permission never authorizes secret or third-party-restricted
metadata. Unknown/mixed user-catalog classification stays local. Configured-project
metadata retains its current grant evaluation under the unchanged-behavior
contract. Adding a source previews its
visibility and classification; ordinary content edits do not cause a per-task
prompt. Validate the exact candidate snapshot against the saved policy when
building a provider request. Policy changes invalidate cached provider decisions.

If an already enrolled root has no project config and the chosen user catalog's
metadata fits its existing grant, advisory Jev remains available by default.
Unknown roots use local deterministic ranking with `project_not_enrolled`.
No synthetic global project, broad ancestor grant, direct-provider environment
bypass, or automatic enrollment is introduced. Keep existing bounded calls,
opt-out controls, and deterministic fallback.

Returning skill text to the coding agent itself also exposes that text to the
agent's runtime. The user must explicitly choose sources suitable for visibility
across their workspaces, independently of Jev permission.

## Target, load and observability contracts

For user-origin catalogs, pass a concrete target into Rust catalog selection;
Python's runtime label alone does not filter compatible skills. Unknown or
mismatched user-catalog targets fail explicitly. Configured projects retain
their existing effective-config target semantics; changing those semantics is
a separate compatibility change, outside this design.
Reuse all existing provenance, role, target, policy, exclusion, resource-boundary,
manual-only/disabled, approval, and digest checks for discovery and direct load.

Bind a host to workspace and catalog origin/config identity. If project config
appears/disappears and would change origin, report `catalog_context_changed` and
require a fresh host. Revalidate source manifests and package digests within the
same origin on every operation. Never load a discovered ID from a replacement
catalog silently.

Doctor/status separates catalog readiness from project readiness and provider
readiness. Proposed fields: `catalog_origin`, `catalog_ready`,
`project_config_state`, `effective_target`, `eligible_skills`,
`workspace_resolution`, `provider_effective_reason` and opaque context identity.
Local administrative output may show paths; routine agent receipts/logs use
opaque identity, origin, digest, target, provider attempts/calls and logging state.

Example proposed receipt:
`catalog=user; mode=baseline; reason=project_not_enrolled; provider_calls=0; log=recorded`.
An enrolled, authorized user-catalog request can instead report advisory and its
actual provider result. Configuration failure, no matches, provider fallback and
successful local discovery remain distinct. Preserve PR #57 error accounting;
version readers with event changes. Do not log queries, bodies, private paths or
raw CLI errors. Status checks remain provider-free and unlogged.

## Implementation and acceptance

| Existing seam | Proposed responsibility |
| --- | --- |
| `cli_skills.rs::cmd_skill_discovery` | Shared discovery-only resolver for catalog/discover/load. |
| `project.rs` context/profile loading | Preserve project behavior; reuse registry validation without fake projects. |
| `skill_discovery.rs`, `library_registry.rs` | Apply verified target and existing eligibility/digest checks. |
| `skill_discovery_host.py`, preferences resolver | Carry identity/origin; authorize same candidate snapshot; recheck preferences. |
| `cli_skill_discovery_connection.rs` | Managed workspace/catalog modes, preview/apply/replace/rollback and doctor. |
| Embedded assets and trial readers | Byte parity, new receipt fields, explicit compatible reader behavior. |

Required tests:

1. Configured project IDs/digests/provider decisions unchanged, including profiles.
2. Missing project + user catalog discovers and loads original bytes without
   creating files in the working folder.
3. Missing user setup, invalid project, missing explicit config and empty project
   produce their distinct results; none silently substitutes a catalog.
4. Enrolled unconfigured workspace with authorized metadata preserves advisory;
   unknown/revoked workspace and unauthorized/new metadata make zero provider calls.
5. Direct-provider flags/environment cannot bypass either permission decision.
6. Explicit `--project` wins over connection modes. Nested directories/repos,
   nested configs, worktrees and symlink aliases obey
   the selected identity mode, without ancestor/sibling enrollment inheritance.
7. Origin changes, stale digests, resource escapes and invalid sources fail safely
   for discovery and direct load. New target-mismatch cases apply to user origin only.
8. Connection migration/removal preserves unrelated configuration and the prior
   identity mode, including Git-normalizing launchers, and rolls back.
9. Unwritable/full/missing log paths report truthful state without payload leaks;
   setup errors do not count as successful discovery or task outcomes.
10. Fresh native client sessions in two unrelated folders prove actual workspace
    identity, discovery and load. Verify each target before advertising CWD mode.

Build order: resolver/offline fixtures; host and permission tests with fake
gateways; connector migration; native-client acceptance; opt-in release. Keep
fallback opt-in until target identity and source-visibility checks pass. Rollback
restores the prior user catalog/connection, retaining provider preferences and
logs. Restored metadata policy invalidates cached provider decisions just like
any other policy change. This design makes discovery available; it does not claim token savings,
native catalog suppression, or improved coding outcomes.
