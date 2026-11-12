#!/usr/bin/env bash
# usage: build-rust-payload.sh <checkout>
# Builds the wasm32 payload with the Rust core: crates/epdf-wasm plus the fork's
# wasm32-eh PDFium, linked with emsdk 4.0.0 from /workspaces/emsdk4.
set -uo pipefail
cd "$1" || exit 2
source /workspaces/emsdk4/emsdk_env.sh > /dev/null 2>&1
export PATH="/workspaces/bin:$PATH" CI=true NPM_TOKEN=""
export CARGO_TARGET_DIR="$1/target" CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PDF_RUNTIME_BUILD_FILE=/workspaces/wasm32-eh-pin.json
echo "emcc: $(emcc --version | head -1)"
bash packages/engine/runtime/scripts/build-target.sh wasm32-eh || { echo "FAILED build-target wasm32-eh"; exit 1; }
echo "PAYLOAD DONE"
