#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="darwin-arm64"
LIB_DIR="$(bash "$ROOT/build/fetch-libpdfium.sh" "$TARGET")"
SWIFT_PACKAGE="$ROOT/packages/swift/PdfiumSwift"
SWIFT_SCRATCH="$ROOT/build/out/swift-direct"
SWIFTC="${SWIFT_EXEC:-swiftc}"
VERSION="$($SWIFTC --version | head -n 1)"
if [[ ! "$VERSION" =~ "Swift version 6.4" ]]; then
  echo "Swift 6.4 is required (found: $VERSION). Run: swiftly use 6.4.0" >&2
  exit 1
fi

(cd "$ROOT" && EPDF_SCAFFOLD_TARGET="$TARGET" cargo build --release -p epdf-ops --example native_bench)
PATH="$(dirname "$SWIFTC"):$PATH" CLANG_MODULE_CACHE_PATH="$SWIFT_SCRATCH/clang-cache" swift build --disable-sandbox \
  --package-path "$SWIFT_PACKAGE" --scratch-path "$SWIFT_SCRATCH" -c release \
  -Xcc -I"$LIB_DIR/include" -Xlinker -L -Xlinker "$LIB_DIR/lib" \
  -Xlinker -rpath -Xlinker "$LIB_DIR/lib"

DYLD_LIBRARY_PATH="$LIB_DIR/lib" node "$ROOT/bench/compare-swift-rust.mjs" \
  "$ROOT/target/release/examples/native_bench" \
  "$SWIFT_SCRATCH/release/PdfiumSwiftBench" \
  "$ROOT/fixtures/report.pdf" "${1:-20}"
