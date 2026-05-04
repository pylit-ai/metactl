# Security

metactl is a local tool that materializes agent configuration files. Treat generated files, MCP tool surfaces, hooks, and local-only pack sources as security-sensitive.

## Reporting

Report security issues privately through GitHub private vulnerability reporting or a direct maintainer contact listed by the project. Do not open public issues with exploit details.

## Scope

- Local CLI behavior
- Local JSON-RPC/MCP stdio behavior
- Generated target surfaces
- Public schemas and fixtures

## Expectations

- `metactl` should not require network access for local compile, validation, search, or projection workflows.
- Generated files must be reviewable before they are applied to a repository.
- Machine-specific paths, credentials, local state, and generated agent outputs must not appear in public fixtures or release artifacts.
- Public examples must use neutral placeholders.

See `docs/threat-model.md` and `docs/security-checklist.md` for release checks.
