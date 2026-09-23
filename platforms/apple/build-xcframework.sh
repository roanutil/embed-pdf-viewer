#!/usr/bin/env bash
# Builds the Apple XCFramework and the Swift package around it.
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
# Produces platforms/apple/EmbedPDF/ : a Swift package whose one binary target
# is EmbedPDFFFI.xcframework, with device and simulator slices.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT="$ROOT/platforms/apple/EmbedPDF"
BUILD="$ROOT/platforms/apple/.build"
LIB_ROOT="$ROOT/packages/engine/runtime/build/libpdfium"
PROFILE="${PROFILE:-release}"

# Rust target -> fork target. The fork spells these the way Node does.
SLICES=(
  "aarch64-apple-ios:ios-arm64"
  "aarch64-apple-ios-sim:ios-sim-arm64"
)

rm -rf "$BUILD"
mkdir -p "$BUILD"

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
  (cd "$ROOT" && cargo build -p epdf-uniffi --target "$rust" --profile "$PROFILE")

  # One archive per slice, because `xcodebuild -create-xcframework` takes one
  # `-library` per platform. libtool merges rather than concatenates, which is
  # what keeps the duplicate-symbol check honest.
  mkdir -p "$BUILD/$fork"
  libtool -static -o "$BUILD/$fork/libEmbedPDFFFI.a" \
    "$ROOT/target/$rust/$PROFILE/libepdf_uniffi.a" \
    "$LIB_ROOT/$fork/lib/libembedpdf.a" 2>/dev/null
done

# The headers every slice shares. Generated from the built library rather than
# written by hand: uniffi's proc macros are the only thing that knows the
# current shape, and a hand-kept header drifts silently.
echo "generating bindings"
(cd "$ROOT" && cargo build -p uniffi-bindgen)
mkdir -p "$BUILD/headers" "$BUILD/swift"
"$ROOT/target/debug/uniffi-bindgen" generate \
  --library "$ROOT/target/aarch64-apple-ios-sim/$PROFILE/libepdf_uniffi.dylib" \
  --language swift --out-dir "$BUILD/generated" 2>/dev/null ||
  "$ROOT/target/debug/uniffi-bindgen" generate \
    --library "$ROOT/target/$PROFILE/libepdf_uniffi.dylib" \
    --language swift --out-dir "$BUILD/generated"

cp "$BUILD/generated"/*.h "$BUILD/headers/"
# xcframework wants the modulemap named module.modulemap inside the headers
# directory, not the generated epdf_uniffiFFI.modulemap beside it.
cp "$BUILD/generated"/*.modulemap "$BUILD/headers/module.modulemap"
cp "$BUILD/generated"/*.swift "$BUILD/swift/"

echo "creating the xcframework"
rm -rf "$OUT/EmbedPDFFFI.xcframework"
mkdir -p "$OUT/Sources/EmbedPDF"
xcodebuild -create-xcframework \
  -library "$BUILD/ios-arm64/libEmbedPDFFFI.a" -headers "$BUILD/headers" \
  -library "$BUILD/ios-sim-arm64/libEmbedPDFFFI.a" -headers "$BUILD/headers" \
  -output "$OUT/EmbedPDFFFI.xcframework" > /dev/null

cp "$BUILD/swift"/*.swift "$OUT/Sources/EmbedPDF/"

# The smoke test reads the same fixtures the Rust tests read. SwiftPM will only
# bundle resources that live inside the package, so they are copied rather than
# referenced, and `.gitignore`d rather than committed twice.
mkdir -p "$OUT/Tests/EmbedPDFSmokeTests/Fixtures"
for fixture in report colors encrypted-hello; do
  cp "$ROOT/pdfium-multiplatform-prototype/fixtures/$fixture.pdf" \
    "$OUT/Tests/EmbedPDFSmokeTests/Fixtures/"
done

cat > "$OUT/Package.swift" <<'SWIFT'
// swift-tools-version: 5.9
import PackageDescription

// The generated Swift and the binary it calls. `multiplatform-plan.md` is
// explicit that uniffi's output is not the public API: a handwritten facade
// belongs on top of this, in Swift, and does not exist yet. What ships here is
// the layer that facade will sit on.
let package = Package(
    name: "EmbedPDF",
    // 17.0 because that is the floor the fork's ios build is pinned to; see
    // ios_deployment_target in the fork's target-args.sh.
    platforms: [.iOS(.v17)],
    products: [
        .library(name: "EmbedPDF", targets: ["EmbedPDF"])
    ],
    targets: [
        .binaryTarget(name: "EmbedPDFFFI", path: "EmbedPDFFFI.xcframework"),
        .target(
            name: "EmbedPDF",
            dependencies: ["EmbedPDFFFI"],
            // PDFium is C++ and the slices are static archives, so the C++
            // runtime has to come from the SDK at link time.
            linkerSettings: [.linkedLibrary("c++")]
        ),
        .testTarget(
            name: "EmbedPDFSmokeTests",
            dependencies: ["EmbedPDF"],
            resources: [.copy("Fixtures")]
        ),
    ]
)
SWIFT

echo
echo "  $OUT/EmbedPDFFFI.xcframework"
xcodebuild -version | head -1
lipo -info "$OUT/EmbedPDFFFI.xcframework"/*/libEmbedPDFFFI.a 2>/dev/null | sed 's/^/  /'
