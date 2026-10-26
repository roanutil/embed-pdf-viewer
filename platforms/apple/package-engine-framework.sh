#!/usr/bin/env bash
# Packages the iOS engine as a binary framework a Swift package can download.
#
# Runs after build-xcframework.sh and reads what it left in .build/: each
# slice's merged archive (Rust core and PDFium), the uniffi-generated Swift,
# and the C headers that Swift imports. Per slice, the Swift is compiled as one
# library-evolution module, EmbedPDFBindings, and its objects join the archive
# in a static EmbedPDFBindings.framework. The two frameworks become one
# XCFramework, zipped for a GitHub release.
#
#   bash platforms/apple/build-xcframework.sh
#   bash platforms/apple/package-engine-framework.sh
#
# Writes .build/publish/EmbedPDFBindings.xcframework.zip and prints its SwiftPM
# checksum on the last line. The unzipped XCFramework is also copied to
# EmbedPDF/EmbedPDFBindings.xcframework, where Package.swift can point a local
# binary target at it.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
APPLE="$ROOT/platforms/apple"
BUILD="$APPLE/.build"
WORK="$BUILD/package"
PUBLISH="$BUILD/publish"
NAME=EmbedPDFBindings
# Package.swift's `.iOS(.v17)`.
MIN_IOS=17.0

# Fork target : Swift target triple : SDK. The fork targets name the slice
# directories build-xcframework.sh writes.
SLICES=(
  "ios-arm64:arm64-apple-ios$MIN_IOS:iphoneos"
  "ios-sim-arm64:arm64-apple-ios$MIN_IOS-simulator:iphonesimulator"
)

for slice in "${SLICES[@]}"; do
  fork="${slice%%:*}"
  if [[ ! -f "$BUILD/$fork/libEmbedPDFFFI.a" ]]; then
    echo "missing $BUILD/$fork/libEmbedPDFFFI.a" >&2
    echo "Build it first: bash platforms/apple/build-xcframework.sh" >&2
    exit 1
  fi
done
if ! compgen -G "$BUILD/generated/*.swift" >/dev/null \
  || [[ ! -f "$BUILD/headers/module.modulemap" ]]; then
  echo "missing the generated Swift or headers under $BUILD" >&2
  echo "Build them first: bash platforms/apple/build-xcframework.sh" >&2
  exit 1
fi

rm -rf "$WORK" "$PUBLISH"
mkdir -p "$WORK" "$PUBLISH"

frameworks=()
for slice in "${SLICES[@]}"; do
  fork="${slice%%:*}"
  rest="${slice#*:}"
  triple="${rest%%:*}"
  sdk="${rest##*:}"
  echo "compiling $NAME for $triple"

  # uniffi guards each C import as `#if canImport(<crate>FFI)` / `import
  # <crate>FFI`. A plain import would land in the .swiftinterface, and every
  # client would then need these headers on its own search path. `internal
  # import` keeps them out.
  #
  # That leaves the free `FfiConverterType<T>_lift` and `_lower` functions,
  # which uniffi makes public so another crate's Swift module can convert this
  # crate's records, and whose signatures take or return the C `RustBuffer`.
  # A public declaration can't use an internally imported type. Every crate
  # compiles into this one module, so nothing outside it calls them, and
  # dropping `public` makes them internal. The `UInt64` handle variants have
  # no C type in them and stay as uniffi wrote them.
  src="$WORK/$fork/src"
  mkdir -p "$src"
  for swift in "$BUILD/generated"/*.swift; do
    sed -E \
      -e 's/^import ([A-Za-z0-9_]+FFI)$/internal import \1/' \
      -e 's/^public (func FfiConverter[A-Za-z0-9_]+_(lift|lower)\(.*RustBuffer)/\1/' \
      "$swift" > "$src/$(basename "$swift")"
  done

  fw="$WORK/$fork/$NAME.framework"
  module="$fw/Modules/$NAME.swiftmodule"
  # Inside a .swiftmodule directory each file is named for the triple without
  # its OS version, the way Xcode names them.
  arch="${triple/$MIN_IOS/}"
  mkdir -p "$module"
  # Swift 5 language mode, as the source target it replaces. Library evolution
  # is what makes the .swiftinterface, which a newer compiler than this one can
  # still read.
  xcrun --sdk "$sdk" swiftc \
    -parse-as-library -module-name "$NAME" -swift-version 5 \
    -enable-library-evolution -emit-module-interface -emit-module \
    -emit-library -static -target "$triple" \
    -sdk "$(xcrun --sdk "$sdk" --show-sdk-path)" \
    -I "$BUILD/headers" -O -wmo \
    -emit-module-path "$module/$arch.swiftmodule" \
    -emit-module-interface-path "$module/$arch.swiftinterface" \
    -emit-private-module-interface-path "$module/$arch.private.swiftinterface" \
    -o "$WORK/$fork/lib$NAME-swift.a" \
    "$src"/*.swift
  # If uniffi ever spells its imports differently, the rewrite above misses
  # them silently and the interface names a C module no client can find.
  if grep -l 'FFI' "$module"/*.swiftinterface >&2; then
    echo "the interfaces above mention a *FFI module" >&2
    exit 1
  fi

  # One archive with the Swift objects and the slice's Rust-and-PDFium archive.
  # Rust's empty codegen units warn once each, as in build-xcframework.sh.
  libtool -static -no_warning_for_no_symbols -o "$fw/$NAME" \
    "$WORK/$fork/lib$NAME-swift.a" "$BUILD/$fork/libEmbedPDFFFI.a"

  cat > "$fw/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleExecutable</key>
  <string>$NAME</string>
  <key>CFBundleIdentifier</key>
  <string>com.embedpdf.$NAME</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>$NAME</string>
  <key>CFBundlePackageType</key>
  <string>FMWK</string>
  <key>CFBundleShortVersionString</key>
  <string>1.0</string>
  <key>CFBundleVersion</key>
  <string>1</string>
  <key>MinimumOSVersion</key>
  <string>$MIN_IOS</string>
</dict>
</plist>
PLIST
  frameworks+=(-framework "$fw")
done

echo "creating the xcframework"
# xcodebuild drops each binary .swiftmodule when a .swiftinterface sits beside
# it, so clients always build the module from the interface.
xcodebuild -create-xcframework "${frameworks[@]}" \
  -output "$PUBLISH/$NAME.xcframework" > /dev/null
# --keepParent puts NAME.xcframework at the top of the zip, which is the
# layout SwiftPM expects of a binary target's archive.
(cd "$PUBLISH" && ditto -c -k --keepParent "$NAME.xcframework" "$NAME.xcframework.zip")

rm -rf "$APPLE/EmbedPDF/$NAME.xcframework"
ditto "$PUBLISH/$NAME.xcframework" "$APPLE/EmbedPDF/$NAME.xcframework"

echo
echo "  $PUBLISH/$NAME.xcframework.zip"
echo "  $APPLE/EmbedPDF/$NAME.xcframework"
(cd "$PUBLISH" && swift package compute-checksum "$NAME.xcframework.zip")
