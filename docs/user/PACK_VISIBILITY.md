# Pack Visibility

Packs are shared by default. A pack with `"visibility_scope": "private"` is routed only to local-only target surfaces when those surfaces exist.

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
