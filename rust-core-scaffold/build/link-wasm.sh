#!/usr/bin/env bash
# One em++ invocation: libembedpdf.a plus the Rust staticlib, into one module.
# This is recommendation 5 of docs/research/pdfium-rust-wrapper-decision.md, and
# pdfium-rust-spike/build/link-wasm.sh proved it four times over.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SPIKE="$ROOT/../pdfium-rust-spike"
LIB_DIR="$ROOT/build/libpdfium/wasm32"
OUT_DIR="$ROOT/build/out/web"

bash "$ROOT/build/fetch-libpdfium.sh" wasm32

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

# epdf-ops uses std::panic::catch_unwind (crates/epdf-ops/src/dispatch/inline.rs
# and threaded.rs), which needs the panic strategy to be "unwind", and the root
# Cargo.toml deliberately leaves the workspace default at "unwind" rather than
# "abort" for exactly that reason. On every other target that link works. On
# wasm32-unknown-emscripten it cannot, for a reason specific to this pinned
# rustc/emsdk pair:
#
# rustc 1.98.0's precompiled std for wasm32-unknown-emscripten lowers a Rust
# panic's unwinding through LLVM's native WebAssembly exception-handling
# proposal: the compiled object references the symbol `__cpp_exception`, which
# libclang/nm report as a WASM TAG (not a function). Satisfying a tag import
# requires em++'s own exception-handling runtime, turned on with
# `-fwasm-exceptions` — verified: without it, Node's WebAssembly.instantiate
# refuses to load the module at all ("Import #31 "env" "__cpp_exception": tag
# import requires a WebAssembly.Tag"), and `-sERROR_ON_UNDEFINED_SYMBOLS=0`
# alone just defers that same failure from link time to instantiate time.
#
# But `-fwasm-exceptions` is flatly incompatible with the prebuilt
# libembedpdf.a fetched above: build-target.sh in the fork compiles it with
# emscripten's OLDER setjmp/longjmp emulation (`nm` on libembedpdf.a shows
# eight undefined references to `emscripten_longjmp`), which needs
# `-sSUPPORT_LONGJMP=emscripten` — and em++ hard-refuses that combination
# (emcc.py: "SUPPORT_LONGJMP=emscripten is not compatible with
# -fwasm-exceptions"), because the two mechanisms need mutually exclusive JS
# glue. There is no flag that satisfies both a prebuilt archive expecting JS
# SjLj and a Rust object expecting native wasm exceptions; recompiling either
# side is out of scope for this task (libembedpdf.a is fetched pre-built, and
# crates/epdf-ops is off limits).
#
# The only working option found: build epdf-wasm itself with `-C panic=abort`
# for this ONE target, which drops libpanic_unwind (and the `__cpp_exception`
# reference) in favor of libpanic_abort. This is scoped to the one cargo
# invocation below by putting RUSTFLAGS and BINDGEN_EXTRA_CLANG_ARGS on that
# command's own environment rather than exporting them into the script's
# (and, if this script is ever sourced instead of run, the caller shell's)
# environment: an `export` here would silently discard any RUSTFLAGS the
# caller already had set (say `-D warnings` or `-C target-cpu=native`, with
# no message that it was dropped), and if a developer sourced this script --
# this script's own precedent at line 17 above is that emsdk setup is
# something you source -- both variables would persist in their shell
# afterward, so a later native `cargo test` would silently run with
# panic=abort (a genuine panic would then abort the test binary instead of
# failing a test, which reads like a harness crash) and a later native
# `cargo build` would run bindgen with --target=wasm32-unknown-emscripten.
# Because --target is passed on this same command, RUSTFLAGS does not reach
# host build scripts, so crates/pdfium-sys's own bindgen build script (which
# runs against the host target, not this one) is unaffected either way.
#
# every other target keeps panic=unwind, since neither variable touches
# Cargo.toml. The real cost: on wasm32-unknown-emscripten specifically,
# `epdf_ops::dispatch::inline::Worker::run`'s `catch_unwind` can no longer
# actually catch anything -- a genuine Rust panic aborts the wasm instance
# (there is no `OpsError::Internal` to receive; the engine dies and the page
# must reload) rather than becoming one. The reentrancy guard itself is
# unaffected: its aliasing invariant rests on `try_borrow` refusing a second
# borrow while one is live, not on the `Drop`-releases-on-unwind property,
# and with no unwinding there is no stuck flag and no surviving instance to
# observe one -- see the SAFETY comment on `Worker` in
# crates/epdf-ops/src/dispatch/inline.rs. None of the four required tests
# trigger an actual Rust panic (they are all ordinary `Result::Err` returns),
# so this does not show up as a test failure; it is a real, silent gap
# between this target and the other three host boundaries, which this script
# cannot close without either a differently-built libembedpdf.a or a nightly
# `-Z build-std` (both out of scope here). Flagged in the task report; not
# fixed. A plain `cargo build -p epdf-wasm --target wasm32-unknown-emscripten`
# run after this script, without these flags, rebuilds with panic=unwind and
# produces an archive that cannot be linked -- with no hint that this
# script's flags were load-bearing to get here.
( cd "$ROOT" \
  && RUSTFLAGS="-C panic=abort" \
     BINDGEN_EXTRA_CLANG_ARGS="--target=wasm32-unknown-emscripten -isystem${EM_SYSROOT_INCLUDE} -fvisibility=default" \
     cargo build --release -p epdf-wasm --target wasm32-unknown-emscripten )

EXPORTS="_malloc,_free\
,_epdf_open,_epdf_page_count,_epdf_json,_epdf_render,_epdf_free,_epdf_close,_epdf_last_error"

mkdir -p "$OUT_DIR"
em++ \
  "$LIB_DIR/lib/libembedpdf.a" \
  "$ROOT/target/wasm32-unknown-emscripten/release/libepdf_wasm.a" \
  -sENVIRONMENT=node,web,worker \
  -sMODULARIZE=1 \
  -sEXPORT_ES6=1 \
  -sWASM=1 \
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
