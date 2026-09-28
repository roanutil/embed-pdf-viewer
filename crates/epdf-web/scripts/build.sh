#!/usr/bin/env bash
# Build epdf-web for the browser (packages/core-wasm/wasm/) and for Node
# (packages/core-wasm/wasm-node/, which the package's vitest loads). Both
# directories are build output and gitignored.
set -euo pipefail
root="$(cd "$(dirname "$0")/../../.." && pwd)"
cargo build -p epdf-web --release --target wasm32-unknown-unknown -j 4 --manifest-path "$root/Cargo.toml"
wasm="$root/target/wasm32-unknown-unknown/release/epdf_web.wasm"
wasm-bindgen --target web --out-dir "$root/packages/core-wasm/wasm" "$wasm"
wasm-bindgen --target experimental-nodejs-module --out-dir "$root/packages/core-wasm/wasm-node" "$wasm"
