# Decision: Private-By-Default Sources And Explicit Sanitized Export

Status: accepted for v1.

## Context

metactlv0 allowed broad configurability around source and visibility modes. That flexibility made it too easy to confuse canonical private source material, generated project projections, public examples, and publishable output.

v1 chooses a smaller model: the canonical source is the active private library stack, and public sharing is an explicit exception.

## Decision

Real user and organization artifacts are private-by-default.

The canonical source of truth is:

```text
0..N pinned read-only baseline libraries selected by active project/profile
+ exactly one writable overlay per active profile
-> generated project projections
```

Public repository content is limited to code, schemas, fixtures, public docs, public examples, and explicit sanitized exports.

Two exception modes are named:

- `public_example_library`: generic OSS examples and fixtures authored for public use.
- `sanitized_export`: a reviewed export from private source material.

A `sanitized_export` must include:

- source artifact reference;
- sanitizer transform;
- dropped fields;
- reviewer-ready diff;
- original digest;
- sanitized digest;
- export time;
- applied sanitizer identifiers.

Generated project projections are not canonical source. Committed projections in public repos require explicit profile opt-in and public-boundary gates.

## Rejected Alternatives

Per-pack visibility lattice:
Rejected because it recreates the metactlv0 broad configurability failure mode.

Mixed public and private pack trees as normal UX:
Rejected because users need one clear source-of-truth rule.

Public-by-default artifacts:
Rejected because metactl primarily manages agentic operating context, which commonly includes local, organizational, or sensitive workflow assumptions.

Automatic publishing:
Rejected because publish authority, review, and sanitization must stay explicit.

Silent copying of private library content into public repos:
Rejected because project projections are build output and private source markers must fail boundary checks.

## Allowed Public Example Data

Public examples may contain:

- generic roles, packs, policies, targets, and skill skeletons;
- placeholder paths such as `/Users/example/project` and `/home/example/project`;
- fictional organization names marked as examples;
- synthetic provenance and digest values;
- target descriptors that document public agent surfaces;
- no-network fixtures and deterministic golden outputs.

Public examples must not contain secrets, private source markers, internal URLs, private knowledge-source URIs, proprietary local paths, unapproved executable scripts, or names that identify real private deployments.

## Verification

`make verify-public-boundary` runs the public boundary script and a self-test fixture that proves unsafe private export markers are rejected.

`metactl explain` must report whether emitted artifacts came from baseline, overlay, public example, or sanitized export once provenance reporting for these source classes is available.

Boundary failures are release-blocking for public artifacts.
