#!/usr/bin/env bash
# Native arms. Arm A is the addon that actually ships, built out of
# packages/engine/runtime; arms B and C are one napi-rs addon; arm D is a
# cmake-js addon mirroring the shipping CMakeLists.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="${1:-darwin-arm64}"
LIB_DIR="$ROOT/build/libpdfium/$TARGET/lib"
OUT_DIR="$ROOT/build/out/native"

mkdir -p "$OUT_DIR"

PDFIUM_LIB_DIR="$LIB_DIR" cargo build --manifest-path "$ROOT/Cargo.toml" \
  -p napi-arms --release
cp "$ROOT/target/release/libnapi_arms.dylib" "$OUT_DIR/napi-arms.node.tmp"
mv -f "$OUT_DIR/napi-arms.node.tmp" "$OUT_DIR/napi-arms.node"
cp "$LIB_DIR/libembedpdf.dylib" "$OUT_DIR/libembedpdf.dylib.tmp"
mv -f "$OUT_DIR/libembedpdf.dylib.tmp" "$OUT_DIR/libembedpdf.dylib"

echo "native arms b,c -> $OUT_DIR/napi-arms.node"

INCLUDE_DIR="$ROOT/build/libpdfium/$TARGET/include"
npx --prefix "$ROOT" cmake-js compile \
  --directory "$ROOT/build" \
  --arch arm64 \
  --CDPDFIUM_LIB_DIR="$LIB_DIR" \
  --CDPDFIUM_INCLUDE_DIR="$INCLUDE_DIR"

cp "$ROOT/build/build/Release/cc-ops.node" "$OUT_DIR/cc-ops.node.tmp"
mv -f "$OUT_DIR/cc-ops.node.tmp" "$OUT_DIR/cc-ops.node"
echo "native arm d -> $OUT_DIR/cc-ops.node"
