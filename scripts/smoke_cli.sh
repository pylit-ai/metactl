#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INSTALL_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/metactl-cli-install.XXXXXX")"
PROJECT_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/metactl-cli-project.XXXXXX")"
trap 'rm -rf "$INSTALL_ROOT" "$PROJECT_ROOT"' EXIT

cd "$ROOT"

cargo install --path crates/metactl --root "$INSTALL_ROOT" --force >/dev/null

METACTL_BIN="$INSTALL_ROOT/bin/metactl"
TEST_HOME="$PROJECT_ROOT/.test-home"
mkdir -p "$TEST_HOME"
unset METACTL_PROFILE XDG_CONFIG_HOME
export HOME="$TEST_HOME"

"$METACTL_BIN" --help >/dev/null
"$METACTL_BIN" --project "$PROJECT_ROOT" init --role release-manager --policy release-policy --target codex-cli --target openclaw >/dev/null
"$METACTL_BIN" --project "$PROJECT_ROOT" search "python refactor" >/dev/null
"$METACTL_BIN" --project "$PROJECT_ROOT" explain >/dev/null
"$METACTL_BIN" --project "$PROJECT_ROOT" doctor >/dev/null
"$METACTL_BIN" --project "$PROJECT_ROOT" compile >/dev/null
test -f "$PROJECT_ROOT/.metactl/generated/codex-cli/AGENTS.md"
test -f "$PROJECT_ROOT/.metactl/generated/openclaw/OPENCLAW.md"
"$METACTL_BIN" --project "$PROJECT_ROOT" apply --mode copy >/dev/null
"$METACTL_BIN" --project "$PROJECT_ROOT" validate >/dev/null
test -f "$PROJECT_ROOT/AGENTS.md"
test -f "$PROJECT_ROOT/OPENCLAW.md"
"$METACTL_BIN" --project "$PROJECT_ROOT" revert --all >/dev/null
test ! -e "$PROJECT_ROOT/AGENTS.md"
test ! -e "$PROJECT_ROOT/OPENCLAW.md"

echo "metactl CLI smoke passed"
