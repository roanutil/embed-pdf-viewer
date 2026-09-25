#!/usr/bin/env bash
# Builds epdf-crux and regenerates both halves of the Swift surface:
# Crux's typegen (Serde/, EmbedPdfCrux/) and uniffi's FFI (epdf_cruxFFI/,
# EpdfCruxCore/). Needs the fork's darwin-arm64 prebuilt; point
# PDF_RUNTIME_LIB_DIR at it if it is not in this checkout.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../../../.." && pwd)"
sources="$here/Sources"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT

cd "$root"
cargo build -p epdf-crux
cargo run -q -p epdf-crux --features codegen --bin codegen -- swift "$scratch/typegen"
cargo run -q -p uniffi-bindgen -- generate --library target/debug/libepdf_crux.dylib \
  --language swift --out-dir "$scratch/uniffi"

rm -rf "$sources/Serde" "$sources/EmbedPdfCrux" "$sources/epdf_cruxFFI" "$sources/EpdfCruxCore"
cp -R "$scratch/typegen/EmbedPdfCrux/Sources/Serde" "$sources/Serde"
cp -R "$scratch/typegen/EmbedPdfCrux/Sources/EmbedPdfCrux" "$sources/EmbedPdfCrux"

mkdir -p "$sources/epdf_cruxFFI/include" "$sources/EpdfCruxCore"
cp "$scratch/uniffi/epdf_cruxFFI.h" "$sources/epdf_cruxFFI/include/"
cp "$scratch/uniffi/epdf_cruxFFI.modulemap" "$sources/epdf_cruxFFI/include/module.modulemap"
# SwiftPM wants at least one source file in a C target.
echo '// The header is the target; this file only satisfies SwiftPM.' > "$sources/epdf_cruxFFI/shim.c"
cp "$scratch/uniffi/epdf_crux.swift" "$sources/EpdfCruxCore/"
