#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INSTALL_ROOT=""
SANDBOX="$(mktemp -d "${TMPDIR:-/tmp}/metactl-trust-smoke.XXXXXX")"
trap 'rm -rf "$INSTALL_ROOT" "$SANDBOX"' EXIT

cd "$ROOT"

if [[ -z "${METACTL_BIN:-}" ]]; then
  INSTALL_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/metactl-trust-install.XXXXXX")"
  cargo install --path crates/metactl --root "$INSTALL_ROOT" --force >/dev/null
  METACTL_BIN="$INSTALL_ROOT/bin/metactl"
fi

run_metactl() {
  local project="$1"
  shift
  HOME="$project/.test-home" \
    METACTL_PROFILE= \
    XDG_CONFIG_HOME= \
    "$METACTL_BIN" --project "$project" "$@"
}

assert_regular_file() {
  local path="$1"
  [[ -f "$path" && ! -L "$path" ]]
}

greenfield="$SANDBOX/greenfield-codex"
mkdir -p "$greenfield"
run_metactl "$greenfield" init --target codex-cli >/dev/null
run_metactl "$greenfield" sync >/dev/null
run_metactl "$greenfield" validate >/dev/null
assert_regular_file "$greenfield/AGENTS.md"
grep -q "\\[metactl Instruction Index\\]" "$greenfield/AGENTS.md"

brownfield="$SANDBOX/brownfield-claude"
mkdir -p "$brownfield"
run_metactl "$brownfield" init --target claude-code >/dev/null
sentinel="TRUST_SMOKE_CLAUDE_SENTINEL"
{
  echo "# Claude Rules"
  echo
  for _ in $(seq 1 80); do
    echo "$sentinel preserve this guidance"
  done
} > "$brownfield/CLAUDE.md"
if run_metactl "$brownfield" sync >/tmp/metactl-trust-refusal.out 2>&1; then
  echo "expected brownfield sync refusal" >&2
  exit 1
fi
grep -q "metactl sync --adopt patch" /tmp/metactl-trust-refusal.out
run_metactl "$brownfield" sync --adopt patch >/dev/null
run_metactl "$brownfield" sync >/dev/null
grep -q "$sentinel" "$brownfield/CLAUDE.md"
[[ "$(grep -c "metactl:begin" "$brownfield/CLAUDE.md")" = "1" ]]
assert_regular_file "$brownfield/CLAUDE.md"

lock_project="$SANDBOX/active-lock"
mkdir -p "$lock_project"
run_metactl "$lock_project" init --target codex-cli >/dev/null
METACTL_TEST_HOLD_OPERATION_LOCK_MS=1500 run_metactl "$lock_project" sync >/tmp/metactl-trust-held.out 2>&1 &
held_pid=$!
for _ in $(seq 1 120); do
  [[ -f "$lock_project/.metactl/state/operation.lock" ]] && break
  sleep 0.025
done
if [[ ! -f "$lock_project/.metactl/state/operation.lock" ]]; then
  echo "operation lock was not created" >&2
  kill "$held_pid" 2>/dev/null || true
  wait "$held_pid" 2>/dev/null || true
  exit 1
fi
if run_metactl "$lock_project" sync >/tmp/metactl-trust-active-lock.out 2>&1; then
  echo "expected active operation lock refusal" >&2
  kill "$held_pid" 2>/dev/null || true
  wait "$held_pid" 2>/dev/null || true
  exit 1
fi
grep -q "another metactl write operation is already active" /tmp/metactl-trust-active-lock.out
wait "$held_pid"

private_project="$SANDBOX/private-leak"
mkdir -p "$private_project"
run_metactl "$private_project" init --target codex-cli --target claude-code --target gemini-cli --target cursor >/dev/null
run_metactl "$private_project" add local-only-example >/dev/null
run_metactl "$private_project" compile >/dev/null
for surface in \
  ".metactl/generated/codex-cli/AGENTS.md" \
  ".metactl/generated/claude-code/CLAUDE.md" \
  ".metactl/generated/gemini-cli/GEMINI.md" \
  ".metactl/generated/cursor/.cursor/rules/metactl-pack-index.mdc"; do
  if grep -Eq "local-only-example|Local-Only Example" "$private_project/$surface"; then
    echo "private marker leaked into $surface" >&2
    exit 1
  fi
done

echo "metactl trust blindspot smoke passed"
