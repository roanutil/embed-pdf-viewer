#!/usr/bin/env bash
# Link the wasm32 artifacts: ONE shared embedpdf.wasm + three environment-pure
# JS glues around it.
#
#   embedpdf.browser.js  ESM  -sENVIRONMENT=web,worker  (zero Node branches)
#   embedpdf.node.js     ESM  -sENVIRONMENT=node
#   embedpdf.node.cjs    CJS  -sENVIRONMENT=node
#
# Environment forks live in the package's export conditions, NOT in a
# universal loader: a universal glue's unused Node branches import Node
# builtins, which strict browser bundlers (Angular/esbuild) refuse to resolve
# — Vite merely papers over it with stubs. Limiting -sENVIRONMENT removes the
# excluded environments' support code at the source.
#
# Each glue is linked in its own temp dir as `embedpdf.{js,cjs}` so all of them
# reference the SAME wasm basename (`embedpdf.wasm`, resolved relative to the
# glue via import.meta.url / __dirname); the wasm outputs are verified
# identical and published once.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="${PDF_RUNTIME_TARGET:-wasm32}"
LIB_DIR="$ROOT/build/libpdfium/$TARGET"
# Overridable so a variant can be linked and inspected without replacing the
# payload every test on the machine loads. The default is the shipping one.
OUT_DIR="${PDF_RUNTIME_WASM_OUT_DIR:-$ROOT/npm/wasm32/lib}"
GEN_DIR="$ROOT/build/generated"

# wasm32-eh links the archive the fork compiled with native WebAssembly
# exception handling, so the link has to agree with it. em++ refuses
# -fwasm-exceptions together with SUPPORT_LONGJMP=emscripten, which is what
# PDFium's libpng and libjpeg error handling would otherwise select, and emsdk
# 4.0.0 still defaults -fwasm-exceptions to the legacy encoding that Firefox
# never shipped. All three flags are the fork's, in
# patches/embedpdf-runtime/wasm/config-eh.gn; they are repeated here because a
# static archive carries no link settings with it.
#
# The Rust core needs this variant: rustc's precompiled std for
# wasm32-unknown-emscripten imports __cpp_exception as a wasm tag.
EH_FLAGS=()
if [[ "$TARGET" == "wasm32-eh" ]]; then
  EH_FLAGS=(-fwasm-exceptions -sSUPPORT_LONGJMP=wasm -sWASM_LEGACY_EXCEPTIONS=0)
fi

# The Rust core. `scripts/build-target.sh` builds it and sets this, so a
# normal build always has it; calling this script directly without it links
# PDFium alone, which is what a bisect or a payload-size comparison wants.
#
#   cargo build -p epdf-wasm --target wasm32-unknown-emscripten --release
#   EMBEDPDF_RUST_STATICLIB=target/wasm32-unknown-emscripten/release/libepdf_wasm.a \
#     PDF_RUNTIME_TARGET=wasm32-eh bash build/compile.esm.sh
#
# Rust and PDFium end up in ONE module, which is the whole point: a page
# pointer is an offset into one linear memory, so the Stage A borrow costs
# nothing to cross. Two modules joined by JavaScript would be two heaps and a
# copy per call.
RUST_INPUTS=()
RUST_EXPORTS=""
if [[ -n "${EMBEDPDF_RUST_STATICLIB:-}" ]]; then
  if [[ ! -f "$EMBEDPDF_RUST_STATICLIB" ]]; then
    echo "EMBEDPDF_RUST_STATICLIB is set but not a file: $EMBEDPDF_RUST_STATICLIB" >&2
    exit 1
  fi
  RUST_INPUTS=("$EMBEDPDF_RUST_STATICLIB")
  # Every #[no_mangle] export of crates/epdf-wasm. Listed rather than derived:
  # a typo here links fine and fails at the first call, so it should be a
  # diff somebody reads.
  RUST_EXPORTS=",_epdf_page_geometry,_epdf_buffer_free"
  RUST_EXPORTS+=",_epdf_host_create,_epdf_host_call,_epdf_host_destroy"
fi

mkdir -p "$OUT_DIR"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

# WASM_BIGINT=0: src/wasm/wasm-runtime.ts splits each i64 argument into two
# i32 halves (expandI64Args), which only matches a build without BigInt
# integration. emcc 4.0.0, which the wasm32-eh build needs, turns it on by
# default; emsdk 3.1.72 has it off.
# args: <out-file> <environment> [extra flags...]
link() {
  local out="$1" environment="$2"
  shift 2
  em++ "$LIB_DIR/lib/libembedpdf.a" \
    "${RUST_INPUTS[@]+"${RUST_INPUTS[@]}"}" \
    -sENVIRONMENT="$environment" \
    -sMODULARIZE=1 \
    -sWASM=1 \
    -sALLOW_MEMORY_GROWTH=1 \
    -sALLOW_TABLE_GROWTH=1 \
    -sEXPORT_NAME=createPdfRuntimeWasm \
    -sWASM_BIGINT=0 \
    -O2 \
    -sASSERTIONS=1 \
    -sEXPORTED_RUNTIME_METHODS="$(cat "$GEN_DIR/exported-runtime-methods.txt")" \
    -sEXPORTED_FUNCTIONS="$(cat "$GEN_DIR/exported-functions.txt")$RUST_EXPORTS" \
    -I"$LIB_DIR/include" \
    -std=c++17 \
    -Wall \
    --no-entry \
    "${EH_FLAGS[@]+"${EH_FLAGS[@]}"}" \
    "$@" \
    -o "$out"
}

mkdir -p "$TMP_DIR/browser" "$TMP_DIR/node-esm" "$TMP_DIR/node-cjs"
link "$TMP_DIR/browser/embedpdf.js" "web,worker" -sEXPORT_ES6=1
link "$TMP_DIR/node-esm/embedpdf.js" "node" -sEXPORT_ES6=1
link "$TMP_DIR/node-cjs/embedpdf.cjs" "node"

# The wasm must be environment-independent (ENVIRONMENT only shapes the JS
# glue). Verify, then publish one copy every glue's `embedpdf.wasm` reference
# resolves to.
cmp "$TMP_DIR/browser/embedpdf.wasm" "$TMP_DIR/node-esm/embedpdf.wasm"
cmp "$TMP_DIR/browser/embedpdf.wasm" "$TMP_DIR/node-cjs/embedpdf.wasm"

# pre-split universal glues + pre-rename pdfium.* outputs
rm -f "$OUT_DIR/embedpdf.js" "$OUT_DIR/embedpdf.cjs" "$OUT_DIR"/pdfium.*
cp "$TMP_DIR/browser/embedpdf.js" "$OUT_DIR/embedpdf.browser.js"
cp "$TMP_DIR/node-esm/embedpdf.js" "$OUT_DIR/embedpdf.node.js"
cp "$TMP_DIR/node-cjs/embedpdf.cjs" "$OUT_DIR/embedpdf.node.cjs"
cp "$TMP_DIR/browser/embedpdf.wasm" "$OUT_DIR/embedpdf.wasm"

# Every bound function's parameter count against the bindings generated for
# it. Without ASSERTIONS a mismatch would read wrong arguments, not abort.
node "$ROOT/build/verify-wasm-abi.mjs" "$OUT_DIR/embedpdf.wasm" "$GEN_DIR/functions.json"
