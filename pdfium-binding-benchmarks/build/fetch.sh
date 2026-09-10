#!/usr/bin/env bash
# Pull the same prebuilt libembedpdf.a the shipping build uses, pinned by
# sha256 in packages/engine/runtime/engine-runtime-build.json. No PDFium
# source build is needed anywhere in this spike.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUNTIME="$ROOT/../packages/engine/runtime"
TARGET="${1:-wasm32}"

PDF_RUNTIME_LIB_DIR="$ROOT/build/libpdfium" \
  bash "$RUNTIME/scripts/fetch-libpdfium.sh" "$TARGET"

echo "libembedpdf for $TARGET in $ROOT/build/libpdfium/$TARGET"
