#!/usr/bin/env bash
set -euo pipefail

artifact="${1:?usage: verify_github_attestation.sh <artifact> [repository]}"
repository="${2:-pylit-ai/metactl}"

if ! command -v gh >/dev/null 2>&1 || ! gh attestation verify --help >/dev/null 2>&1; then
  echo "::warning title=metactl provenance verification skipped::GitHub CLI with attestation support is unavailable. SHA-256 verification passed, but build provenance was not verified. See https://github.com/pylit-ai/metactl/blob/main/docs/user/install-verification.md"
  exit 0
fi

echo "Verifying GitHub build provenance for $artifact"
gh attestation verify "$artifact" --repo "$repository"
