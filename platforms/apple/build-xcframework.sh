#!/usr/bin/env bash
# Builds the Apple XCFramework and the generated Swift for the package around it.
#
# `multiplatform-plan.md` Phase 1: "Apple XCFramework in a Swift Package".
# Everything here is local; nothing is fetched and nothing is published. The
# libembedpdf slices come from whatever `scripts/fetch-libpdfium.sh` has put in
# `packages/engine/runtime/build/libpdfium/`, which for a local build means the
# fork's own `out/embedpdf-runtime-artifacts` registered through
# `scripts/runtime-use-local.sh`.
#
#   bash platforms/apple/build-xcframework.sh
#
# Produces the generated inputs of platforms/apple/EmbedPDF/ : the binary target
# EmbedPDFFFI.xcframework (device and simulator slices), the generated Swift in
# Sources/EmbedPDFBindings, and the test fixtures. Package.swift is tracked.
#
#   bash platforms/apple/build-xcframework.sh --experimental-manual-bridge
#
# builds an experimental XCFramework for Task 11's bridge benchmarks instead:
# the same archive plus the hand-written C bridge (crates/epdf-manual-bridge)
# and the benchmark control exports, with the `EpdfManualFFI` module in the
# modulemap. Use it with EMBEDPDF_EXPERIMENTAL_BRIDGE=1 (see Package.swift).
# Run the script again without the flag to get the shipping XCFramework back.
set -euo pipefail

EXPERIMENTAL=0
for arg in "$@"; do
  case "$arg" in
    --experimental-manual-bridge) EXPERIMENTAL=1 ;;
    *)
      echo "unknown argument: $arg" >&2
      exit 2
      ;;
  esac
