# metactl v1 Migration Guide

Use this path for existing projects:

```bash
metactl library init --user --profile user
metactl project link --profile user
metactl sync --preview
metactl sync --apply
metactl check --strict
```

Keep private packs in the user library or private source roots. Commit generated projections only when the active profile intentionally allows committed projections and public-boundary gates pass.

From metactlv0-era layouts, remove public/private mode assumptions, avoid automatic membership unions, and replace broad source inheritance with explicit profile-selected baselines plus one writable overlay.
