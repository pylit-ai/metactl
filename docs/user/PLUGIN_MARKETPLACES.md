# Plugin Marketplaces

`metactl plugin` exports pack libraries as runtime plugin bundles. Packs remain canonical in the source metactl library; plugins are generated projections for runtime install and discovery.

## Public Starter Export

Public export reads from the bundled starter library by default and includes only shared/public packs.

```bash
metactl plugin export --tier public --target codex-cli --out dist/plugins/codex
metactl plugin verify --tier public --target codex-cli --path dist/plugins/codex
codex plugin marketplace add dist/plugins/codex

metactl plugin export --tier public --target claude-code --out dist/plugins/claude
metactl plugin verify --tier public --target claude-code --path dist/plugins/claude
claude plugin validate dist/plugins/claude
claude plugin marketplace add dist/plugins/claude
```

Expected output:

```text
Project: /path/to/project
Verified Codex plugin marketplace: pass
Bundles: 1
Packs: 3

Project: /path/to/project
Verified Claude Code plugin marketplace: pass
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
  --target claude-code \
  --out /path/to/private-claude-plugin-marketplace

metactl plugin verify \
  --tier private \
  --target claude-code \
  --path /path/to/private-claude-plugin-marketplace

claude plugin validate /path/to/private-claude-plugin-marketplace
claude plugin marketplace add /path/to/private-claude-plugin-marketplace
```

Expected output:

```text
Project: /path/to/project
Verified Claude Code plugin marketplace: pass
Bundles: 1
Packs: 1
```

For Codex, the generated marketplace root includes `.agents/plugins/marketplace.json`; the bundle includes `.codex-plugin/plugin.json`, `.codex-plugin/metactl-projection.json`, and `skills/<pack-id>/SKILL.md`.

For Claude Code, the generated marketplace root includes `.claude-plugin/marketplace.json`; the bundle includes `.claude-plugin/plugin.json`, `.metactl/plugin-projection.json`, and `skills/<pack-id>/SKILL.md`.

Both projection manifests record target runtime, output tier, selected pack ids, source digest, visibility filter, and degraded surfaces.

## Scope

- `public` tier includes shared/public packs.
- `private` tier includes private packs from a user-selected library root.
- Team/collaborator install semantics are out of scope for this workflow.
- `codex-cli` and `claude-code` plugin layouts are implemented.
