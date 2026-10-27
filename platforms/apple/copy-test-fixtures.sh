#!/usr/bin/env bash
# Copy package test resources without requiring a Rust or PDFium build.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT="$ROOT/platforms/apple/EmbedPDF"
EXPERIMENTAL=0
for arg in "$@"; do
  case "$arg" in
    --experimental-manual-bridge) EXPERIMENTAL=1 ;;
    *) echo "unknown argument: $arg" >&2; exit 2 ;;
  esac
done

# The tests read the same fixtures the Rust tests read. SwiftPM will only
# bundle resources that live inside the package, so they are copied rather than
# referenced, and `.gitignore`d rather than committed twice.
TEST_TARGETS=(EmbedPDFBindingsSmokeTests EmbedPDFTests EmbedPDFUITests)
if [[ $EXPERIMENTAL == 1 ]]; then
  TEST_TARGETS+=(EmbedPDFBridgeBenchmarks)
fi
# Repo-relative files every test bundle gets, one per line. Each plugin plan
# appends its own after the last plan's lines. Entries are expanded
# unquoted, so a glob copies every match.
FIXTURES=(
  pdfium-multiplatform-prototype/fixtures/report.pdf
  pdfium-multiplatform-prototype/fixtures/colors.pdf
  pdfium-multiplatform-prototype/fixtures/encrypted-hello.pdf
  tooling/viewer-e2e/fixtures/features.pdf
  tooling/engine-conformance/fixtures/annotation_kinds.pdf
  tooling/engine-conformance/fixtures/tiny.png
  tooling/engine-conformance/fixtures/form_fields.pdf
  packages/default-stamps/en/stamps.pdf
  tooling/engine-conformance/fixtures/document_actions.pdf
  crates/test-support/fixtures/measure-foreign-viewport.pdf
  packages/engine/main/test/fixtures/signed_two_approvals.pdf
  packages/engine/main/test/fixtures/signed_certified.pdf
  packages/engine/main/test/fixtures/unsigned_sigfield.pdf
  packages/engine/main/test/fixtures/signature_artwork.pdf
  packages/engine/main/test/fixtures/signature-compat/keys/TEST-ONLY-*.pem
)
for tests in "${TEST_TARGETS[@]}"; do
  mkdir -p "$OUT/Tests/$tests/Fixtures"
  for fixture in "${FIXTURES[@]}"; do
    # shellcheck disable=SC2086
    cp "$ROOT"/$fixture "$OUT/Tests/$tests/Fixtures/"
  done
done

