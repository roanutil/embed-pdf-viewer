#!/usr/bin/env bash
# usage: prep.sh <checkout-dir>   (installs deps, payload, TS builds) 
set -uo pipefail
cd "$1" || exit 2
export NPM_TOKEN="" CI=true
export CARGO_TARGET_DIR="$1/target"
step() { echo "=== $(date +%T) $*"; "$@" || { echo "FAILED: $*"; exit 1; }; }
step pnpm install --frozen-lockfile
step bash packages/engine/runtime/scripts/fetch-libpdfium.sh linux-$(node -p process.arch)
bash packages/engine/runtime/scripts/fetch-libpdfium.sh wasm32-eh || echo "skipped wasm32-eh"
if ! bash packages/engine/runtime/scripts/fetch-payload.sh wasm32; then
  echo "no published payload; building wasm32 without the Rust core (no wasm32-eh PDFium yet)"
  EMBEDPDF_RUST_CORE=0 step pnpm --filter @embedpdf/engine-runtime build:target wasm32
fi
for p in @embedpdf/engine-runtime @embedpdf/engine-core @embedpdf/engine-services @embedpdf/engine; do step pnpm --filter $p build; done
pnpm --filter @embedpdf/example-rust-kernel-eval bootstrap || echo "kernel-eval bootstrap failed (non-fatal)"
step cargo fetch --locked
echo "PREP DONE"
