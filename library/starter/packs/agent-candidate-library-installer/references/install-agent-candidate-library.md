# Install an Agent Candidate Library

This runbook installs a private or local agent-authored candidate library into a metactl-managed project.

## What This Sets Up

- candidate-library checkout
- candidate-library pre-commit verifier hook
- private metactl source registration
- adapter sync for agent tools

The hook usually belongs in the candidate library itself. Ordinary project repos only need the metactl source and sync unless they also store candidate drafts.

## Prerequisites

- metactl CLI installed
- Git access to a private candidate-library repo, or permission to create one
- a metactl-managed project
- a verifier command for the candidate library

## Install

```sh
AGENT_LIBRARY_URL="git@github.com:ORG/metactl-agent-library.git"
AGENT_LIBRARY_PATH="/path/to/metactl-agent-library"
PROJECT="/path/to/project"
SOURCE_NAME="agent-candidates"
STARTER_PACK="/path/to/metactl/library/starter/packs/agent-candidate-library-installer"

git clone "$AGENT_LIBRARY_URL" "$AGENT_LIBRARY_PATH"

cd "$AGENT_LIBRARY_PATH"
python3 "$STARTER_PACK/scripts/install_candidate_library_hook.py" \
  --repo "$AGENT_LIBRARY_PATH" \
  --verify-command "python3 scripts/verify_candidates.py"
python3 scripts/verify_candidates.py

cd "$PROJECT"
metactl source add "$SOURCE_NAME" "$AGENT_LIBRARY_PATH" \
  --type local \
  --private \
  --lock-publicity private \
  --no-input
metactl source sync "$SOURCE_NAME" --no-input
metactl add agent-candidate-library-installer --sync
metactl sync --preview --require-private-sources
metactl sync --adopt patch --no-input --require-private-sources
```

If the source already exists, run:

```sh
cd "$PROJECT"
metactl source sync "$SOURCE_NAME" --no-input
metactl sync --preview --require-private-sources
metactl sync --adopt patch --no-input --require-private-sources
```

## Hook Installer

The included installer writes `.git/hooks/pre-commit` in the selected repo and makes it executable.

```sh
python3 "$STARTER_PACK/scripts/install_candidate_library_hook.py" \
  --repo "$AGENT_LIBRARY_PATH" \
  --verify-command "python3 scripts/verify_candidates.py"
```

Use `--force` only when replacing a known, reviewed existing pre-commit hook.

To install the same hook in another repo that stores candidate drafts:

```sh
DRAFT_REPO="/path/to/repo-that-stores-candidate-drafts"
python3 "$STARTER_PACK/scripts/install_candidate_library_hook.py" \
  --repo "$DRAFT_REPO" \
  --verify-command "python3 /path/to/metactl-agent-library/scripts/verify_candidates.py"
```

## Fleet Rollout

Use fleet rollout only when the user wants multiple projects updated.

```sh
metactl fleet status
metactl fleet sync --preview
metactl fleet sync --apply --yes --no-input
```

Review dirty projects before applying. Do not force adoption across ambiguous local changes.

## Verify

```sh
cd "$AGENT_LIBRARY_PATH"
git status --short
test -x .git/hooks/pre-commit
python3 scripts/verify_candidates.py

cd "$PROJECT"
git status --short
metactl source sync "$SOURCE_NAME" --no-input
metactl sync --preview --require-private-sources
rg "agent-candidate-library-installer" .codex .agents .claude .cursor .gemini .metactl 2>/dev/null
```

Expected result:

- candidate verifier passes
- pre-commit hook exists and is executable in the candidate library
- metactl private source syncs
- sync preview is understood before apply
- generated adapter output includes the installer skill after apply

## Access Model

This pattern does not depend on a specific organization repository.

- Shared private repo: best for a team with common candidate artifacts.
- Per-user private fork: best when people should not share experiments by default.
- Local-only checkout: acceptable for experimentation, but weaker for review, rollback, and provenance.

The curated library remains owner-managed. Candidate artifacts stay quarantined until a human-approved promotion step copies reviewed content into a curated library.
