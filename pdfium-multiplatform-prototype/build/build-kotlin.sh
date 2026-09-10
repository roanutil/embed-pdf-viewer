#!/usr/bin/env bash
# Builds libepdf_uniffi for the JVM host and, when artifacts are available, for the
# Android ABIs, then generates the Kotlin bindings.
#
#   bash build/build-kotlin.sh                 # host JVM (darwin-aarch64)
#   bash build/build-kotlin.sh android-arm64   # needs a published or local pin
set -euo pipefail

# /usr/bin/java is the macOS stub and /usr/libexec/java_home does not see the
# JBR Android Studio ships, so every script sets JAVA_HOME explicitly rather
# than relying on the invoking shell.
export JAVA_HOME="${JAVA_HOME:-/Applications/Android Studio.app/Contents/jbr/Contents/Home}"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SLICE="${1:-host}"
MODULE="$ROOT/packages/kotlin/epdf-ops"
GENERATED="$MODULE/src/main/kotlin/generated"

case "$SLICE" in
  host)
    RUST_TARGET=aarch64-apple-darwin
    FORK_TARGET=darwin-arm64
    DEST="$MODULE/libs/darwin-aarch64"
    ;;
  android-arm64)
    RUST_TARGET=aarch64-linux-android
    FORK_TARGET=android-arm64
    DEST="$MODULE/src/main/jniLibs/arm64-v8a"
    ;;
  android-x64)
    RUST_TARGET=x86_64-linux-android
    FORK_TARGET=android-x64
    DEST="$MODULE/src/main/jniLibs/x86_64"
    ;;
  *)
    echo "unknown slice: $SLICE (known: host, android-arm64, android-x64)" >&2
    exit 1
    ;;
esac

# Exits 2 with its own message for the pending Android targets. Prints the
# directory it populated: build/libpdfium/<target>, or EPDF_SCAFFOLD_LIB_DIR's
# when that override is set, which is also where cargo (crates/build-support)
# links from, so the host copy below must come from the same place.
LIB_DIR="$(bash "$ROOT/build/fetch-libpdfium.sh" "$FORK_TARGET")"

rustup target add "$RUST_TARGET"
mkdir -p "$DEST"

if [[ "$SLICE" == host ]]; then
  ( cd "$ROOT" && EPDF_SCAFFOLD_TARGET="$FORK_TARGET" cargo build --release -p epdf-uniffi --target "$RUST_TARGET" )
  cp "$ROOT/target/$RUST_TARGET/release/libepdf_uniffi.dylib" "$DEST/"
  cp "$LIB_DIR/lib/libembedpdf."* "$DEST/"
  # JNA dlopens libepdf_uniffi from an absolute path, so @loader_path resolves
  # to this directory and libembedpdf is found beside it. Checked the actual
  # install name first: libembedpdf.dylib carries @rpath/libembedpdf.dylib
  # (verified with otool -D), so adding the rpath is enough; no
  # install_name_tool -change needed.
  otool -L "$DEST/libepdf_uniffi.dylib"
  install_name_tool -add_rpath "@loader_path" "$DEST/libepdf_uniffi.dylib"
else
  cargo install cargo-ndk --version 4.1.2 --locked
  ( cd "$ROOT" && EPDF_SCAFFOLD_TARGET="$FORK_TARGET" cargo ndk -t "$RUST_TARGET" build --release -p epdf-uniffi )
  cp "$ROOT/target/$RUST_TARGET/release/libepdf_uniffi.so" "$DEST/"

  # libc++_shared.so must ship alongside it. The fork builds android-* with
  # use_custom_libcxx=false, so libembedpdf.a uses the NDK's libc++ rather than
  # Chromium's bundled copy, and the NDK links that dynamically: without this
  # the app installs and launches fine, then dies at the first FFI call with
  #   dlopen failed: library "libc++_shared.so" not found:
  #   needed by .../lib/arm64-v8a/libepdf_uniffi.so
  # Take it from the same NDK cargo-ndk just built against rather than a
  # hardcoded version, so the two cannot drift apart.
  NDK_ROOT="${ANDROID_NDK_HOME:-${ANDROID_NDK_ROOT:-}}"
  if [[ -z "$NDK_ROOT" ]]; then
    NDK_ROOT="$(ls -d "${ANDROID_HOME:-$HOME/Library/Android/sdk}"/ndk/* 2>/dev/null | sort -V | tail -1)"
  fi
  CXX_SHARED="$(find "$NDK_ROOT" -path "*${RUST_TARGET%%-*}-linux-android*" -name libc++_shared.so 2>/dev/null | head -1)"
  if [[ -z "$CXX_SHARED" ]]; then
    echo "no libc++_shared.so for $RUST_TARGET under NDK ${NDK_ROOT:-<not found>}" >&2
    echo "libepdf_uniffi.so links it dynamically; the app would fail at its first call." >&2
    exit 1
  fi
  cp "$CXX_SHARED" "$DEST/"
  echo "staged $(basename "$CXX_SHARED") from $NDK_ROOT"
  # No separate libembedpdf copy here: the fork's android-* artifact is a
  # static archive (lib/libembedpdf.a), not a shared library. build-support's
  # emit_link_directives already statically links it into libepdf_uniffi.so
  # above (ArtifactShape::StaticArchive), so there is no libembedpdf.so to
  # stage into jniLibs, unlike the host branch's darwin-arm64 dylib above.
fi

mkdir -p "$GENERATED"
LIBRARY="$DEST/libepdf_uniffi.dylib"
[[ -f "$LIBRARY" ]] || LIBRARY="$DEST/libepdf_uniffi.so"
# --config points at packages/kotlin/uniffi.toml (outside crates/, which this
# task does not touch) so the generated bindings land in package
# com.embedpdf.scaffold, the same package as Epdf.kt and VectorsTest.kt.
# Without it uniffi-bindgen defaults to `uniffi.epdf_uniffi`, and the
# handwritten files would need imports the brief's verbatim code doesn't have.
( cd "$ROOT" && cargo run --release --bin uniffi-bindgen -- generate \
    --library "$LIBRARY" --language kotlin --out-dir "$GENERATED" \
    --config "$ROOT/packages/kotlin/uniffi.toml" )

echo "staged $SLICE -> $DEST; bindings -> $GENERATED"
