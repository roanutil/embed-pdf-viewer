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
# wasm32-eh is still "status": "pending" in the fork, so the published pin has no
# such artifact and the fetch exits 2. Same skip as build/test-all.sh.
bash packages/engine/runtime/scripts/fetch-libpdfium.sh wasm32-eh \
  || echo "skipped: wasm32-eh is pending in engine-runtime-build.json" >&2
bash packages/engine/runtime/scripts/fetch-payload.sh wasm32

pnpm --filter @embedpdf/engine-runtime build
pnpm --filter @embedpdf/engine-core build
pnpm --filter @embedpdf/engine-services build
pnpm --filter @embedpdf/engine build

# Browser versions follow the workspace's Playwright dependency.
pnpm --filter @embedpdf/tooling-viewer-e2e exec playwright install --with-deps chromium firefox webkit
cargo fetch --locked

printf '\nReady: cargo test -p epdf-engine; pnpm --filter @embedpdf/engine test\n'
