#!/usr/bin/env bash
# Build epdf-web for the browser (packages/core-wasm/wasm/) and for Node
# (packages/core-wasm/wasm-node/, which the package's vitest loads). Both
# directories are build output and gitignored.
set -euo pipefail
root="$(cd "$(dirname "$0")/../../.." && pwd)"
# Checked before the cargo build, which takes minutes and would otherwise
# end in a half-written wasm/ when wasm-opt turns out to be missing.
if ! command -v wasm-opt >/dev/null 2>&1; then
  echo "build.sh: wasm-opt not found on PATH. It comes from binaryen (version 133):" >&2
  echo "  brew install binaryen, or https://github.com/WebAssembly/binaryen/releases/tag/version_133" >&2
  exit 1
fi
cargo build -p epdf-web --release --target wasm32-unknown-unknown -j 4 --manifest-path "$root/Cargo.toml"
wasm="$root/target/wasm32-unknown-unknown/release/epdf_web.wasm"
wasm-bindgen --target web --out-dir "$root/packages/core-wasm/wasm" "$wasm"
wasm-bindgen --target experimental-nodejs-module --out-dir "$root/packages/core-wasm/wasm-node" "$wasm"
# wasm-bindgen runs no optimizer of its own. -O3 on its output, with the
# features the kernel uses (externref, bulk memory), and no debug names.
for out in "$root/packages/core-wasm/wasm/epdf_web_bg.wasm" "$root/packages/core-wasm/wasm-node/epdf_web_bg.wasm"; do
  wasm-opt -O3 --strip-debug --enable-reference-types --enable-bulk-memory --enable-mutable-globals \
    --enable-sign-ext --enable-nontrapping-float-to-int --enable-multivalue -o "$out" "$out"
done
