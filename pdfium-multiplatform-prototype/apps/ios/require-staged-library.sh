#!/usr/bin/env bash
# Run-script build phase (see project.yml's preBuildScripts) that checks the
# slice directory for whatever SDK Xcode is currently building against
# actually contains the matching native library, and fails the build with a plain
# explanation if not, instead of letting the compiler or linker fail first
# with something inscrutable.
#
# Without this, a build for the iOS Simulator or iOS device destination finds
# no dylib in its slice directory (the committed pin has no published mobile artifacts) and either:
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

EXT=a
[[ "$SLICE" == macos-* ]] && EXT=dylib
LIB="$FRAMEWORKS_DIR/$SLICE/libepdf_uniffi.$EXT"

if [[ -f "$LIB" ]]; then
  exit 0
fi

cat >&2 <<MSG
error: EpdfScaffold cannot link against $SLICE: no libepdf_uniffi.$EXT at
$LIB

Generate and stage this slice with build/build-swift.sh $SLICE from the
prototype root. For a pending mobile target, first set EPDF_SCAFFOLD_PIN_FILE
to a separate pin containing the matching local archive URL and checksum.
See README.md, "Building with local artifacts".

The local fork supports mobile targets; the committed release pin is pending.
Mobile slices link statically into the app. Device distribution still needs
signing/provisioning and verification on a physical device.

What already works:
  - the macOS destination of this same scheme: bash build/build-ios-app.sh
    (no argument), or Xcode's "My Mac" run destination
  - bash build/test-swift.sh, which exercises the uniffi bindings on macOS
    without needing Xcode at all
MSG
exit 1