done

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT="$ROOT/platforms/apple/EmbedPDF"
BUILD="$ROOT/platforms/apple/.build"
LIB_ROOT="${LIB_ROOT:-$ROOT/packages/engine/runtime/build/libpdfium}"
# pdfium-sys reads its headers and archive from PDF_RUNTIME_LIB_DIR
# (crates/build-support/src/lib.rs), so cargo links the same slices the
# check below looks at. Setting LIB_ROOT to the fork's staging root is how
# the release script builds without fetching.
export PDF_RUNTIME_LIB_DIR="$LIB_ROOT"
PROFILE="${PROFILE:-release}"
# Where cargo puts what it builds: CARGO_TARGET_DIR, a config's
# build.target-dir, or $ROOT/target. `cargo metadata` resolves all three.
TARGET_DIR="$(cd "$ROOT" && cargo metadata --manifest-path "$ROOT/Cargo.toml" --offline --locked --format-version 1 --no-deps \
  | sed -E 's/.*"target_directory":"([^"]*)".*/\1/')"

# The manual bridge links into epdf-viewer-host's own staticlib (one copy of
# the kernel thread's statics for both bridges), so the flag is only cargo
# features, never a second archive.
FEATURES=()
if [[ $EXPERIMENTAL == 1 ]]; then
  FEATURES=(--features "manual-bridge,epdf-ffi/bench")
fi

# Rust target -> fork target. The fork spells these the way Node does.
SLICES=(
  "aarch64-apple-ios:ios-arm64"
  "aarch64-apple-ios-sim:ios-sim-arm64"
  "aarch64-apple-darwin:darwin-static-arm64"
  "x86_64-apple-darwin:darwin-static-x64"
)

# Panic messages and debug info embed the source path of every crate, which
# for registry crates and the toolchain's std is under the builder's home
# directory (759 `$HOME` strings per slice before this). The published archive
# should not carry the builder's username, so rustc rewrites the prefixes. The
# repo root goes last because rustc tries the last matching mapping first.
# Appended to RUSTFLAGS rather than replacing it; no config in this repo sets
# rustflags, so nothing is overridden. Paths with spaces would need
# CARGO_ENCODED_RUSTFLAGS, which this does not handle.
REMAP="--remap-path-prefix=$HOME=~ --remap-path-prefix=$ROOT=/embedpdf-viewer"

rm -rf "$BUILD"
mkdir -p "$BUILD"

# Refuse partial symbol inventories: a skipped bitcode member can hide a
# second PDFium definition. A compatible owned tool may override Xcode's nm.
NM="${EMBEDPDF_NM:-nm}"
defined_symbols() {
  local symbols
  if ! symbols="$("$NM" -gUj "$1")"; then
    echo "symbol inspection failed for $1; set EMBEDPDF_NM to a compatible reviewed tool" >&2
    return 1
  fi
  # Public PDFium/EmbedPDF exports must have exactly one strong definition.
  # Rust/Swift weak or coalesced symbols are outside this export namespace.
  if ! printf '%s\n' "$symbols" | awk '
    /^_?(FPDF[A-Za-z0-9_]*|FORM_[A-Za-z0-9_]*|FSDK_[A-Za-z0-9_]*|EPDF_[A-Za-z0-9_]*)$/ {
      if (++seen[$0] > 1) duplicate = 1
    }
    END { exit duplicate ? 1 : 0 }
  '; then
    echo "duplicate PDFium definitions in $1" >&2
    return 1
  fi
  printf '%s\n' "$symbols" | grep -v -e ':$' -e '^$' | sort -u
}

for slice in "${SLICES[@]}"; do
  rust="${slice%%:*}"
  fork="${slice##*:}"
  if [[ ! -f "$LIB_ROOT/$fork/lib/libembedpdf.a" ]]; then
    echo "missing $LIB_ROOT/$fork/lib/libembedpdf.a" >&2
    echo "Fetch it first:" >&2
    echo "  cd packages/engine/runtime" >&2
    echo "  bash scripts/runtime-use-local.sh $fork <path to the fork's tarball>" >&2
    echo "  PDF_RUNTIME_BUILD_FILE=\$PWD/engine-runtime-build.local.json \\" >&2
    echo "    bash scripts/fetch-libpdfium.sh $fork" >&2
    exit 1
  fi

  echo "building $rust"
  case "$fork" in
    ios-arm64) sdk=iphoneos; clang_target=arm64-apple-ios17.0 ;;
    ios-sim-arm64) sdk=iphonesimulator; clang_target=arm64-apple-ios17.0-simulator ;;
    darwin-static-arm64) sdk=macosx; clang_target=arm64-apple-macosx14.0 ;;
    darwin-static-x64) sdk=macosx; clang_target=x86_64-apple-macosx14.0 ;;
  esac
  sdk_root="$(xcrun --sdk "$sdk" --show-sdk-path)"
  (cd "$ROOT" && EMBEDPDF_FORK_TARGET="$fork" SDKROOT="$sdk_root" \
    MACOSX_DEPLOYMENT_TARGET=14.0 IPHONEOS_DEPLOYMENT_TARGET=17.0 \
    BINDGEN_EXTRA_CLANG_ARGS="${BINDGEN_EXTRA_CLANG_ARGS:+$BINDGEN_EXTRA_CLANG_ARGS }--target=$clang_target -isysroot $sdk_root" \
    RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }$REMAP" \
    cargo build --manifest-path "$ROOT/Cargo.toml" --offline --locked \
      -j "${CARGO_BUILD_JOBS:-2}" -p epdf-viewer-host --target "$rust" --profile "$PROFILE" \
      ${FEATURES[@]+"${FEATURES[@]}"})

  # One archive per slice, because `xcodebuild -create-xcframework` takes one
  # `-library` per platform. pdfium-sys links libembedpdf as
  # `static=embedpdf`, which rustc bundles into the staticlib, so the Rust
  # archive already holds PDFium; merging libembedpdf.a again would add a
  # second copy of every member. It is merged only when the Rust archive
  # lacks PDFium's entry point (a build that links it `-bundle`).
  mkdir -p "$BUILD/$fork"
  host="$TARGET_DIR/$rust/$PROFILE/libepdf_viewer_host.a"
  inputs=("$host")
  # grep reads to the end (no -q), so nm never dies of SIGPIPE under pipefail.
  host_symbols="$(defined_symbols "$host")"
  if ! printf '%s\n' "$host_symbols" | grep -x '_FPDF_InitLibraryWithConfig' >/dev/null; then
    inputs+=("$LIB_ROOT/$fork/lib/libembedpdf.a")
    # libtool -static keeps a symbol two inputs both define without a word,
    # and the app's linker then takes whichever member it reaches first.
    # Refuse that here rather than ship two definitions.
    runtime_symbols="$(defined_symbols "${inputs[1]}")"
    dupes="$(comm -12 <(printf '%s\n' "$host_symbols") <(printf '%s\n' "$runtime_symbols"))"
    if [[ -n "$dupes" ]]; then
      echo "symbols defined in both $rust archives:" >&2
      echo "$dupes" | head -20 >&2
      exit 1
    fi
  fi
  # Rust's empty codegen units have no symbols, and libtool warns once per
  # member (about 76 lines); anything else it says is shown.
  libtool -static -no_warning_for_no_symbols -o "$BUILD/$fork/libEmbedPDFFFI.a" "${inputs[@]}"
done

# The headers every slice shares. Generated from the built library rather than
# written by hand: uniffi's proc macros are the only thing that knows the
# current shape, and a hand-kept header drifts silently. Library mode emits
# Swift for every crate in the archive that has scaffolding: epdf_uniffi,
# epdf_ffi, epdf_viewer_host and the plugins' ffi modules.
echo "generating bindings"
case "$(uname -m)" in
  arm64 | aarch64) host_fork=darwin-static-arm64 ;;
  x86_64) host_fork=darwin-static-x64 ;;
  *) echo "unsupported Apple build host" >&2; exit 1 ;;
esac
(cd "$ROOT" && EMBEDPDF_FORK_TARGET="$host_fork" \
  SDKROOT="$(xcrun --sdk macosx --show-sdk-path)" MACOSX_DEPLOYMENT_TARGET=14.0 \
  cargo build --manifest-path "$ROOT/Cargo.toml" --offline --locked \
    -j "${CARGO_BUILD_JOBS:-2}" -p uniffi-bindgen)
