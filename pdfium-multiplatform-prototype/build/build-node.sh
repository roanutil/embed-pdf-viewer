#!/usr/bin/env bash
# Builds the napi addon and drops it beside packages/node/index.mjs under the
# fork's target name, which is the same spelling packages/engine/runtime/npm
# uses. No @napi-rs/cli: a cdylib rename is the whole build.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="${1:-$(node -p "process.platform === 'darwin' ? 'darwin' : process.platform === 'win32' ? 'win32' : 'linux'")-$(node -p "process.arch === 'arm64' ? 'arm64' : 'x64'")}"

bash "$ROOT/build/fetch-libpdfium.sh" "$TARGET"
( cd "$ROOT" && EPDF_SCAFFOLD_TARGET="$TARGET" cargo build --release -p epdf-node )

case "$(uname -s)" in
  Darwin) ext=dylib ;;
  Linux)  ext=so ;;
  *)      echo "unsupported host: $(uname -s)" >&2; exit 1 ;;
esac

cp "$ROOT/target/release/libepdf_node.$ext" "$ROOT/packages/node/epdf-node.$TARGET.node"
echo "built packages/node/epdf-node.$TARGET.node"
