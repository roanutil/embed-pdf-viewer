#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="${1:-}"

if [[ -z "$TARGET" ]]; then
  echo "usage: $0 <target>" >&2
  exit 1
fi

"$ROOT/scripts/fetch-libpdfium.sh" "$TARGET"
node "$ROOT/build/generate-functions.mjs" "$TARGET"
node "$ROOT/build/generate-runtime-methods.mjs" "$ROOT/build/generated"

# Prefix match, not equality: wasm32-eh is the same link with WebAssembly
# exception handling, and every later wasm variant should land here too.
if [[ "$TARGET" == wasm32* ]]; then
  # The Rust engine core goes into the payload. It used to be opt-in, because
  # the shipping payload was PDFium alone until the Stage A routing was
  # proven; the routing is proven now (every wire kind reaches an
  # implementation, gated by crates/epdf-engine/tests/wire_contract.rs), so
  # the default is the other way round.
  #
  # Required rather than best-effort. A build that quietly produced a payload
  # without the core would leave every Rust path silently falling back to
  # TypeScript, which is indistinguishable from the port not having happened.
  # EMBEDPDF_RUST_CORE=0 opts out, for someone building the runtime without a
  # Rust toolchain.
  if [[ "${EMBEDPDF_RUST_CORE:-1}" != "0" && -z "${EMBEDPDF_RUST_STATICLIB:-}" ]]; then
    if ! command -v cargo >/dev/null 2>&1; then
      echo "building $TARGET needs cargo for the Rust engine core." >&2
      echo "Install Rust, or set EMBEDPDF_RUST_CORE=0 to build without it." >&2
      exit 1
    fi
    REPO_ROOT="$(cd "$ROOT/../../.." && pwd)"
    (cd "$REPO_ROOT" && cargo build -p epdf-wasm --target wasm32-unknown-emscripten --release)
    EMBEDPDF_RUST_STATICLIB="$REPO_ROOT/target/wasm32-unknown-emscripten/release/libepdf_wasm.a"
    export EMBEDPDF_RUST_STATICLIB
  fi
  PDF_RUNTIME_TARGET="$TARGET" bash "$ROOT/build/compile.esm.sh"
  exit 0
fi

node "$ROOT/build/generate-napi-binding.mjs" "$TARGET"

case "$TARGET" in
  darwin-arm64|linux-arm64|linuxmusl-arm64|win32-arm64)
    ARCH="arm64"
    ;;
  darwin-x64|linux-x64|linuxmusl-x64|win32-x64)
    ARCH="x64"
    ;;
  *)
    echo "unknown native target: $TARGET" >&2
    exit 1
    ;;
esac

LIB_DIR="$ROOT/build/libpdfium/$TARGET"
cmake-js compile \
  --directory "$ROOT/build" \
  --arch "$ARCH" \
  --CDPDF_RUNTIME_TARGET="$TARGET" \
  --CDPDFIUM_LIB_DIR="$LIB_DIR/lib" \
  --CDPDFIUM_INCLUDE_DIR="$LIB_DIR/include"

# Replace, never overwrite: `cp` onto an existing binary rewrites the SAME
# inode, and the macOS kernel's per-inode code-signature cache then kills
# the next process to dlopen it (SIGKILL: Code Signature Invalid) even
# though `codesign -vv` reports the file as valid. Copying to a temp name
# and renaming lands the bytes on a fresh inode, atomically.
install_binary() {
  local src="$1" dst="$2"
  cp "$src" "$dst.tmp"
  mv -f "$dst.tmp" "$dst"
}

mkdir -p "$ROOT/npm/$TARGET/lib"
rm -f \
  "$ROOT/npm/$TARGET/lib/libpdfium.dylib" \
  "$ROOT/npm/$TARGET/lib/libpdfium.so" \
  "$ROOT/npm/$TARGET/lib/pdfium.dll"
install_binary "$ROOT/build/build/Release/pdf-runtime.node" "$ROOT/npm/$TARGET/lib/pdf-runtime.node"
case "$TARGET" in
  win32-*)
    install_binary "$LIB_DIR/bin/embedpdf.dll" "$ROOT/npm/$TARGET/lib/embedpdf.dll"
    ;;
  darwin-*)
    install_binary "$LIB_DIR/lib/libembedpdf.dylib" "$ROOT/npm/$TARGET/lib/libembedpdf.dylib"
    ;;
  linux-*)
    if [[ -f "$LIB_DIR/lib/libembedpdf.so" ]]; then
      install_binary "$LIB_DIR/lib/libembedpdf.so" "$ROOT/npm/$TARGET/lib/libembedpdf.so"
    fi
    ;;
esac
