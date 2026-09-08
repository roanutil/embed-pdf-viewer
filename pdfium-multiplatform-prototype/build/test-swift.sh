#!/usr/bin/env bash
# swift test with the staged dylibs on the linker and loader paths. Without
# this, Package.swift's .linkedLibrary("epdf_uniffi") has nowhere to look.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SLICE="${1:-macos-arm64}"
LIBS="$ROOT/packages/swift/EpdfOps/Frameworks/$SLICE"

if [[ ! -f "$LIBS/libepdf_uniffi.dylib" ]]; then
  echo "no staged libraries at $LIBS" >&2
  echo "Fix with: bash build/build-swift.sh $SLICE" >&2
  exit 1
fi

export LIBRARY_PATH="$LIBS${LIBRARY_PATH:+:$LIBRARY_PATH}"

cd "$ROOT/packages/swift/EpdfOps"
# swift test runs the bundle through swiftpm-testing-helper, which is
# Apple-signed with the hardened runtime and carries no
# allow-dyld-environment-variables entitlement, so macOS strips DYLD_*
# before it reaches the test bundle. Bake the load path into the test
# binary's rpath instead; that survives the strip.
swift test -Xlinker -rpath -Xlinker "$LIBS" "${@:2}"
