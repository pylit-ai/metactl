#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/metactl-packaged-smoke.XXXXXX")"
CONTAINER="metactl-packaged-smoke-$$"
trap 'docker rm -f "$CONTAINER" >/dev/null 2>&1 || true; rm -rf "$TMP_ROOT"' EXIT

command -v docker >/dev/null
docker info >/dev/null

cd "$ROOT"
PACKAGE_ARGS=(-p metactl --locked)
if [[ "${METACTL_PACKAGE_SMOKE_ALLOW_DIRTY:-0}" == "1" ]]; then
  PACKAGE_ARGS+=(--allow-dirty)
fi
cargo package "${PACKAGE_ARGS[@]}" >/dev/null

CRATE="$(ls -t target/package/metactl-*.crate | head -1)"
mkdir -p "$TMP_ROOT/package"
tar -xzf "$CRATE" -C "$TMP_ROOT/package"
PACKAGE_DIR="$(find "$TMP_ROOT/package" -mindepth 1 -maxdepth 1 -type d | head -1)"

docker run -d --name "$CONTAINER" \
  --mount "type=bind,src=$PACKAGE_DIR,dst=/package,readonly" \
  -w /sandbox \
  rust:1-bookworm \
  sleep infinity >/dev/null

docker exec "$CONTAINER" bash -lc \
  'export PATH=/usr/local/cargo/bin:$PATH; apt-get update >/dev/null && apt-get install -y --no-install-recommends python3 git ca-certificates >/dev/null'

docker exec "$CONTAINER" bash -lc \
  'export PATH=/usr/local/cargo/bin:$PATH; export CARGO_TARGET_DIR=/tmp/metactl-cargo-target; cargo install --path /package --locked >/dev/null'

docker exec "$CONTAINER" bash -lc \
  'export PATH=/usr/local/cargo/bin:$PATH; metactl demo create --sync >/dev/null'

docker exec "$CONTAINER" bash -lc \
  'export PATH=/usr/local/cargo/bin:$PATH; cd "$(metactl demo path)" && metactl sync --preview --json --no-input | python3 -m json.tool >/dev/null'

docker exec "$CONTAINER" bash -lc \
  'export PATH=/usr/local/cargo/bin:$PATH; cd "$(metactl demo path)" && metactl validate >/dev/null'

docker exec "$CONTAINER" bash -lc \
  'export PATH=/usr/local/cargo/bin:$PATH; mkdir -p /sandbox/use-pack && cd /sandbox/use-pack && metactl init -t codex-cli --no-input >/dev/null && metactl use python-refactor --no-input >/dev/null && test -f AGENTS.md && ! metactl use missing-pack --no-input >/tmp/missing-pack.out 2>&1'

docker exec "$CONTAINER" bash -lc \
  'export PATH=/usr/local/cargo/bin:$PATH; mkdir -p /sandbox/bad-target && cd /sandbox/bad-target && ! metactl init -t made-up-target --no-input >/tmp/bad-target.out 2>&1'

echo "smoke-packaged-metactl: OK"
