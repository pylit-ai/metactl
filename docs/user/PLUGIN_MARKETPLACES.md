# Plugin Marketplaces

`metactl plugin` exports pack libraries as Codex plugin bundles. Packs remain canonical in the source metactl library; plugins are generated projections for runtime install and discovery.

## Public Starter Export

Public export reads from the bundled starter library by default and includes only shared/public packs.

```bash
metactl plugin export --tier public --target codex-cli --out dist/plugins/codex
metactl plugin verify --tier public --target codex-cli --path dist/plugins/codex
codex plugin marketplace add dist/plugins/codex
```

Expected output:

```text
Project: /path/to/project
Verified Codex plugin marketplace: pass
Bundles: 1
Packs: 3
```

Run the public boundary scanner before publication:

```bash
metactl check-public-boundary
```

No publication or release is implied by export.

## Private Library Export

Private export requires an explicit user-owned library root and writes a local/private marketplace tree. Keep that output local or in a private Git repository unless every included pack has been reviewed for public release.

```bash
metactl plugin export \
  --tier private \
  --library-root /path/to/private-metactl-library \
  --target codex-cli \
  --out /path/to/private-plugin-marketplace

metactl plugin verify \
  --tier private \
  --target codex-cli \
  --path /path/to/private-plugin-marketplace

codex plugin marketplace add /path/to/private-plugin-marketplace
```

Expected output:

```text
Project: /path/to/project
Verified Codex plugin marketplace: pass
Bundles: 1
Packs: 1
```

The generated marketplace root includes `.agents/plugins/marketplace.json` for Codex registration and `plugins/<plugin-name>/` for the projected plugin bundle. The bundle includes `.codex-plugin/plugin.json`, `.codex-plugin/metactl-projection.json`, and `skills/<pack-id>/SKILL.md`. The projection manifest records target runtime, output tier, selected pack ids, source digest, visibility filter, and degraded surfaces.

## Scope

- `public` tier includes shared/public packs.
- `private` tier includes private packs from a user-selected library root.
- Team/collaborator install semantics are out of scope for this workflow.
- Only `codex-cli` plugin layout is implemented in this milestone.
