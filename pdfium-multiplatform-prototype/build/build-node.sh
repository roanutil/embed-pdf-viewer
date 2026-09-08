#!/usr/bin/env bash
# Builds the napi addon and drops it beside packages/node/index.mjs under the
# fork's target name, which is the same spelling packages/engine/runtime/npm
# uses. No @napi-rs/cli: a cdylib rename is the whole build.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HOST_TARGET="$(node --input-type=module -e '
  import { pathToFileURL } from "node:url";
  const { resolveTarget } = await import(pathToFileURL(process.argv[1]));
  const target = resolveTarget();
  if (!target) throw new Error(`unsupported host: ${process.platform}-${process.arch}`);
  console.log(target);
' "$ROOT/packages/node/platform.mjs")"
TARGET="${1:-$HOST_TARGET}"
# This script builds for its native host. Reject mislabeled cross-builds before
# fetching anything; a Cargo cross-target needs its own linker/toolchain setup.
if [[ "$TARGET" != "$HOST_TARGET" ]]; then
  echo "target $TARGET does not match host $HOST_TARGET; run this script on the target host" >&2
  exit 1
fi

bash "$ROOT/build/fetch-libpdfium.sh" "$TARGET"
( cd "$ROOT" && EPDF_SCAFFOLD_TARGET="$TARGET" cargo build --release -p epdf-node )

case "$(uname -s)" in
  Darwin) ext=dylib ;;
  Linux)  ext=so ;;
  *)      echo "unsupported host: $(uname -s)" >&2; exit 1 ;;
esac

cp "$ROOT/target/release/libepdf_node.$ext" "$ROOT/packages/node/epdf-node.$TARGET.node"
echo "built packages/node/epdf-node.$TARGET.node"
