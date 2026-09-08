#!/usr/bin/env bash
# Install and activate emsdk 3.1.72, matching packages/engine/runtime/scripts/dev.sh:7. Source this
# script rather than running it: it exports PATH entries em++ needs.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EMSDK_VERSION="3.1.72"
EMSDK_DIR="$ROOT/build/emsdk"

if [[ ! -d "$EMSDK_DIR/.git" ]]; then
  git clone https://github.com/emscripten-core/emsdk.git "$EMSDK_DIR"
fi
"$EMSDK_DIR/emsdk" install "$EMSDK_VERSION"
"$EMSDK_DIR/emsdk" activate "$EMSDK_VERSION"
# shellcheck disable=SC1091
source "$EMSDK_DIR/emsdk_env.sh"
