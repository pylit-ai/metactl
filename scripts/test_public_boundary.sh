#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
checker="$script_dir/check_public_boundary.sh"
bash_bin="$(command -v bash)"
if ! command -v rg >/dev/null 2>&1; then
  echo "Boundary regression tests require rg (ripgrep)." >&2
  exit 2
fi

scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
fixture="$scratch/repo"
mkdir -p "$fixture" "$scratch/no-scanner" "$scratch/error-scanner"
git -C "$fixture" init -q
printf 'Synthetic public fixture\n' >"$fixture/README.md"

check_case() {
  local name="$1" expected="$2" status
  shift 2
  if (cd "$fixture" && "$@") >"$scratch/output" 2>&1; then
    status=0
  else
    status=$?
  fi
  if [ "$status" -ne "$expected" ]; then
    echo "FAIL $name: expected status $expected, got $status" >&2
    cat "$scratch/output" >&2
    exit 1
  fi
  if [ "$expected" -ne 0 ] && grep -q 'Public boundary OK' "$scratch/output"; then
    echo "FAIL $name: failed scan reported success" >&2
    cat "$scratch/output" >&2
    exit 1
  fi
  echo "PASS $name"
}

check_case clean_fixture 0 "$bash_bin" "$checker"
check_case missing_scanner 2 env PATH="$scratch/no-scanner" "$bash_bin" "$checker"
grep -q 'required scanner rg' "$scratch/output"
printf '#!%s\nexit 2\n' "$bash_bin" >"$scratch/error-scanner/rg"
chmod +x "$scratch/error-scanner/rg"
check_case scanner_error 2 env PATH="$scratch/error-scanner:$PATH" "$bash_bin" "$checker"
grep -q 'rg status 2' "$scratch/output"

# Construct the synthetic private marker so this test source is itself public.
printf '/%s/%s/private-fixture\n' Users synthetic_operator >"$fixture/README.md"
check_case private_marker 1 "$bash_bin" "$checker"
grep -q 'non-public content markers' "$scratch/output"
printf '/%s/%s/documented-fixture\n' Users example >"$fixture/README.md"
check_case permitted_example 0 "$bash_bin" "$checker"
echo "Public boundary regression tests OK (5 cases)"
