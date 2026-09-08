#!/usr/bin/env bash
# One em++ invocation: libembedpdf.a plus the Rust staticlib, into one module.
# This is recommendation 5 of docs/research/pdfium-rust-wrapper-decision.md, and
# pdfium-rust-spike/build/link-wasm.sh proved it four times over.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SPIKE="$ROOT/../pdfium-rust-spike"
LIB_DIR="$ROOT/build/libpdfium/wasm32-eh"
OUT_DIR="$ROOT/build/out/web"

bash "$ROOT/build/fetch-libpdfium.sh" wasm32-eh

if ! command -v em++ >/dev/null 2>&1; then
  if [[ -f "$SPIKE/build/emsdk.sh" ]]; then
    # shellcheck disable=SC1091
    source "$SPIKE/build/emsdk.sh"
    # emsdk.sh is sourced, not run in a subshell, and it declares its own
    # $ROOT (pdfium-rust-spike, not this scaffold) in the same scope, so
    # sourcing it clobbers ours. Every use of $ROOT below this line would
    # silently resolve inside pdfium-rust-spike instead: cargo would fail
    # with "package ID specification `epdf-wasm` did not match any
    # packages", and the em++ link would look for libepdf_wasm.a in the
    # wrong target directory. Recompute it.
    ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
  else
    echo "em++ not on PATH and no emsdk at $SPIKE/build/emsdk.sh" >&2
    exit 1
  fi
fi

# emsdk 3.1.72 bundles wasm-opt v119, which rejects two wasm feature names
# present in rustc 1.98.0's precompiled std. -O3 makes em++ run it, so a newer
# wasm-opt has to shadow the bundled one. pdfium-rust-spike/README.md documents
# the same substitution.
if ! wasm-opt --version 2>/dev/null | grep -qE 'version 1[2-9][0-9]'; then
  echo "warning: wasm-opt is older than v120; the -O3 link will fail." >&2
  echo "         install a newer binaryen and put it ahead of emsdk's on PATH." >&2
fi

# Not `rustup target add wasm32-unknown-emscripten` here: run from outside any
# subshell that has `cd`'d to $ROOT, that would resolve the toolchain from the
# caller's working directory and mutate the developer's DEFAULT toolchain,
# exactly the trap rust-toolchain.toml:3-8 documents. It is also redundant:
# rust-toolchain.toml already lists this target in `targets`, so cargo
# installs it for the pinned toolchain automatically on first use below.

# pdfium-sys's build.rs runs bindgen over the fork's headers for every target,
# including this one, and bindgen needs help for wasm32-unknown-emscripten
# specifically:
#
# 1. --target=wasm32-unknown-emscripten and -isystem<sysroot>/include: without
#    them libclang parses the headers against the host's default include
#    search paths, and PDFium's headers use <stdint.h> types, so parsing fails
#    outright with "unknown type name 'uint32_t'".
#
# 2. -fvisibility=default: with only (1) fixed, libclang parses cleanly (zero
#    diagnostics) but bindgen still emits zero functions for this target, with
#    no warning or error. bindgen's Function::parse
#    (bindgen-0.72.1/ir/function.rs:733) skips any cursor whose
#    clang_getCursorVisibility() isn't CXVisibility_Default, and under
#    --target=wasm32-unknown-emscripten libclang reports every FPDF_EXPORT
#    (__attribute__((visibility("default")))) function as non-default
#    visibility, so every single one is silently dropped. Confirmed with a
#    standalone bindgen harness against wrapper.h: 0 emitted `pub fn`s without
#    this flag, 764 with it (matching the native darwin-arm64 count exactly).
#    -fvisibility=default overrides whatever the wasm32-emscripten target
#    otherwise defaults to and restores normal ELF-style visibility semantics
#    for bindgen's parse.
#
# None of this touches crates/pdfium-sys: bindgen reads BINDGEN_EXTRA_CLANG_ARGS
# from the environment automatically (bindgen-0.72.1/lib.rs:257-265), so these
# three flags apply without a code change in a crate this task must not modify.
#
# The sysroot include path is asked of emscripten, not inferred from where
# em++ lives: `dirname "$(command -v em++)"` only resolves for an emsdk tree
# (.../emsdk/upstream/emscripten/em++ -> .../upstream/emscripten, which does
# hold cache/sysroot/include). For a Homebrew or system em++ on PATH --
# precisely the case the `if ! command -v em++` branch above exists to serve
# -- `command -v` returns e.g. /opt/homebrew/bin/em++, and the flag becomes
# -isystem/opt/homebrew/bin/cache/sysroot/include. clang ignores a
# nonexistent -isystem silently, so bindgen would then fail with an "unknown
# type name 'uint32_t'" cascade pointing at a cause that is actually already
# fixed by flag (1) above. `em-config CACHE` asks emscripten for its actual
# cache root and is correct in both cases.
# Not assigned to BINDGEN_EXTRA_CLANG_ARGS here as a script-scoped variable:
# sourcing this script (see line 17) would leave that variable sitting in the
# caller's shell even without `export`, since a sourced script's plain
# assignments persist in the sourcing shell regardless of export status. Built
# inline on the cargo command below instead, alongside RUSTFLAGS, so neither
# survives past that one command.
EM_SYSROOT_INCLUDE="$(em-config CACHE)/sysroot/include"

