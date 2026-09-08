#!/usr/bin/env bash
# Builds libepdf_uniffi for the requested Apple targets, generates the Swift
# bindings, and stages macOS dylibs or iOS static libraries for SwiftPM/Xcode.
#
#   bash build/build-swift.sh            # macos-arm64 only, the default today
#   bash build/build-swift.sh ios-arm64  # exits 2 until embedpdf/runtime ships it
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SLICES=("${@:-macos-arm64}")

declare -A RUST_TARGET=(
  [macos-arm64]=aarch64-apple-darwin
  [ios-arm64]=aarch64-apple-ios
  [ios-sim-arm64]=aarch64-apple-ios-sim
)
declare -A FORK_TARGET=(
  [macos-arm64]=darwin-arm64
  [ios-arm64]=ios-arm64
  [ios-sim-arm64]=ios-sim-arm64
)

SWIFT_PKG="$ROOT/packages/swift/EpdfOps"
GENERATED="$SWIFT_PKG/Sources/EpdfOps/Generated"
FFI_TARGET="$SWIFT_PKG/Sources/EpdfOpsFFI"

for slice in "${SLICES[@]}"; do
  rust="${RUST_TARGET[$slice]:-}"
  fork="${FORK_TARGET[$slice]:-}"
  if [[ -z "$rust" ]]; then
    echo "unknown slice: $slice (known: ${!RUST_TARGET[*]})" >&2
    exit 1
  fi

  # Exits 2 with its own message for the pending mobile targets. Prints the
  # directory it populated: build/libpdfium/<target>, or EPDF_SCAFFOLD_LIB_DIR's
  # when that override is set, which is also where cargo (crates/build-support)
  # links from, so the copies below must come from the same place.
  lib_dir="$(bash "$ROOT/build/fetch-libpdfium.sh" "$fork")"

  rustup target add "$rust"
  ( cd "$ROOT" && EPDF_SCAFFOLD_TARGET="$fork" cargo build --release -p epdf-uniffi --target "$rust" )

  dest="$SWIFT_PKG/Frameworks/$slice"
  mkdir -p "$dest"
  if [[ "$slice" == macos-* ]]; then
    cp "$ROOT/target/$rust/release/libepdf_uniffi.dylib" "$dest/"
    cp "$lib_dir/lib/libembedpdf."* "$dest/"
    install_name_tool -id "@rpath/libepdf_uniffi.dylib" "$dest/libepdf_uniffi.dylib"
  else
    # Link into the app; remove the old generated dylib so ld cannot prefer it.
    cp "$ROOT/target/$rust/release/libepdf_uniffi.a" "$dest/"
    cp "$lib_dir/lib/libembedpdf.a" "$dest/"
    rm -f "$dest/libepdf_uniffi.dylib"
  fi
  echo "staged $slice -> $dest"
done

# Bindings are architecture-independent, so generate them once from whichever
# slice was built first.
first="${SLICES[0]}"
mkdir -p "$GENERATED" "$FFI_TARGET"
( cd "$ROOT" && cargo run --release --bin uniffi-bindgen -- generate \
    --library "$ROOT/target/${RUST_TARGET[$first]}/release/libepdf_uniffi.dylib" \
    --language swift \
    --out-dir "$GENERATED" )

# uniffi emits the .swift beside a C header and a modulemap. SwiftPM needs the
# latter two in their own systemLibrary target.
mv "$GENERATED"/epdf_uniffiFFI.h "$FFI_TARGET/"
mv "$GENERATED"/epdf_uniffiFFI.modulemap "$FFI_TARGET/module.modulemap"

echo "generated Swift bindings -> $GENERATED"
