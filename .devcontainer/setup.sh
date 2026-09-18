#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."
# Public dependencies need no publishing token; preserve one if supplied.
export NPM_TOKEN="${NPM_TOKEN:-}"

# Lifecycle hooks have no interactive terminal; allow pnpm to reconcile host modules.
CI=true pnpm install --frozen-lockfile

# Rust links the pinned fork directly; the TS reference uses the finished WASM payload.
runtime_target="$(node -p "'linux-' + process.arch")"
bash packages/engine/runtime/scripts/fetch-libpdfium.sh "$runtime_target"
bash packages/engine/runtime/scripts/fetch-libpdfium.sh wasm32-eh
bash packages/engine/runtime/scripts/fetch-payload.sh wasm32

pnpm --filter @embedpdf/engine-runtime build
pnpm --filter @embedpdf/engine-core build
pnpm --filter @embedpdf/engine-services build
pnpm --filter @embedpdf/engine build

# Browser versions follow the workspace's Playwright dependency.
pnpm --filter @embedpdf/tooling-viewer-e2e exec playwright install --with-deps chromium firefox webkit
cargo fetch --locked

printf '\nReady: cargo test -p epdf-engine; pnpm --filter @embedpdf/engine test\n'
