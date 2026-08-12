#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
verifier="$root/scripts/verify_github_attestation.sh"
temporary="$(mktemp -d)"
trap 'rm -rf "$temporary"' EXIT
artifact="$temporary/metactl-v0.1.21-x86_64-unknown-linux-gnu.tar.gz"
: > "$artifact"

unavailable_output="$(PATH="$temporary/empty" /bin/bash "$verifier" "$artifact" 2>&1)"
case "$unavailable_output" in
  *"provenance verification skipped"*"SHA-256 verification passed"*) ;;
  *) echo "missing explicit unavailable-tool warning" >&2; exit 1 ;;
esac

mkdir -p "$temporary/bin"
cat > "$temporary/bin/gh" <<'EOF'
#!/bin/sh
if [ "$1 $2 $3" = "attestation verify --help" ]; then
  exit "${FAKE_GH_HELP_EXIT:-0}"
fi
printf '%s\n' "$*" > "$GH_RECORD"
exit "${FAKE_GH_VERIFY_EXIT:-0}"
EOF
chmod +x "$temporary/bin/gh"

GH_RECORD="$temporary/gh-args" PATH="$temporary/bin:/usr/bin:/bin" /bin/bash "$verifier" "$artifact"
expected="attestation verify $artifact --repo pylit-ai/metactl"
actual="$(cat "$temporary/gh-args")"
[ "$actual" = "$expected" ] || { echo "unexpected gh arguments: $actual" >&2; exit 1; }

set +e
GH_RECORD="$temporary/gh-args-fail" FAKE_GH_VERIFY_EXIT=17 PATH="$temporary/bin:/usr/bin:/bin" /bin/bash "$verifier" "$artifact" >/dev/null 2>&1
status=$?
set -e
[ "$status" -eq 17 ] || { echo "attestation failure did not fail closed: $status" >&2; exit 1; }

old_cli_output="$(GH_RECORD="$temporary/unused" FAKE_GH_HELP_EXIT=1 PATH="$temporary/bin:/usr/bin:/bin" /bin/bash "$verifier" "$artifact" 2>&1)"
case "$old_cli_output" in
  *"provenance verification skipped"*) ;;
  *) echo "missing warning for gh without attestation support" >&2; exit 1 ;;
esac

echo "verify-github-attestation tests: OK"