# rustc's precompiled std for wasm32-unknown-emscripten lowers a Rust panic's
# unwinding through the native WebAssembly exception-handling proposal: the
# compiled object imports `__cpp_exception` as a WASM TAG, which needs em++'s
# -fwasm-exceptions.
#
# This used to be impossible to satisfy. The fork's wasm32 libembedpdf.a is
# compiled with emscripten's JS setjmp/longjmp emulation (eight undefined
# references to `emscripten_longjmp`), needing -sSUPPORT_LONGJMP=emscripten,
# and em++ refuses that alongside -fwasm-exceptions: "SUPPORT_LONGJMP=
# emscripten is not compatible with -fwasm-exceptions". The workaround was to
# build epdf-wasm with `-C panic=abort`, which cost recoverability: a panic
# killed the whole instance.
#
# The fork now also builds wasm32-eh, the same source compiled with
# -sSUPPORT_LONGJMP=wasm, so both sides use the native mechanism and both
# flags agree. That is the artifact fetched above, and it is why there is no
# RUSTFLAGS on the cargo command below. PDFium's setjmp/longjmp use is real,
# not incidental: core/fxcodec/png/pngmodule.cpp and
# core/fxcodec/jpeg/jpeg_common.c use it for libpng and libjpeg error handling.
#
# `epdf_test_panic` is the manual check that a Rust panic reaches the JS caller
# as a catchable RuntimeError instead of killing the instance (README.md, "Five
# things a reader will otherwise be surprised by"). It is a probe, not an API,
# and crates/epdf-wasm/src/lib.rs:224 says it is "absent from a normal build",
# so it has to be opt-in here rather than always on:
#
#   EPDF_PANIC_PROBE=1 bash build/link-wasm.sh
CARGO_FEATURES=()
PROBE_EXPORT=""
if [[ "${EPDF_PANIC_PROBE:-0}" != "0" ]]; then
  CARGO_FEATURES=(--features panic-probe)
  PROBE_EXPORT=",_epdf_test_panic"
  echo "building WITH the panic probe: epdf_test_panic is exported" >&2
fi

( cd "$ROOT" \
  && BINDGEN_EXTRA_CLANG_ARGS="--target=wasm32-unknown-emscripten -isystem${EM_SYSROOT_INCLUDE} -fvisibility=default" \
     cargo build --release -p epdf-wasm "${CARGO_FEATURES[@]}" --target wasm32-unknown-emscripten )

EXPORTS="_malloc,_free\
,_epdf_open,_epdf_page_count,_epdf_json,_epdf_render,_epdf_free,_epdf_close,_epdf_last_error\
${PROBE_EXPORT}"

mkdir -p "$OUT_DIR"
em++ \
  "$LIB_DIR/lib/libembedpdf.a" \
  "$ROOT/target/wasm32-unknown-emscripten/release/libepdf_wasm.a" \
  -sENVIRONMENT=node,web,worker \
  -sMODULARIZE=1 \
  -sEXPORT_ES6=1 \
  -sWASM=1 \
  -fwasm-exceptions \
  -sSUPPORT_LONGJMP=wasm \
  -sALLOW_MEMORY_GROWTH=1 \
  -sALLOW_TABLE_GROWTH=1 \
  -sEXPORT_NAME=createEpdfModule \
  -sASSERTIONS=1 \
  -O3 \
  -sERROR_ON_UNDEFINED_SYMBOLS=0 \
  -sEXPORTED_RUNTIME_METHODS=cwrap,UTF8ToString,HEAPU8,HEAPU32 \
  -sEXPORTED_FUNCTIONS="$EXPORTS" \
  -I"$LIB_DIR/include" \
  -std=c++17 --no-entry \
  -o "$OUT_DIR/epdf.mjs"

echo "linked $OUT_DIR/epdf.wasm ($(wc -c < "$OUT_DIR/epdf.wasm") bytes)"
