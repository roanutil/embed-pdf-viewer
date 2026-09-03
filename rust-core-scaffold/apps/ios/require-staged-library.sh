#!/usr/bin/env bash
# Run-script build phase (see project.yml's preBuildScripts) that checks the
# slice directory for whatever SDK Xcode is currently building against
# actually contains libepdf_uniffi.dylib, and fails the build with a plain
# explanation if not, instead of letting the compiler or linker fail first
# with something inscrutable.
#
# Without this, a build for the iOS Simulator or iOS device destination finds
# no dylib in its slice directory (embedpdf/runtime does not build ios-arm64
# or ios-sim-arm64 yet) and either:
#   - links against a leftover macOS dylib on a stale search path and fails
#     with "Building for 'iOS-simulator', but linking in dylib ... built for
#     'macOS'", or
#   - fails with a bare "library not found for -lepdf_uniffi"
# Neither tells whoever hit it what is actually missing or what to do next.
# This script runs before either of those and says so directly.
set -euo pipefail

FRAMEWORKS_DIR="$SRCROOT/../../packages/swift/EpdfOps/Frameworks"

case "${PLATFORM_NAME:-}" in
  macosx)
    SLICE=macos-arm64
    ;;
  iphonesimulator)
    SLICE=ios-sim-arm64
    ;;
  iphoneos)
    SLICE=ios-arm64
    ;;
  *)
    echo "warning: require-staged-library.sh does not recognize PLATFORM_NAME '${PLATFORM_NAME:-}'; skipping the staged-library check" >&2
    exit 0
    ;;
esac

LIB="$FRAMEWORKS_DIR/$SLICE/libepdf_uniffi.dylib"

if [[ -f "$LIB" ]]; then
  exit 0
fi

cat >&2 <<MSG
error: EpdfScaffold cannot link against $SLICE: no libepdf_uniffi.dylib at
$LIB

embedpdf/runtime does not build $SLICE yet. Its release matrix in
.github/workflows/release-libpdfium.yml covers nine targets (wasm32, two
darwin, two linux, two linuxmusl, two win32) and neither mobile one.
Adding ios-arm64 and ios-sim-arm64 is recommendation 6 of
docs/research/pdfium-rust-wrapper-decision.md, and has its own design doc.
Until that lands, the $PLATFORM_NAME destination of this scaffold cannot be
built, the same way build/fetch-libpdfium.sh already refuses to fetch either
mobile slice.

What already works:
  - the macOS destination of this same scheme: bash build/build-ios-app.sh
    (no argument), or Xcode's "My Mac" run destination
  - bash build/test-swift.sh, which exercises the uniffi bindings on macOS
    without needing Xcode at all
MSG
exit 1
