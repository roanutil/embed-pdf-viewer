#!/usr/bin/env bash
# usage: build-rust-payload.sh <checkout>
# Builds the wasm32 payload with the Rust core: crates/epdf-wasm plus the fork's
# wasm32-eh PDFium, linked with emsdk 4.0.0 from /workspaces/emsdk4.
set -uo pipefail
cd "$1" || exit 2
# The image sets EM_BINARYEN_ROOT=/opt/binaryen (binaryen 132) and EM_CONFIG for its emsdk 3.1.72;
# emcc 4.0.0 expects its own binaryen (121), so drop both before sourcing emsdk 4.
unset EM_BINARYEN_ROOT EM_CONFIG
source /workspaces/emsdk4/emsdk_env.sh > /dev/null 2>&1
export PATH="/workspaces/bin:$PATH" CI=true NPM_TOKEN=""
export CARGO_TARGET_DIR="$1/target" CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PDF_RUNTIME_BUILD_FILE=/workspaces/wasm32-eh-pin.json
echo "emcc: $(emcc --version | head -1)"
echo "EM_BINARYEN_ROOT=${EM_BINARYEN_ROOT:-unset} EM_CONFIG=${EM_CONFIG:-unset} wasm-opt: $(command -v wasm-opt) $(wasm-opt --version 2>/dev/null)"
bash packages/engine/runtime/scripts/build-target.sh wasm32-eh || { echo "FAILED build-target wasm32-eh"; exit 1; }
echo "PAYLOAD DONE"
