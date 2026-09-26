---
name: skill-discovery
description: Find relevant specialist instructions in the configured project library when a task or phase needs them.
---

# Specialist discovery

Keep required project governance active. When a new task or phase needs specialist
instructions, use `discover_skills` if the optional MCP adapter is registered.
Otherwise run `metactl skills host --use-preferences --call-tool discover_skills`,
sending a JSON object with a minimal `query` on stdin. Saved project preferences
control Jev; absent permission uses local discovery. Never put credentials or
private task details in argv. Exclude secrets and unnecessary task context.
If Python or the optional host is unavailable, use the provider-free
`metactl --json skills discover --query-stdin` with the query on stdin. This
fallback has no routing receipt or discovery ledger; report that limitation.

Use returned names/descriptions to choose relevant IDs. Load original instructions
with `load_skill(id, digest)` or `metactl --json skills load ID --digest DIGEST`.
When the MCP tool is called, show its `routing_receipt` in the task trace or
progress report. If discovery was skipped, say so when reporting routing; never
infer Jev use from registration or a readiness check. Report `log=failed` as
missing measurement, not a successful recorded event. The preference-aware CLI
also returns a receipt and writes the private event log shown by host status.
Check `metactl skills preferences` for effective policy. Do not change permission
or enroll a project without operator authorization; normal calls inherit it.
Multiple skills can apply. Exact names can be searched. No candidates means a
search miss, not proof no skill exists; reformulate once or use ordinary discovery.
Never repeatedly retry a failed optional provider.

Read original instructions and their required references before using a skill.
The loader returns its base directory and declared resources; read only resources
permitted by existing host policy. Loading instructions grants no permissions and
does not reproduce native hooks, tools, prerequisites or subagent behavior.
Never bypass disabled, manual-only or approval-required skills via file reads.

Reuse delivered instructions while relevant. After compaction or changed source,
verify they are still available/current; do not assume a historic loaded flag is
sufficient. No model-provider access is required. Jev, if configured by the host,
is advisory ordering only and cannot activate skills or approve actions.
