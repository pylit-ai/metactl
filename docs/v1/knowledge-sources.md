# metactl v1 Knowledge Sources

Knowledge sources are optional, bounded references that packs and skills may cite without making metactl a RAG system, memory database, hosted wiki, or vector store. The manifest declares where references may come from, how much content may be returned, and how freshness is reported.

Supported v1 source kinds:

- `filesystem_markdown`: repo- or library-relative Markdown under an allowed prefix.
- `llms_txt_index`: a bounded `llms.txt` index with optional static fallback metadata.
- `mcp_resource`: read-only MCP resource pointers with static fallback refs when a target cannot use MCP.

Every source declares a URI scheme, byte budget, allowed targets, trust tier, freshness policy, owner, verification time, and source digest. Search and read operations are bounded. Mutation is not allowed; `propose_update` may only create a draft, pull request, or request for review.

Filesystem adapters must reject unsupported schemes, absolute paths, path traversal, and URI prefixes that escape the declared source root. MCP adapters are optional and may degrade to static pointers for targets without MCP support.

## Freshness

`metactl check --strict` reports KnowledgeSource freshness. Expired sources with `freshness_policy: fail` are validation failures. Expired sources with `warn` remain machine-readable warnings, and `ignore` must be explicit in the manifest.
