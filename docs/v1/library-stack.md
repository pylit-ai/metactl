# metactl v1 Library Stack

The v1 stack model is explicit and small:

```text
0..N pinned read-only baseline libraries selected by active project/profile
+ exactly one writable overlay per active profile
-> generated project projections
```

Profiles select baselines and the overlay by ID. Membership, installed tools, or organization accounts do not activate libraries automatically. Resolution is artifact-level only: metactl does not deep-merge arbitrary YAML, JSON, or Markdown bodies.

Conflict rules:

- Distinct artifact IDs are unioned.
- A locked baseline artifact cannot be overridden by the overlay.
- An overlay may win only when the baseline artifact explicitly declares `override_policy: allow_overlay`.
- A duplicate artifact without explicit policy fails as an accidental collision.
- Baseline-to-baseline duplicates require an explicit profile precedence policy or they fail.

The lock records the source library ID, source role, source digest, artifact digest, artifact ref, override status, and generated paths for every resolved artifact.
