#!/usr/bin/env bash
# usage: prep.sh <checkout-dir>   (installs deps, payload, TS builds) 
set -uo pipefail
cd "$1" || exit 2
export NPM_TOKEN="" CI=true
export CARGO_TARGET_DIR="$1/target" CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
# clang shim (see clang-shim.sh): header AST parsed against the emscripten sysroot
export PATH="/workspaces/bin:$PATH"
step() { echo "=== $(date +%T) $*"; "$@" || { echo "FAILED: $*"; exit 1; }; }
step pnpm install --frozen-lockfile
step bash packages/engine/runtime/scripts/fetch-libpdfium.sh linux-$(node -p process.arch)
PDF_RUNTIME_BUILD_FILE=/workspaces/wasm32-eh-pin.json bash packages/engine/runtime/scripts/fetch-libpdfium.sh wasm32-eh || echo "skipped wasm32-eh"
if ! bash packages/engine/runtime/scripts/fetch-payload.sh wasm32; then
  echo "no published payload; building wasm32 without the Rust core (no wasm32-eh PDFium yet)"
  EMBEDPDF_RUST_CORE=0 step pnpm --filter @embedpdf/engine-runtime build:target wasm32
fi
# build every workspace package (dependencies included) so the TS suites and generators resolve
step pnpm exec turbo run build --filter='./packages/**' --filter='./tooling/**' --filter='!@mfb/embedpdf-rust-viewer' --concurrency=6
pnpm --filter @embedpdf/example-rust-kernel-eval bootstrap || echo "kernel-eval bootstrap failed (non-fatal)"
step cargo fetch --locked
echo "PREP DONE"
