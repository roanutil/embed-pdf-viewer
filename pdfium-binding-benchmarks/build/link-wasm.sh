#!/usr/bin/env bash
# Builds all four wasm arms from ONE em++ invocation each, with identical
# flags. That identity is the whole point: differing flags would mean
# comparing builds rather than comparing wrappers.
#
#   a  libembedpdf.a alone                      (today)
#   b  + libshim.a, one rs_* per PDFium call    (the tax)
#   c  + libops.a, four coarse operations       (the benefit)
#   d  + ops.cc, the same four in C++           (the control)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LIB_DIR="$ROOT/build/libpdfium/wasm32"
OUT_DIR="$ROOT/build/out"
ARMS="${*:-a b c d}"

# shellcheck disable=SC1091
source "$ROOT/build/emsdk.sh"

PDFIUM_EXPORTS="_malloc,_free\
,_FPDF_InitLibrary,_FPDF_DestroyLibrary,_FPDF_LoadMemDocument,_FPDF_CloseDocument\
,_FPDF_GetPageCount,_FPDF_LoadPage,_FPDF_ClosePage,_FPDF_GetPageWidthF,_FPDF_GetPageHeightF\
,_EPDF_GetPageSizeByIndexNormalized\
,_FPDFText_LoadPage,_FPDFText_ClosePage,_FPDFText_CountChars,_FPDFText_GetTextObject\
,_FPDFText_GetFontSize,_EPDFText_GetCharGeometry,_EPDFText_GetTextFull\
,_FPDFText_FindStart,_FPDFText_FindNext,_FPDFText_GetSchResultIndex,_FPDFText_GetSchCount\
,_FPDFText_FindClose,_FPDFText_GetRect,_FPDFText_CountRects\
,_FPDFBitmap_CreateEx,_FPDFBitmap_FillRect,_FPDFBitmap_Destroy,_FPDF_RenderPageBitmap"

SHIM_EXPORTS="_rs_probe_page_size\
,_rs_FPDFText_CountChars,_rs_FPDFText_GetTextObject,_rs_FPDFText_GetFontSize\
,_rs_EPDFText_GetCharGeometry,_rs_EPDFText_GetTextFull\
,_rs_FPDFText_FindStart,_rs_FPDFText_FindNext,_rs_FPDFText_GetSchResultIndex\
,_rs_FPDFText_GetSchCount,_rs_FPDFText_FindClose,_rs_FPDFText_CountRects,_rs_FPDFText_GetRect\
,_rs_FPDFBitmap_FillRect,_rs_FPDF_RenderPageBitmap,_rs_EPDF_GetPageSizeByIndexNormalized"

OPS_EXPORTS="_rs_read_page_geometry,_rs_read_page_text,_rs_render_page,_rs_search_page"
CPP_EXPORTS="_cc_read_page_geometry,_cc_read_page_text,_cc_render_page,_cc_search_page"

link() {
  local arm="$1"; shift
  local exports="$1"; shift
  mkdir -p "$OUT_DIR/$arm"
  # -O3 is a deliberate departure from packages/engine/runtime/build/compile.esm.sh,
  # which carries no -O flag at all (so em++ defaults to -O0). That default is
  # harmless there because that script compiles no C++ of its own; it only links
  # a prebuilt libembedpdf.a, so -O0 touches nothing but the JS glue. Here, arm d
  # compiles cpp/ops.cc from source, and arm c's Rust is already built with
  # `cargo build --release` (opt-level = 3, lto = true, codegen-units = 1). Linking
  # arm d at -O0 against that would hand arm c an unearned win on any workload that
  # exercises ops.cc, so -O3 goes in this one shared link() body, applied identically
  # to all four arms, rather than as a per-arm flag.
  em++ "$LIB_DIR/lib/libembedpdf.a" "$@" \
    -sENVIRONMENT=node \
    -sMODULARIZE=1 \
    -sEXPORT_ES6=1 \
    -sWASM=1 \
    -sALLOW_MEMORY_GROWTH=1 \
    -sALLOW_TABLE_GROWTH=1 \
    -sEXPORT_NAME=createArm \
    -sASSERTIONS=1 \
    -O3 \
    -sEXPORTED_RUNTIME_METHODS=cwrap,getValue,setValue,UTF8ToString,UTF16ToString,stringToUTF8,stringToUTF16,wasmExports \
    -sEXPORTED_FUNCTIONS="$exports" \
    -I"$LIB_DIR/include" \
    -std=c++17 --no-entry \
    -o "$OUT_DIR/$arm/arm.mjs"
  echo "linked arm $arm -> $OUT_DIR/$arm/arm.wasm ($(wc -c < "$OUT_DIR/$arm/arm.wasm") bytes)"
}

build_rust() {
  cargo build --manifest-path "$ROOT/Cargo.toml" \
    -p "$1" --target wasm32-unknown-emscripten --release
}

for arm in $ARMS; do
  case "$arm" in
    a) link a "$PDFIUM_EXPORTS" ;;
    b) build_rust shim
       link b "$PDFIUM_EXPORTS,$SHIM_EXPORTS" \
         "$ROOT/target/wasm32-unknown-emscripten/release/libshim.a" ;;
    c) build_rust ops
       link c "$PDFIUM_EXPORTS,$OPS_EXPORTS" \
         "$ROOT/target/wasm32-unknown-emscripten/release/libops.a" ;;
    d) link d "$PDFIUM_EXPORTS,$CPP_EXPORTS" "$ROOT/cpp/ops.cc" ;;
    *) echo "unknown arm: $arm" >&2; exit 1 ;;
  esac
done