rm -rf "$BUILD/generated" "$BUILD/headers"
mkdir -p "$BUILD/headers"
generated_roots=()
for slice in "${SLICES[@]}"; do
  rust="${slice%%:*}"
  fork="${slice##*:}"
  generated="$BUILD/generated-$fork"
  mkdir -p "$generated"
  "$TARGET_DIR/debug/uniffi-bindgen" generate \
    --library "$TARGET_DIR/$rust/$PROFILE/libepdf_viewer_host.a" \
    --language swift --out-dir "$generated"
  generated_roots+=("$generated")
done
python3 "$ROOT/platforms/apple/check-native-graph.py" \
  --compare-generated "${generated_roots[@]}"
cp -R "$BUILD/generated-ios-sim-arm64" "$BUILD/generated"

# One *FFI.h and one *FFI.modulemap per crate. xcframework wants a single
# module.modulemap inside the headers directory, so the per-crate maps are
# concatenated into it. uniffi writes each map without a trailing newline, so
# a bare `cat` would run one map's closing brace into the next `module`.
for map in "$BUILD/generated"/*.modulemap; do
  cat "$map"
  echo
done > "$BUILD/headers/module.modulemap"
cp "$BUILD/generated"/*.h "$BUILD/headers/"

if [[ $EXPERIMENTAL == 1 ]]; then
  cp "$ROOT/crates/epdf-manual-bridge/include/epdf_manual.h" "$BUILD/headers/"
  cat >> "$BUILD/headers/module.modulemap" <<'MODULEMAP'
module EpdfManualFFI {
  header "epdf_manual.h"
  export *
}
MODULEMAP
fi

echo "creating the xcframework"
rm -rf "$OUT/EmbedPDFFFI.xcframework"
mkdir -p "$BUILD/macos-universal"
lipo -create "$BUILD/darwin-static-arm64/libEmbedPDFFFI.a" \
  "$BUILD/darwin-static-x64/libEmbedPDFFFI.a" \
  -output "$BUILD/macos-universal/libEmbedPDFFFI.a"
xcodebuild -create-xcframework \
  -library "$BUILD/ios-arm64/libEmbedPDFFFI.a" -headers "$BUILD/headers" \
  -library "$BUILD/ios-sim-arm64/libEmbedPDFFFI.a" -headers "$BUILD/headers" \
  -library "$BUILD/macos-universal/libEmbedPDFFFI.a" -headers "$BUILD/headers" \
  -output "$OUT/EmbedPDFFFI.xcframework" > /dev/null

mkdir -p "$OUT/Sources/EmbedPDFBindings"
rm -f "$OUT/Sources/EmbedPDFBindings"/*.swift
cp "$BUILD/generated"/*.swift "$OUT/Sources/EmbedPDFBindings/"

# The tests read the same fixtures the Rust tests read. SwiftPM will only
# bundle resources that live inside the package, so they are copied rather than
# referenced, and `.gitignore`d rather than committed twice.
TEST_TARGETS=(EmbedPDFBindingsSmokeTests EmbedPDFTests EmbedPDFUITests)
if [[ $EXPERIMENTAL == 1 ]]; then
  TEST_TARGETS+=(EmbedPDFBridgeBenchmarks)
fi
# Repo-relative files every test bundle gets, one per line. Each plugin plan
# appends its own after the last plan's lines. Entries are expanded
# unquoted, so a glob copies every match.
FIXTURES=(
  pdfium-multiplatform-prototype/fixtures/report.pdf
  pdfium-multiplatform-prototype/fixtures/colors.pdf
  pdfium-multiplatform-prototype/fixtures/encrypted-hello.pdf
  tooling/viewer-e2e/fixtures/features.pdf
  tooling/engine-conformance/fixtures/annotation_kinds.pdf
  tooling/engine-conformance/fixtures/tiny.png
  tooling/engine-conformance/fixtures/form_fields.pdf
  packages/default-stamps/en/stamps.pdf
  tooling/engine-conformance/fixtures/document_actions.pdf
  crates/test-support/fixtures/measure-foreign-viewport.pdf
  packages/engine/main/test/fixtures/signed_two_approvals.pdf
  packages/engine/main/test/fixtures/signed_certified.pdf
  packages/engine/main/test/fixtures/unsigned_sigfield.pdf
  packages/engine/main/test/fixtures/signature_artwork.pdf
  packages/engine/main/test/fixtures/signature-compat/keys/TEST-ONLY-*.pem
)
for tests in "${TEST_TARGETS[@]}"; do
  mkdir -p "$OUT/Tests/$tests/Fixtures"
  for fixture in "${FIXTURES[@]}"; do
    # shellcheck disable=SC2086
    cp "$ROOT"/$fixture "$OUT/Tests/$tests/Fixtures/"
  done
done

echo
echo "  $OUT/EmbedPDFFFI.xcframework"
xcodebuild -version | sed -n '1p'
lipo -info "$OUT/EmbedPDFFFI.xcframework"/*/libEmbedPDFFFI.a 2>/dev/null | sed 's/^/  /'
