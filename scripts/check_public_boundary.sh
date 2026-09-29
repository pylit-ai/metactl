#!/usr/bin/env bash
set -euo pipefail

if ! command -v rg >/dev/null 2>&1; then
  echo "Public boundary scan unavailable: required scanner rg (ripgrep) is missing." >&2
  exit 2
fi

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

blocked_paths='^(AGENTS\.md|CLAUDE\.md|GEMINI\.md|CLAUDE\.local\.md|GEMINI\.local\.md|PRD\.md|NORTHSTAR\.md|CONSTITUTION\.md|CURRENT_STATE\.md|APPLY\.md|MIGRATION_GUARDRAILS\.md|metactl\.yaml|metactl\.lock\.json|\.claudeignore|\.codexignore|\.cursorignore|\.geminiignore|\.mcp\.json|opencode\.json|\.metactl/|\.agents/|\.codex/|\.claude/|\.cursor/|\.gemini/|\.omc/|\.opendream/|\.ruler/|\.meta/|(.*/)?memory/|(.*/)?notepads/|(.*/)?scratch/|specs/|openspec/|skills/|docs/adr/|docs/spec/|docs/agents/|docs/governance/|docs/evidence/|docs/status/|docs/superpowers/|docs/repomix-bundles/|[^/]+-library/|.*\.code-workspace$|.*\.zip$)'
allowed_public_paths='^specs/(cli-ux-dx-defaults/(plan|spec|tasks)|guided-human-cli-ux-ax/spec)\.md$'

tracked_or_new="$(mktemp)"
blocked_filtered="$(mktemp)"
content_hits="$(mktemp)"
content_filtered="$(mktemp)"
trap 'rm -f "$tracked_or_new" "$tracked_or_new.existing" "$blocked_filtered" "$content_hits" "$content_filtered"' EXIT
{
  git ls-files
  git ls-files --others --exclude-standard
} | sort -u >"$tracked_or_new"

while IFS= read -r path; do
  [ -e "$path" ] && printf '%s\n' "$path"
done <"$tracked_or_new" >"$tracked_or_new.existing"

if grep -E "$blocked_paths" "$tracked_or_new.existing" | grep -Ev "$allowed_public_paths" >"$blocked_filtered"; then
  echo "Public repo contains non-public/review paths:"
  cat "$blocked_filtered"
  exit 1
fi

content_markers='/Users/[[:alnum:]_.-]+|/home/[[:alnum:]_.-]+|[A-Za-z]:\\Users\\'
if rg -n "$content_markers" --glob '!target/**' --glob '!tmp/**' --glob '!Cargo.lock' --glob '!scripts/check_public_boundary.sh' . >"$content_hits"; then
  if grep -Ev '(/Users/example|/home/example|[A-Za-z]:\\Users\\example)' "$content_hits" >"$content_filtered"; then
    filter_status=0
  else
    filter_status=$?
  fi
  if [ "$filter_status" -gt 1 ]; then
    echo "Public boundary content filter failed (status $filter_status)." >&2
    exit "$filter_status"
  fi
  if [ -s "$content_filtered" ]; then
    echo "Public repo contains non-public content markers:"
    cat "$content_filtered"
    exit 1
  fi
else
  scan_status=$?
  if [ "$scan_status" -ne 1 ]; then
    echo "Public boundary content scan failed (rg status $scan_status)." >&2
    exit "$scan_status"
  fi
fi

echo "Public boundary OK"
