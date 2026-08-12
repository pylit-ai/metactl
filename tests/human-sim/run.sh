#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
METACTL_BIN="${METACTL_BIN:-$REPO_ROOT/target/debug/metactl}"
GOLDEN="$REPO_ROOT/tests/human-sim/golden/summary.txt"
UPDATE_GOLDEN="${UPDATE_GOLDEN:-0}"

if [[ ! -x "$METACTL_BIN" ]]; then
  cargo build --offline --manifest-path "$REPO_ROOT/Cargo.toml" -p metactl >/dev/null
fi

SIM_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/metactl-human-sim.XXXXXX")"
trap 'rm -rf "$SIM_ROOT"' EXIT
PROJECT="$SIM_ROOT/project"
mkdir -p "$PROJECT"
git -C "$PROJECT" init -q
git -C "$PROJECT" config user.email sim@example.invalid
git -C "$PROJECT" config user.name "Human Sim"
export HOME="$SIM_ROOT/home"
mkdir -p "$HOME"

run_json() {
  "$METACTL_BIN" --project "$PROJECT" --json "$@"
}

run_json init --target codex-cli --role builder --policy brownfield-safe-builder --no-input -y >/dev/null
PREVIEW_ONE="$SIM_ROOT/preview-one.json"
run_json sync --preview >"$PREVIEW_ONE"
PLAN_ONE="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["apply"]["targets"][0]["plan_digest"])' "$PREVIEW_ONE")"
CLASS_ONE="$(python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); print(",".join(sorted({a["classification"] for a in d["apply"]["targets"][0]["actions"]})))' "$PREVIEW_ONE")"
run_json sync --apply --plan-digest "$PLAN_ONE" >"$SIM_ROOT/apply-one.json"
if [[ -f "$PROJECT/.agents/skills/python-refactor/python-refactor/SKILL.md" || -f "$PROJECT/.agents/skills/python-refactor/contracts/SKILL.md" ]]; then
  CANONICAL_EXISTS=true
else
  CANONICAL_EXISTS=false
fi

PREVIEW_TWO="$SIM_ROOT/preview-two.json"
run_json sync --preview >"$PREVIEW_TWO"
PLAN_TWO="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["apply"]["targets"][0]["plan_digest"])' "$PREVIEW_TWO")"
run_json sync --apply --plan-digest "$PLAN_TWO" >"$SIM_ROOT/apply-two.json"
RECEIPT_STATUS="$(python3 -c 'import json,sys,pathlib; d=json.load(open(sys.argv[1])); p=pathlib.Path(sys.argv[2])/d["apply"]["targets"][0]["receipt_path"]; print(json.load(open(p))["status"])' "$SIM_ROOT/apply-two.json" "$PROJECT")"

STALE_PREVIEW="$SIM_ROOT/stale-preview.json"
run_json sync --preview >"$STALE_PREVIEW"
STALE_PLAN="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["apply"]["targets"][0]["plan_digest"])' "$STALE_PREVIEW")"
printf '\n# user edit\n' >>"$PROJECT/.agents/skills/python-refactor/python-refactor/SKILL.md"
set +e
run_json sync --apply --plan-digest "$STALE_PLAN" >"$SIM_ROOT/stale-apply.json"
STALE_EXIT=$?
set -e
STALE_MESSAGE="$(python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); print("stale_plan" if "stale_plan" in str(d) else d.get("message","missing"))' "$SIM_ROOT/stale-apply.json")"

LEGACY="$PROJECT/.codex/skills/python-refactor/python-refactor/SKILL.md"
mkdir -p "$(dirname "$LEGACY")"
printf 'user-owned divergent legacy bytes\n' >"$LEGACY"
rm -f "$PROJECT/.agents/skills/python-refactor/python-refactor/SKILL.md"
rm -f "$PROJECT/.metactl/state/codex-cli.json"
LEGACY_BEFORE="$(shasum -a 256 "$LEGACY" | awk '{print $1}')"
run_json sync --preview >"$SIM_ROOT/legacy-preview.json"
LEGACY_CLASS="$(python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); print(next(a["classification"] for a in d["apply"]["targets"][0]["actions"] if "python-refactor/python-refactor" in a["destination_path"]))' "$SIM_ROOT/legacy-preview.json")"
LEGACY_PLAN="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["apply"]["targets"][0]["plan_digest"])' "$SIM_ROOT/legacy-preview.json")"
set +e
run_json sync --apply --plan-digest "$LEGACY_PLAN" >"$SIM_ROOT/legacy-apply.json"
LEGACY_EXIT=$?
set -e
LEGACY_AFTER="$(shasum -a 256 "$LEGACY" | awk '{print $1}')"

cat >"$SIM_ROOT/summary.txt" <<EOF
greenfield.canonical_exists=$CANONICAL_EXISTS
greenfield.legacy_created=$(test -e "$PROJECT/.codex/skills/python-refactor/contracts/SKILL.md" && echo true || echo false)
greenfield.classifications=$CLASS_ONE
repeat.receipt_status=$RECEIPT_STATUS
stale.exit=$STALE_EXIT
stale.reason=$STALE_MESSAGE
legacy.classification=$LEGACY_CLASS
legacy.exit=$LEGACY_EXIT
legacy.bytes_preserved=$(test "$LEGACY_BEFORE" = "$LEGACY_AFTER" && echo true || echo false)
network_or_user_root_access=not_exercised
claim_limit=offline_temp_filesystem_fixture
EOF

if [[ "$UPDATE_GOLDEN" == "1" ]]; then
  cp "$SIM_ROOT/summary.txt" "$GOLDEN"
fi
diff -u "$GOLDEN" "$SIM_ROOT/summary.txt"
cat "$SIM_ROOT/summary.txt"
