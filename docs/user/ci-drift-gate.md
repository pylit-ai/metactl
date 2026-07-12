# CI Drift Gate

Use the `metactl-check` action to fail a pull request when generated agent surfaces drift from the repository's metactl configuration. The action installs a release archive only after its SHA-256 checksum matches the release checksum; if an archive is unavailable for the runner, it falls back to `cargo install --locked`.

Copy [.github/workflows/self-drift-gate.yml.example](../../.github/workflows/self-drift-gate.yml.example) into your repository as `.github/workflows/metactl-drift-gate.yml`:

```yaml
name: metactl drift gate
on: [pull_request]
permissions:
  contents: read
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: pylit-ai/metactl@v0
        with:
          project-path: .
          args: check --agent
```

The default `args` value is `check --agent`. `--agent` means non-interactive JSON output with stable recoverable-error fields. The action places the JSON envelope's `next_commands` (suggested repair commands) in the GitHub job summary and preserves a nonzero metactl exit code as a failed job.

Add this badge to a README after the workflow exists on the default branch:

```markdown
[![metactl drift gate](https://github.com/OWNER/REPOSITORY/actions/workflows/metactl-drift-gate.yml/badge.svg)](https://github.com/OWNER/REPOSITORY/actions/workflows/metactl-drift-gate.yml)
```

`pylit-ai/metactl@v0` requires a published `v0` major tag and a GitHub release containing the checksummed platform archives. Keep `v0` pointed at the compatible stable release line before adopting the floating major tag.
