# metactl v1 Lightweight Control Plane Charter

This charter is the canonical scope source for metactl v1. Code, contracts, docs, tests, release gates, and issue work should optimize for this boundary.

Core posture:

```text
0..N pinned read-only baseline libraries selected by active project/profile
+ exactly one writable overlay per active profile
-> generated project projections
```

metactl v1 is a private-by-default deterministic resolver/compiler/validator for portable agentic standards.

## What metactl is

metactl is:

- a local deterministic resolver/compiler/validator for agentic standards;
- a private-by-default control plane for resolving library stacks into target-native files;
- a way to select 0..N pinned read-only baseline libraries selected by active project/profile plus exactly one writable overlay per active profile;
- a compiler for generated project projections such as root instruction docs, skill folders, target rules, MCP config pointers, and projection lock metadata;
- a validator that explains provenance, target degradations, projection drift, trust tier, freshness, and public-boundary status;
- a CLI and local read-only MCP/JSON-RPC surface over the same deterministic kernel.

## What metactl is not

metactl is not:

- an agent runtime;
- a hosted registry;
- a vector DB, graph DB, memory database, or RAG system;
- an enterprise admin UI or multi-tenant permissions platform;
- a browser automation system;
- a public-by-default publishing workflow;
- an arbitrary inheritance lattice;
- a per-field merge policy engine;
- an automatic union of all teams, orgs, or memberships a user belongs to.

## Vocabulary

Baseline:
A pinned read-only library source selected by the active project/profile. A profile may select 0..N baselines. Baseline order is explicit and deterministic.

Overlay:
The exactly one writable private library for the active profile. User-authored changes land here unless an explicit import or sanitized export flow says otherwise.

Profile:
The selector for baseline list, overlay location, target defaults, projection mode, and policy gates. Profiles prevent consultant or multi-team material from leaking across contexts.

Projection:
Generated project-local artifacts emitted for a target. Projections are build output, not canonical source.

Public example:
Generic OSS content authored or generated from safe fixtures. Public examples may include schemas, fixture packs, target descriptors, docs, and tutorial material that contains no private source markers.

Sanitized export:
An explicit reviewed export from private source material. A sanitized export records source artifact, transform, dropped fields, original digest, sanitized digest, export time, and a reviewer-ready diff.

## Public And Private Boundary

Real user, team, organization, consultant, and customer agentic artifacts are private by default. Public repository content is limited to:

- code, schemas, tests, packaging, and CI required to build and verify metactl;
- public examples and fixtures;
- target descriptors and conformance fixtures that contain no private content;
- documentation needed by outside users;
- explicit sanitized exports.

Project-local files generated from a private library stack are generated project projections. They must not be treated as canonical public content unless the active profile explicitly opts into committed projections and the public-boundary gate passes.

## Anti-Bloat Rule

Every new feature must compile to deterministic resolver/compiler/validator behavior or be deferred. A proposed feature belongs outside v1 if it requires metactl to become an agent runtime, hosted service, registry server, vector database, RAG system, memory writeback platform, enterprise admin UI, arbitrary inheritance system, or per-field merge-policy engine.

The default answer for ambiguous scope is private and local. Public sharing requires a public example or sanitized export.

## Verification Hooks

The charter is enforced by `make verify-v1-charter`, anti-bloat terminology linting, public-boundary checks, and release-gate checklist items. Stale v1 wording must not reintroduce narrower baseline assumptions.
