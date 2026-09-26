# Verify release installation provenance

metactl release archives have two independent consumer checks:

1. The `.sha256` file detects an archive that does not match the published release checksum.
2. A GitHub build-provenance attestation links that archive digest to the `pylit-ai/metactl` GitHub Actions build.

The composite GitHub Action performs both checks when the runner provides GitHub CLI with `gh attestation verify` support. If that command is unavailable, the Action keeps the checksum-verified installation path working and emits a workflow warning before extraction.

The npm shim always verifies the checksum. It does not bundle a Sigstore verifier, so it emits an explicit warning and leaves provenance verification as a manual step. To verify the exact npm release archive independently:

```bash
version="$(npm view @pylit-ai/metactl version)"
tag="v$version"
case "$(uname -s)/$(uname -m)" in
  Linux/x86_64) target="x86_64-unknown-linux-gnu" ;;
  Darwin/arm64) target="aarch64-apple-darwin" ;;
  *) echo "No prebuilt npm target for this platform" >&2; exit 1 ;;
esac
archive="metactl-${tag}-${target}.tar.gz"
workdir="$(mktemp -d)"
gh release download "$tag" --repo pylit-ai/metactl --pattern "$archive" --dir "$workdir"
gh attestation verify "$workdir/$archive" --repo pylit-ai/metactl
rm -rf "$workdir"
```

Successful verification confirms the archive digest has a valid SLSA provenance attestation associated with `pylit-ai/metactl`. It does not prove the program is vulnerability-free; it proves which repository and build identity produced the bytes.
