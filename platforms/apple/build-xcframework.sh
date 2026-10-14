#!/usr/bin/env bash
# Builds the Apple XCFramework and the generated Swift for the package around it.
#
# `multiplatform-plan.md` Phase 1: "Apple XCFramework in a Swift Package".
# Everything here is local; nothing is fetched and nothing is published. The
# libembedpdf slices come from whatever `scripts/fetch-libpdfium.sh` has put in
# `packages/engine/runtime/build/libpdfium/`, which for a local build means the
# fork's own `out/embedpdf-runtime-artifacts` registered through
# `scripts/runtime-use-local.sh`.
#
#   bash platforms/apple/build-xcframework.sh
#
# Produces the generated inputs of platforms/apple/EmbedPDF/ : the binary target
# EmbedPDFFFI.xcframework (device and simulator slices), the generated Swift in
# Sources/EmbedPDFBindings, and the test fixtures. Package.swift is tracked.
#
#   bash platforms/apple/build-xcframework.sh --experimental-manual-bridge
#
# builds an experimental XCFramework for Task 11's bridge benchmarks instead:
# the same archive plus the hand-written C bridge (crates/epdf-manual-bridge)
# and the benchmark control exports, with the `EpdfManualFFI` module in the
# modulemap. Use it with EMBEDPDF_EXPERIMENTAL_BRIDGE=1 (see Package.swift).
# Run the script again without the flag to get the shipping XCFramework back.
set -euo pipefail

EXPERIMENTAL=0
for arg in "$@"; do
  case "$arg" in
    --experimental-manual-bridge) EXPERIMENTAL=1 ;;
    *)
      echo "unknown argument: $arg" >&2
      exit 2
      ;;
  esac
done

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT="$ROOT/platforms/apple/EmbedPDF"
BUILD="$ROOT/platforms/apple/.build"
LIB_ROOT="$ROOT/packages/engine/runtime/build/libpdfium"
PROFILE="${PROFILE:-release}"

# The manual bridge links into epdf-viewer-host's own staticlib (one copy of
# the kernel thread's statics for both bridges), so the flag is only cargo
# features, never a second archive.
FEATURES=()
if [[ $EXPERIMENTAL == 1 ]]; then
  FEATURES=(--features "manual-bridge,epdf-ffi/bench")
fi

# Rust target -> fork target. The fork spells these the way Node does.
SLICES=(
  "aarch64-apple-ios:ios-arm64"
  "aarch64-apple-ios-sim:ios-sim-arm64"
)

rm -rf "$BUILD"
mkdir -p "$BUILD"

for slice in "${SLICES[@]}"; do
  rust="${slice%%:*}"
  fork="${slice##*:}"
  if [[ ! -f "$LIB_ROOT/$fork/lib/libembedpdf.a" ]]; then
    echo "missing $LIB_ROOT/$fork/lib/libembedpdf.a" >&2
    echo "Fetch it first:" >&2
    echo "  cd packages/engine/runtime" >&2
    echo "  bash scripts/runtime-use-local.sh $fork <path to the fork's tarball>" >&2
    echo "  PDF_RUNTIME_BUILD_FILE=\$PWD/engine-runtime-build.local.json \\" >&2
    echo "    bash scripts/fetch-libpdfium.sh $fork" >&2
    exit 1
  fi

  echo "building $rust"
  (cd "$ROOT" && cargo build -p epdf-viewer-host --target "$rust" --profile "$PROFILE" \
    ${FEATURES[@]+"${FEATURES[@]}"})

  # One archive per slice, because `xcodebuild -create-xcframework` takes one
  # `-library` per platform. libtool merges rather than concatenates, which is
  # what keeps the duplicate-symbol check honest.
  mkdir -p "$BUILD/$fork"
  libtool -static -o "$BUILD/$fork/libEmbedPDFFFI.a" \
    "$ROOT/target/$rust/$PROFILE/libepdf_viewer_host.a" \
    "$LIB_ROOT/$fork/lib/libembedpdf.a" 2>/dev/null
done

# The headers every slice shares. Generated from the built library rather than
# written by hand: uniffi's proc macros are the only thing that knows the
# current shape, and a hand-kept header drifts silently. Library mode emits
# Swift for every crate in the archive that has scaffolding: epdf_uniffi,
# epdf_ffi, epdf_viewer_host and the plugins' ffi modules.
echo "generating bindings"
(cd "$ROOT" && cargo build -p uniffi-bindgen)
rm -rf "$BUILD/generated" "$BUILD/headers"
mkdir -p "$BUILD/generated" "$BUILD/headers"
"$ROOT/target/debug/uniffi-bindgen" generate \
  --library "$ROOT/target/aarch64-apple-ios-sim/$PROFILE/libepdf_viewer_host.a" \
  --language swift --out-dir "$BUILD/generated"

# One *FFI.h and one *FFI.modulemap per crate. xcframework wants a single
# module.modulemap inside the headers directory, so the per-crate maps are
# concatenated into it. uniffi writes each map without a trailing newline, so
# a bare `cat` would run one map's closing brace into the next `module`.
for map in "$BUILD/generated"/*.modulemap; do
  cat "$map"
  echo
done > "$BUILD/headers/module.modulemap"
cp "$BUILD/generated"/*.h "$BUILD/headers/"

if [[ $EXPERIMENTAL == 1 ]]; then
  cp "$ROOT/crates/epdf-manual-bridge/include/epdf_manual.h" "$BUILD/headers/"
  cat >> "$BUILD/headers/module.modulemap" <<'MODULEMAP'
module EpdfManualFFI {
  header "epdf_manual.h"
  export *
}
MODULEMAP
fi

echo "creating the xcframework"
rm -rf "$OUT/EmbedPDFFFI.xcframework"
xcodebuild -create-xcframework \
  -library "$BUILD/ios-arm64/libEmbedPDFFFI.a" -headers "$BUILD/headers" \
  -library "$BUILD/ios-sim-arm64/libEmbedPDFFFI.a" -headers "$BUILD/headers" \
  -output "$OUT/EmbedPDFFFI.xcframework" > /dev/null

mkdir -p "$OUT/Sources/EmbedPDFBindings"
rm -f "$OUT/Sources/EmbedPDFBindings"/*.swift
cp "$BUILD/generated"/*.swift "$OUT/Sources/EmbedPDFBindings/"

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
  tooling/engine-conformance/fixtures/document_actions.pdf
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

echo
echo "  $OUT/EmbedPDFFFI.xcframework"
xcodebuild -version | head -1
lipo -info "$OUT/EmbedPDFFFI.xcframework"/*/libEmbedPDFFFI.a 2>/dev/null | sed 's/^/  /'
