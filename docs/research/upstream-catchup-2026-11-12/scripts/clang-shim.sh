#!/usr/bin/env bash
# Container-only shim: generate-functions.mjs parses the PDFium headers with the
# host clang. On Linux LP64, uint64_t resolves to "unsigned long" and the
# generator classifies it as i32; macOS resolves "unsigned long long" (i64).
# For the AST dump only, parse against the emscripten sysroot, where uint64_t is
# "unsigned long long" as on macOS.
for a in "$@"; do if [[ "$a" == -ast-dump=json ]]; then exec /usr/bin/clang --target=wasm32-unknown-emscripten --sysroot=/opt/emsdk/upstream/emscripten/cache/sysroot "$@"; fi; done
exec /usr/bin/clang "$@"
