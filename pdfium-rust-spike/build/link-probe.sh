#!/usr/bin/env bash
# The gate. Builds the Rust staticlib for wasm32-unknown-emscripten and puts
# it on the SAME em++ line as libembedpdf.a, exactly the way
# packages/engine/runtime/build/compile.esm.sh:35 links the shipping artifact.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LIB_DIR="$ROOT/build/libpdfium/wasm32"
OUT_DIR="$ROOT/build/out"

# shellcheck disable=SC1091
source "$ROOT/build/emsdk.sh"

cargo build --manifest-path "$ROOT/Cargo.toml" \
  -p shim --target wasm32-unknown-emscripten --release

RUST_LIB="$ROOT/target/wasm32-unknown-emscripten/release/libshim.a"
test -f "$RUST_LIB" || { echo "missing $RUST_LIB" >&2; exit 1; }

mkdir -p "$OUT_DIR"
em++ "$LIB_DIR/lib/libembedpdf.a" "$RUST_LIB" \
  -sENVIRONMENT=node \
  -sMODULARIZE=1 \
  -sEXPORT_ES6=1 \
  -sWASM=1 \
  -sALLOW_MEMORY_GROWTH=1 \
  -sALLOW_TABLE_GROWTH=1 \
  -sEXPORT_NAME=createProbe \
  -sASSERTIONS=1 \
  -sEXPORTED_RUNTIME_METHODS=cwrap,getValue,setValue,UTF8ToString,UTF16ToString,stringToUTF8,wasmExports \
  -sEXPORTED_FUNCTIONS=_malloc,_free,_FPDF_InitLibrary,_FPDF_LoadMemDocument,_FPDF_CloseDocument,_EPDF_GetPageSizeByIndexNormalized,_rs_probe_page_size \
  -I"$LIB_DIR/include" \
  -std=c++17 --no-entry \
  -o "$OUT_DIR/probe.mjs"

echo "linked $OUT_DIR/probe.mjs"
