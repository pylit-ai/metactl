#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$ROOT/target/debug/metactl"

cargo build -q -p metactl

SANDBOX="$(mktemp -d "${TMPDIR:-/tmp}/metactl-opencode-target.XXXXXX")"
cleanup() {
  rm -rf "$SANDBOX"
}
trap cleanup EXIT

export XDG_CONFIG_HOME="$SANDBOX/.xdg-config"
export XDG_CACHE_HOME="$SANDBOX/.xdg-cache"

PROJECT="$SANDBOX/project"
mkdir -p "$PROJECT"
cd "$PROJECT"
git init -q
printf '# OpenCode target verifier\n' > README.md
git add README.md
git commit -qm seed

"$BIN" init --target codex-cli --no-input --yes >/tmp/metactl-opencode-init.out
"$BIN" target add opencode >/tmp/metactl-opencode-target-add.out
"$BIN" add unit-test-loop >/tmp/metactl-opencode-add-pack.out
"$BIN" compile >/tmp/metactl-opencode-compile.out
"$BIN" validate --target opencode >/tmp/metactl-opencode-validate.out

required=(
  ".metactl/generated/opencode/AGENTS.md"
  ".metactl/generated/opencode/opencode.json"
  ".metactl/generated/opencode/.opencode/commands/run-targeted-tests.md"
  ".metactl/generated/opencode/.opencode/skills/unit-test-loop/SKILL.md"
  ".metactl/generated/opencode/.opencode/packs/unit-test-loop/testing-discipline.md"
)

for path in "${required[@]}"; do
  if [[ ! -f "$path" ]]; then
    echo "missing generated OpenCode surface: $path" >&2
    exit 1
  fi
done

python3 - <<'PY'
import json
from pathlib import Path

config = json.loads(Path(".metactl/generated/opencode/opencode.json").read_text())
assert config["$schema"] == "https://opencode.ai/config.json"
assert "AGENTS.md" in config["instructions"]
assert ".opencode/packs/*/*.md" in config["instructions"]
assert config["permission"]["edit"] == "ask"
assert config["permission"]["bash"] == "ask"

manifest = json.loads(Path(".metactl/generated/opencode/compile.manifest.json").read_text())
paths = {item["destination_path"] for item in manifest["generated_outputs"]}
expected = {
    "AGENTS.md",
    "opencode.json",
    ".opencode/commands/run-targeted-tests.md",
    ".opencode/skills/unit-test-loop/SKILL.md",
    ".opencode/packs/unit-test-loop/testing-discipline.md",
}
missing = expected - paths
assert not missing, sorted(missing)
PY

echo "verify-opencode-target: OK"
