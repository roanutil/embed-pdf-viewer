#!/usr/bin/env bash
# Every suite in this scaffold that can run without an iOS destination or an
# Android APK. Fails on the first suite that fails, and prints what it
# deliberately did not run and why.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

echo "== cargo test --workspace =="
cargo test --workspace

echo "== build/link-wasm.sh =="
# build/out/web/epdf.{mjs,wasm} are gitignored (.gitignore:3), so on a clean
# checkout node --test packages/web/vectors.test.mjs fails with
# ERR_MODULE_NOT_FOUND instead of exercising anything. npm test below reads
# packages/web/index.mjs, which imports that pair, so link it first.
bash build/link-wasm.sh

echo "== npm test =="
npm test

echo "== swift (macOS destination) =="
# packages/swift/EpdfOps/{Frameworks,Sources/EpdfOps/Generated,Sources/EpdfOpsFFI}
# are gitignored (.gitignore:8-9); build-swift.sh is what stages them.
bash build/build-swift.sh
bash build/test-swift.sh

echo "== kotlin (jvm desktop) =="
# /usr/bin/java is the macOS stub that errors with nothing configured, and
# /usr/libexec/java_home fails outright because the JBR Android Studio ships
# is never registered as a system JVM (docs/superpowers/specs/2026-09-03-
# rust-core-scaffold-design.md, "What actually gets verified"). Every gradlew
# invocation below needs JAVA_HOME set explicitly rather than discovered.
export JAVA_HOME="/Applications/Android Studio.app/Contents/jbr/Contents/Home"
if [[ ! -x "$JAVA_HOME/bin/java" ]]; then
  echo "skipped: no JDK at $JAVA_HOME. Fix with: install Android Studio, or set JAVA_HOME to a JDK 21+." >&2
else
  bash build/build-kotlin.sh
  ( cd packages/kotlin && ./gradlew :epdf-ops:test )

  echo "== android (jvm unit test, no device or APK) =="
  # apps/android/local.properties is gitignored (.gitignore:20), so Gradle has
  # no SDK path unless ANDROID_HOME is set here; apps/android/README.md
  # documents the same requirement for a manual run.
  export ANDROID_HOME="${ANDROID_HOME:-$HOME/Library/Android/sdk}"
  if [[ ! -d "$ANDROID_HOME" ]]; then
    echo "skipped: no Android SDK at $ANDROID_HOME. Fix with: install the Android SDK and set ANDROID_HOME." >&2
  else
    ( cd apps/android && ./gradlew :app:testDebugUnitTest )
  fi
fi

cat <<'MSG'

Not run, and why:
  apps/ios on an iOS destination     embedpdf/runtime does not build ios-arm64 or ios-sim-arm64
                                      (build/runtime-build.json, "status": "pending")
  apps/android packaging             embedpdf/runtime does not build android-arm64 or android-x64;
                                      apps/android/app/build.gradle.kts's requireNativeLibraries task
                                      fails assembleDebug, assembleRelease, bundleDebug and
                                      installDebug on purpose rather than ship an APK that would
                                      crash on launch
  apps/web-demo, apps/ios's window   nothing drives them; they are demo surfaces, not test suites
MSG
