# Pack Visibility

Real user and organization packs are private-by-default in the v1 model. Public OSS material is an explicit exception: `public_example_library` for generic examples and `sanitized_export` for reviewed material derived from private sources.

In the public starter library, packs are shared by default because they are public examples. A pack with `"visibility_scope": "private"` is routed only to local-only target surfaces when those surfaces exist.

A `sanitized_export` must name the source artifact, transform, dropped fields, reviewer-ready diff, original digest, sanitized digest, and export time. It must not silently copy private library content into public repos.

Use this when a pack is useful on one machine but should not appear in committed instruction indexes.

## Example

```json
{
  "kind": "pack",
  "id": "local-only-example",
  "version": "1.0.0",
  "visibility_scope": "private"
}
```

Expected behavior:

- Shared packs can appear in committed generated indexes such as `AGENTS.md`, `CLAUDE.md`, or `GEMINI.md`.
- Local-only packs are excluded from committed generated indexes.
- Targets with local-only surfaces can receive local-only routing files, such as `CLAUDE.local.md`.
- Targets without native local-only surfaces report a degradation instead of leaking local-only pack content into shared surfaces.

The public starter library includes `local-only-example` as a generic visibility fixture.

## Codex Skill Visibility

Codex has two practical skill surfaces:

| Scope | Path | Updated by |
| --- | --- | --- |
| Repo-local | `<repo>/.codex/skills/...` | `metactl sync` and `metactl fleet sync` |
| User-global Personal | `~/.codex/skills/...` | `metactl skills add <skill-path> --scope user` |

Repo-local skills are visible to Codex sessions opened in that repository. A Fleet Sync success means linked repos were updated; it does not mean the active Codex thread's Personal picker source was updated.

Check both surfaces:

```bash
metactl status
metactl skills list --scope repo
metactl skills list --scope user
```

Install an operator-facing repo skill into the user-global Personal source:

```bash
metactl skills add <repo-skill-path> --scope user
```

Replace an existing user-global copy after review:

```bash
metactl skills add <repo-skill-path> --scope user --force
```
