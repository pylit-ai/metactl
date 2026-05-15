---
name: agent-candidate-library-installer
description: Use when installing, updating, or verifying a private or local agent-authored candidate library in a metactl-managed project, including source registration, pre-commit hook installation, sync, and evidence capture.
---

# Agent Candidate Library Installer

## Purpose

Set up a separate agent-authored candidate library without letting agents write directly into a curated library or activate experimental packs by default.

Use this skill when a user asks to:

- install an agent candidate library for a metactl-managed project
- make a candidate-library pre-commit hook easy to install
- register a private or local candidate source with metactl
- verify that generated agent surfaces can see the setup instructions
- roll the setup across a metactl fleet

Do not use this skill to:

- promote candidate artifacts into a curated library
- enable candidate packs globally by default
- require access to a specific private repository
- install hooks into unrelated repositories without user intent

## Inputs

Collect or infer:

- target project path
- candidate library Git URL or local path
- candidate library checkout path
- source name, default `agent-candidates`
- verifier command for the candidate library
- whether this is one project or a fleet rollout

Use placeholders in reusable docs and commands. Avoid embedding machine-local paths unless the user explicitly asked for local setup.

## Recommended Model

Keep three surfaces separate:

- curated library: reviewed, owner-managed packs intended for normal use
- candidate library: private or local source for agent-authored drafts
- target project: metactl-managed repo that consumes selected packs and generated agent surfaces

Candidate packs should stay quarantined until a human-approved promotion step moves reviewed material into a curated library.

## Workflow

1. Choose or create the candidate library.
   - Shared private Git repo: best for a team with common candidate artifacts.
   - Per-user private fork: best when experiments should not be shared by default.
   - Local-only checkout: acceptable for short experiments, but weaker for review and rollback.

2. Clone or initialize the candidate library.

   ```sh
   AGENT_LIBRARY_URL="git@github.com:ORG/metactl-agent-library.git"
   AGENT_LIBRARY_PATH="/path/to/metactl-agent-library"

   git clone "$AGENT_LIBRARY_URL" "$AGENT_LIBRARY_PATH"
   cd "$AGENT_LIBRARY_PATH"
   git pull --ff-only
   ```

   If no shared repo exists yet:

   ```sh
   AGENT_LIBRARY_PATH="/path/to/metactl-agent-library"
   mkdir -p "$AGENT_LIBRARY_PATH"/{imports,decisions,scripts}
   cd "$AGENT_LIBRARY_PATH"
   git init
   ```

3. Identify the verifier command.
   - Prefer the candidate library's own verifier, for example `python3 scripts/verify_candidates.py`.
   - If no verifier exists yet, create one before installing a blocking hook.
   - The hook installer can install any command the user chooses; it does not define candidate policy by itself.

4. Install the candidate-library pre-commit hook.

   ```sh
   STARTER_PACK="/path/to/metactl/library/starter/packs/agent-candidate-library-installer"
   python3 "$STARTER_PACK/scripts/install_candidate_library_hook.py" \
     --repo "$AGENT_LIBRARY_PATH" \
     --verify-command "python3 scripts/verify_candidates.py"
   ```

   Install this hook in another repo only when that repo stores candidate drafts:

   ```sh
   DRAFT_REPO="/path/to/repo-that-stores-candidate-drafts"
   python3 "$STARTER_PACK/scripts/install_candidate_library_hook.py" \
     --repo "$DRAFT_REPO" \
     --verify-command "python3 /path/to/metactl-agent-library/scripts/verify_candidates.py"
   ```

   Use `--force` only when replacing a known, reviewed existing pre-commit hook.

5. Register the source in the target project.

   ```sh
   PROJECT="/path/to/project"
   SOURCE_NAME="agent-candidates"

   cd "$PROJECT"
   metactl source add "$SOURCE_NAME" "$AGENT_LIBRARY_PATH" \
     --type local \
     --private \
     --lock-publicity private \
     --no-input
   metactl source sync "$SOURCE_NAME" --no-input
   ```

   For Git-backed sources, prefer a pinned ref:

   ```sh
   metactl source add "$SOURCE_NAME" "$AGENT_LIBRARY_URL" \
     --type git \
     --ref "<tag-or-commit>" \
     --private \
     --lock-publicity private \
     --no-input
   ```

   If the source already exists, sync it instead of creating a duplicate.

6. Add this setup pack to the project when needed.

   ```sh
   cd "$PROJECT"
   metactl add agent-candidate-library-installer --sync
   ```

   Do not enable candidate packs as default active packs until they have passed review.
   Do not hand-edit global root-agent files such as `~/.codex/AGENTS.md` or `~/.claude/CLAUDE.md` for this setup. Metactl-managed projects should expose this workflow through generated project managed blocks and generated skill surfaces.

7. Preview and apply sync.

   ```sh
   cd "$PROJECT"
   metactl sync --preview --require-private-sources
   metactl sync --adopt patch --no-input --require-private-sources
   ```

   After sync, project root instructions should contain a `metactl:begin agents-md` managed block that points to `agent-candidate-library-installer`, and generated adapter trees should contain the corresponding skill. On a different machine, repeat the clone/source registration/sync steps with that machine's checkout path; no manual root-agent edits are required.

8. Roll out to a fleet only when requested.

   ```sh
   metactl fleet status
   metactl fleet sync --preview
   metactl fleet sync --apply --yes --no-input
   ```

   Do not apply fleet-wide changes across dirty or ambiguous projects without explicit user approval and a status record.

## Verification

Capture evidence for:

- candidate library checkout path and remote
- `git status --short` for the candidate library and target project
- verifier command output
- hook installation path, usually `.git/hooks/pre-commit`
- `metactl source sync <source-name> --no-input`
- `metactl sync --preview --require-private-sources`
- generated adapter search for `agent-candidate-library-installer`
- project root managed-block search for `pack:agent-candidate-library-installer`

## Output

Report:

- access mode: shared private repo, per-user private repo, or local-only checkout
- library URL and checkout path
- target project path
- hook status
- source registration status
- sync status
- verification commands and results
- residual risks or operator-only follow-up

## Reference

See `references/install-agent-candidate-library.md` for a compact installer runbook and hook details.
