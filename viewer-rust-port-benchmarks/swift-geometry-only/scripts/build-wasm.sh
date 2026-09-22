#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SWIFTC="${SWIFT_EXEC:-swiftc}"
VERSION="$($SWIFTC --version | head -n 1)"
if [[ ! "$VERSION" =~ "Swift version 6.4" ]]; then
  echo "Swift 6.4 is required (found: $VERSION). Run: swiftly use 6.4.0" >&2
  exit 1
fi
if ! swift sdk list | grep -q '^swift-6.4.0-RELEASE_wasm-embedded$'; then
  echo "swift-6.4.0-RELEASE_wasm-embedded is required; see ../README.md" >&2
  exit 1
fi

SCRATCH="$ROOT/.build-benchmark"
PATH="$(dirname "$SWIFTC"):$PATH" CLANG_MODULE_CACHE_PATH="$SCRATCH/clang-cache" swift build \
  --disable-sandbox --package-path "$ROOT" --scratch-path "$SCRATCH" \
  --swift-sdk swift-6.4.0-RELEASE_wasm-embedded -c release
ARTIFACT="$(find "$SCRATCH" -name SwiftGeometry.wasm -type f -print -quit)"
test -n "$ARTIFACT"
mkdir -p "$ROOT/wasm"
cp "$ARTIFACT" "$ROOT/wasm/swift_geometry.wasm"
echo "wasm: $(wc -c < "$ROOT/wasm/swift_geometry.wasm") bytes"
