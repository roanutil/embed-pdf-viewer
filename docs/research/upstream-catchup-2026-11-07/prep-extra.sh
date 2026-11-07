#!/usr/bin/env bash
# usage: prep-extra.sh <checkout>  native addon + browsers for the TS suites
set -uo pipefail
cd "$1" || exit 2
export PATH="/workspaces/bin:$PATH" CI=true NPM_TOKEN=""
pnpm --filter @embedpdf/engine-runtime build:target linux-$(node -p process.arch) || echo "FAILED native addon"
pnpm --filter @embedpdf/tooling-viewer-e2e exec playwright install --with-deps chromium firefox webkit || echo "FAILED playwright"
echo "EXTRA DONE"
