#!/usr/bin/env bash
# usage: swift-linux.sh <checkout> <out-dir>
# Type-checks and tests platforms/apple/EmbedPDF on Linux: builds epdf-viewer-host
# as a shared library, generates its Swift bindings, and assembles a SwiftPM
# package of the bindings, the EmbedPDF sources and the EmbedPDFBindingsSmokeTests
# and EmbedPDFTests targets over stub modules for the Apple-only frameworks
# (Stubs/). EmbedPDFUI (SwiftUI) is left out. Every source file gets
# `import CoreGraphics` first, as Foundation re-exports it on Apple platforms.
set -uo pipefail
CHECKOUT="$1"; OUT="$2"; HERE="$(cd "$(dirname "$0")" && pwd)"
export PATH="/workspaces/bin:$PATH" CI=true
export CARGO_TARGET_DIR="$CHECKOUT/target" CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
LIBPDFIUM="$CHECKOUT/packages/engine/runtime/build/libpdfium"
export PDF_RUNTIME_LIB_DIR="$LIBPDFIUM" LD_LIBRARY_PATH="$LIBPDFIUM/linux-$(node -p process.arch)/lib:$CARGO_TARGET_DIR/debug"
step() { echo "=== $(date +%T) $*"; "$@" || { echo "FAILED: $*"; exit 1; }; }
cd "$CHECKOUT" || exit 2
echo "checkout $(git rev-parse HEAD)"
step cargo build --locked -p epdf-viewer-host -p uniffi-bindgen
LIB="$CARGO_TARGET_DIR/debug/libepdf_viewer_host.so"
rm -rf "$OUT"; mkdir -p "$OUT/gen"
P="$OUT/pkg"; A="$CHECKOUT/platforms/apple/EmbedPDF"
step "$CARGO_TARGET_DIR/debug/uniffi-bindgen" generate --library "$LIB" --language swift --out-dir "$OUT/gen"
mkdir -p "$P/Sources/EmbedPDFFFI" "$P/Sources/EmbedPDFBindings" "$P/Tests"
for map in "$OUT/gen"/*.modulemap; do cat "$map"; echo; done > "$P/Sources/EmbedPDFFFI/module.modulemap"
cp "$OUT/gen"/*.h "$P/Sources/EmbedPDFFFI/"
cp "$OUT/gen"/*.swift "$P/Sources/EmbedPDFBindings/"
cp -R "$HERE/Stubs/." "$P/Sources/"
cp -R "$A/Sources/EmbedPDF" "$P/Sources/EmbedPDF"
for t in EmbedPDFBindingsSmokeTests EmbedPDFTests; do cp -R "$A/Tests/$t" "$P/Tests/$t"; mkdir -p "$P/Tests/$t/Fixtures"; done
find "$P/Sources/EmbedPDF" "$P/Tests" -name "*.swift" -exec sed -i "1i import CoreGraphics" {} +
# The fixtures build-xcframework.sh copies into every test bundle, read from that script.
python3 - "$CHECKOUT/platforms/apple/build-xcframework.sh" > "$OUT/fixtures.txt" <<"PY"
import re, sys
s = open(sys.argv[1]).read()
for block in re.findall(r"FIXTURES(?:\+)?=\(\n(.*?)\n\)", s, re.S):
    for line in block.splitlines():
        line = line.strip()
        if line and not line.startswith("#"):
            print(line)
PY
for t in EmbedPDFBindingsSmokeTests EmbedPDFTests; do
  while read -r f; do cp $CHECKOUT/$f "$P/Tests/$t/Fixtures/"; done < "$OUT/fixtures.txt"
done
cat > "$P/Package.swift" <<SWIFT
// swift-tools-version: 6.2
import PackageDescription
// Swift Testing's _Testing_CoreGraphics overlay isn't in Linux toolchains; the stub would pull it in.
let noOverlays: [SwiftSetting] = [.unsafeFlags(["-Xfrontend", "-disable-cross-import-overlays"])]
let package = Package(
  name: "EmbedPDFLinux",
  dependencies: [.package(url: "https://github.com/pointfreeco/swift-custom-dump", from: "1.3.0")],
  targets: [
    .systemLibrary(name: "EmbedPDFFFI", path: "Sources/EmbedPDFFFI"),
    .target(name: "AppleCFStub", path: "Sources/AppleCFStub"),
    .target(name: "CoreGraphics", dependencies: ["AppleCFStub"], path: "Sources/CoreGraphics"),
    .target(name: "ImageIO", dependencies: ["CoreGraphics"], path: "Sources/ImageIO"),
    .target(name: "Security", dependencies: ["AppleCFStub"], path: "Sources/Security"),
    .target(name: "JavaScriptCore", path: "Sources/JavaScriptCore"),
    .target(name: "os", path: "Sources/os"),
    .target(
      name: "EmbedPDFBindings", dependencies: ["EmbedPDFFFI", "CoreGraphics"],
      swiftSettings: [.swiftLanguageMode(.v5)],
      linkerSettings: [.unsafeFlags(["-L$CARGO_TARGET_DIR/debug", "-lepdf_viewer_host", "-Xlinker", "-rpath", "-Xlinker", "$CARGO_TARGET_DIR/debug"])]),
    .target(name: "EmbedPDF", dependencies: ["EmbedPDFBindings", "CoreGraphics", "ImageIO", "Security", "JavaScriptCore", "os"]),
    .testTarget(name: "EmbedPDFBindingsSmokeTests", dependencies: ["EmbedPDFBindings", "CoreGraphics"], resources: [.copy("Fixtures")], swiftSettings: noOverlays),
    .testTarget(
      name: "EmbedPDFTests",
      dependencies: ["EmbedPDF", "CoreGraphics", "Security", .product(name: "CustomDump", package: "swift-custom-dump")],
      resources: [.copy("Fixtures"), .copy("Locales")], swiftSettings: noOverlays),
  ]
)
SWIFT
cd "$P"
swift build --build-tests > "$OUT/build.log" 2>&1; echo "swift build exit=$?" | tee "$OUT/build.exit"
grep -cE "error:" "$OUT/build.log" | sed "s/^/build errors: /"
if grep -q "exit=0" "$OUT/build.exit"; then
  swift test > "$OUT/test.log" 2>&1; echo "swift test exit=$?" | tee "$OUT/test.exit"
  grep -E "Test run with|Executed [0-9]+ tests" "$OUT/test.log" | tail -5
fi
echo "SWIFTLINUX DONE"
