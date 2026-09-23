#!/usr/bin/env bash
# Builds the Android AAR and runs the smoke test on a 16 KB page-size emulator.
#
# `multiplatform-plan.md` Phase 1: an Android AAR with the ABIs, `libc++_shared`
# and the shrinker rules, plus the 16 KB alignment checks. Local only, published
# nowhere.
#
#   bash platforms/android/build-aar.sh          # build and check
#   bash platforms/android/build-aar.sh test     # also run on the emulator
#
# The `libembedpdf.a` slices come from `engine-runtime-build.local.json`, which
# points at a local fork checkout. They cannot be built on this machine: the
# fork's Android config asserts `host_os == "linux"`, so a macOS build fails at
# `gn gen` before it compiles anything. What is used instead are artifacts from
# fork commit 7f0fa5dbe, the same commit the iOS slices come from.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PROJECT="$ROOT/platforms/android/embedpdf"
PROFILE="${PROFILE:-release}"

NDK="${ANDROID_NDK_HOME:-$HOME/Library/Android/sdk/ndk/29.0.14206865}"
[[ -d "$NDK" ]] || { echo "no NDK at $NDK; set ANDROID_NDK_HOME" >&2; exit 1; }
HOST_TAG="$(ls "$NDK/toolchains/llvm/prebuilt" | head -1)"
TC="$NDK/toolchains/llvm/prebuilt/$HOST_TAG"

# The floor the AAR declares. It has to be one the NDK ships a linker stub for,
# and it has to match `minSdk` in library/build.gradle.kts or the .so will
# reference symbols the device does not have.
API=24

# Rust target -> Android ABI directory name -> NDK triple.
SLICES=(
  "aarch64-linux-android:arm64-v8a:aarch64-linux-android"
  "x86_64-linux-android:x86_64:x86_64-linux-android"
)

export ANDROID_NDK_HOME="$NDK"
for slice in "${SLICES[@]}"; do
  rust="${slice%%:*}"
  rest="${slice#*:}"
  abi="${rest%%:*}"
  triple="${rest##*:}"
  fork="android-$([[ $abi == arm64-v8a ]] && echo arm64 || echo x64)"

  if [[ ! -f "$ROOT/packages/engine/runtime/build/libpdfium/$fork/lib/libembedpdf.a" ]]; then
    echo "missing the $fork fork slice; see the header of this script" >&2
    exit 1
  fi

  echo "building $rust"
  # Rust picks the linker from this per-target variable. Without it cargo calls
  # the host `cc`, which is Apple clang and rejects every GNU ld flag rustc
  # passes.
  linker="CARGO_TARGET_$(echo "$rust" | tr 'a-z-' 'A-Z_')_LINKER"
  env "$linker=$TC/bin/$triple$API-clang" \
    cargo build -p epdf-uniffi --target "$rust" --profile "$PROFILE" --manifest-path "$ROOT/Cargo.toml"

  jni="$PROJECT/library/src/main/jniLibs/$abi"
  mkdir -p "$jni"
  cp "$ROOT/target/$rust/$PROFILE/libepdf_uniffi.so" "$jni/"
  # The NDK's libc++, which the .so has a NEEDED entry for. Android does not
  # ship one; an AAR that leaves it out loads on a device that happens to have
  # another copy and fails on one that does not.
  cp "$TC/sysroot/usr/lib/$triple/libc++_shared.so" "$jni/"

  # 16 KB page size. Android 15 devices can use a 16 KB page, and a .so whose
  # LOAD segments are 4 KB aligned will not map there. NDK r28 and later align
  # to 16 KB by default, which is exactly the kind of default that changes back
  # under you, so it is checked rather than assumed.
  for so in "$jni"/*.so; do
    align="$("$TC/bin/llvm-readelf" -l "$so" |
      awk '/LOAD/ { getline; print $NF }' | sort -u | grep -v '^0x8$' | head -1)"
    if [[ "$align" != "0x4000" && "$align" != "0x10000" ]]; then
      echo "$so has LOAD alignment $align, which is below the 16 KB floor" >&2
      exit 1
    fi
    echo "  $(basename "$so") LOAD alignment $align"
  done
done

echo "generating bindings"
cargo build -p uniffi-bindgen --manifest-path "$ROOT/Cargo.toml"
rm -rf "$PROJECT/library/src/main/kotlin"
"$ROOT/target/debug/uniffi-bindgen" generate \
  --library "$ROOT/target/aarch64-linux-android/$PROFILE/libepdf_uniffi.so" \
  --language kotlin --no-format --out-dir "$PROJECT/library/src/main/kotlin"

# The smoke test reads the same fixtures the Rust and iOS tests read.
mkdir -p "$PROJECT/library/src/androidTest/assets"
for fixture in report colors encrypted-hello; do
  cp "$ROOT/pdfium-multiplatform-prototype/fixtures/$fixture.pdf" \
    "$PROJECT/library/src/androidTest/assets/"
done

cd "$PROJECT"
if [[ "${1:-}" == "test" ]]; then
  gradle --console=plain :library:connectedDebugAndroidTest
else
  gradle --console=plain :library:assembleRelease
fi

# The APK half of the alignment check. An ELF built with 16 KB LOAD segments
# still fails to map if the zip entry it sits at is not on a 16 KB boundary, so
# the two checks answer different questions and both are in Phase 1's scope.
# The test APK is the only one this project produces; an app packaging the AAR
# has to run the same check on its own output.
APK="$PROJECT/library/build/outputs/apk/androidTest/debug/library-debug-androidTest.apk"
ZIPALIGN="$(ls -d "${ANDROID_HOME:-$HOME/Library/Android/sdk}"/build-tools/* | sort -V | tail -1)/zipalign"
if [[ -f "$APK" && -x "$ZIPALIGN" ]]; then
  if "$ZIPALIGN" -c -P 16 4 "$APK" > /dev/null; then
    echo "  $(basename "$APK") is 16 KB page aligned"
  else
    echo "$APK is not 16 KB page aligned" >&2
    exit 1
  fi
fi

AAR="$PROJECT/library/build/outputs/aar/library-release.aar"
if [[ -f "$AAR" ]]; then
  echo
  echo "  $AAR"
  # Uncompressed and 16 KB aligned inside the archive. An APK packer will only
  # mmap a .so straight out of the APK when it is stored, not deflated, and at
  # a 16 KB boundary; the AAR is where that has to already be true.
  unzip -l "$AAR" | grep '\.so$' | sed 's/^/  /'
fi
