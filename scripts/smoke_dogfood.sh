#!/usr/bin/env bash
set -euo pipefail

# smoke_dogfood.sh — exercises the 018 CLI surfaces that smoke_cli.sh does not cover:
#   use, use --local, hook install/status, source add/list, status provenance,
#   cursor compile output, local config layer, and init-without-target refusal.

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INSTALL_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/metactl-dogfood-install.XXXXXX")"
PROJECT_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/metactl-dogfood-project.XXXXXX")"
trap 'rm -rf "$INSTALL_ROOT" "$PROJECT_ROOT"' EXIT

cd "$ROOT"

# Build and install
cargo install --path crates/metactl --root "$INSTALL_ROOT" --force >/dev/null
METACTL_BIN="$INSTALL_ROOT/bin/metactl"
TEST_HOME="$PROJECT_ROOT/.test-home"
mkdir -p "$TEST_HOME"
unset METACTL_PROFILE XDG_CONFIG_HOME
export HOME="$TEST_HOME"

# ========================================================================
# 1. Init refuses without target in empty directory
# ========================================================================
if "$METACTL_BIN" --project "$PROJECT_ROOT" init 2>/dev/null; then
    echo "FAIL: init should refuse without target in empty dir" >&2
    exit 1
fi
echo "  [pass] init refuses without target"

# ========================================================================
# 2. Greenfield use workflow
# ========================================================================
"$METACTL_BIN" --project "$PROJECT_ROOT" init --target codex-cli --target cursor >/dev/null
"$METACTL_BIN" --project "$PROJECT_ROOT" add python-refactor >/dev/null
"$METACTL_BIN" --project "$PROJECT_ROOT" status >/dev/null
"$METACTL_BIN" --project "$PROJECT_ROOT" doctor >/dev/null
"$METACTL_BIN" --project "$PROJECT_ROOT" sync >/dev/null
"$METACTL_BIN" --project "$PROJECT_ROOT" validate >/dev/null
"$METACTL_BIN" --project "$PROJECT_ROOT" revert --all >/dev/null
echo "  [pass] greenfield use workflow"

# ========================================================================
# 3. Local config layer
# ========================================================================
echo "packs:" > "$PROJECT_ROOT/metactl.local.yaml"
echo "  - unit-test-loop" >> "$PROJECT_ROOT/metactl.local.yaml"
"$METACTL_BIN" --project "$PROJECT_ROOT" sync >/dev/null
# Verify lock has local config digest
if ! grep -q "local_config_digest" "$PROJECT_ROOT/metactl.lock.json"; then
    echo "FAIL: lock should contain local_config_digest after local config sync" >&2
    exit 1
fi
echo "  [pass] local config layer"

# ========================================================================
# 4. Hook install and status
# ========================================================================
git -C "$PROJECT_ROOT" init --quiet
"$METACTL_BIN" --project "$PROJECT_ROOT" hook install >/dev/null
"$METACTL_BIN" --project "$PROJECT_ROOT" hook status >/dev/null
test -f "$PROJECT_ROOT/.git/hooks/post-checkout"
test -f "$PROJECT_ROOT/.git/hooks/post-merge"
# Verify hook script has hardened guards
grep -q "symbolic-ref" "$PROJECT_ROOT/.git/hooks/post-checkout"
grep -q "metactl.lock.json" "$PROJECT_ROOT/.git/hooks/post-checkout"
echo "  [pass] hook install and status"

# ========================================================================
# 5. Private source add, sync, list, and audit
# ========================================================================
EXTRA_SOURCE="$(mktemp -d "${TMPDIR:-/tmp}/metactl-dogfood-source.XXXXXX")"
trap 'rm -rf "$INSTALL_ROOT" "$PROJECT_ROOT" "$EXTRA_SOURCE"' EXIT
mkdir -p "$EXTRA_SOURCE/packs" "$EXTRA_SOURCE/vendor/dogfood-private-pack"
echo '{"kind":"library","id":"extra-packs","version":"1.0.0"}' > "$EXTRA_SOURCE/library.json"
cat > "$EXTRA_SOURCE/vendor/dogfood-private-pack/SKILL.md" <<'EOF'
# Dogfood Private Pack

Smoke-test private source resolution.
EOF
cat > "$EXTRA_SOURCE/packs/dogfood-private-pack.json" <<'EOF'
{
  "kind": "pack",
  "id": "dogfood-private-pack",
  "version": "1.0.0",
  "title": "Dogfood Private Pack",
  "description": "Smoke-test private source resolution.",
  "activation_class": "instruction",
  "side_effect_class": "none",
  "trust_tier": "org_validated",
  "requires_confirmation": false,
  "compatible_roles": ["builder"],
  "compatible_targets": ["codex-cli"],
  "resources": [
    {
      "path": "vendor/dogfood-private-pack/SKILL.md",
      "kind": "instruction",
      "required": true
    }
  ],
  "visibility_scope": "private"
}
EOF
"$METACTL_BIN" --project "$PROJECT_ROOT" source add extra-packs "$EXTRA_SOURCE" --private >/dev/null
"$METACTL_BIN" --project "$PROJECT_ROOT" source sync extra-packs >/dev/null
SOURCE_LIST=$("$METACTL_BIN" --project "$PROJECT_ROOT" --json source list 2>/dev/null)
if ! echo "$SOURCE_LIST" | grep -q "$EXTRA_SOURCE"; then
    echo "FAIL: source list should include added source path" >&2
    exit 1
fi
"$METACTL_BIN" --project "$PROJECT_ROOT" --json audit sources >/dev/null
echo "  [pass] private source add, sync, list, and audit"

# ========================================================================
# 6. Cursor compile output verification
# ========================================================================
"$METACTL_BIN" --project "$PROJECT_ROOT" compile --update-lock >/dev/null
test -d "$PROJECT_ROOT/.metactl/generated/cursor"
test -f "$PROJECT_ROOT/.metactl/generated/cursor/.cursor/rules/metactl-pack-index.mdc"
echo "  [pass] cursor compile output paths"

# ========================================================================
# 7. Status shows provenance layers
# ========================================================================
STATUS_OUTPUT=$("$METACTL_BIN" --project "$PROJECT_ROOT" status 2>/dev/null)
if ! echo "$STATUS_OUTPUT" | grep -q "Layers:"; then
    echo "FAIL: status should show Layers section" >&2
    exit 1
fi
echo "  [pass] status provenance layers"

echo "metactl dogfood smoke passed"
