#!/usr/bin/env bash
# Every suite in this scaffold that can run without an iOS destination or an
# Android APK. Fails on the first suite that fails, and prints what it
# deliberately did not run and why.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# Required suites that could not run, including unpublished pinned artifacts.
# Continue collecting other results, but never report full coverage as passing.
SKIPPED=()

echo "== build/build-node.sh =="
# build/libpdfium/ is gitignored (.gitignore:2); crates/pdfium-sys/build.rs
# panics without the fork's headers for the host target, and nothing fetched
# them before cargo test --workspace ran below. build-node.sh already does
# that fetch (via fetch-libpdfium.sh) as a prerequisite for building the napi
# addon, for the same host target cargo test resolves by default, so running
# it first satisfies both: cargo test gets its headers, and npm test below
# gets the addon packages/node/index.mjs throws without (packages/node/*.node
# is gitignored, .gitignore:15). README.md:127 already documents this as the
# Build step for the Node row; it just wasn't wired in here.
bash build/build-node.sh

echo "== cargo test --workspace =="
cargo test --workspace

echo "== build/link-wasm.sh =="
# build/out/web/epdf.{mjs,wasm} are gitignored (.gitignore:3), so on a clean
# checkout node --test packages/web/vectors.test.mjs fails with
# ERR_MODULE_NOT_FOUND instead of exercising anything. npm test below reads
# packages/web/index.mjs, which imports that pair, so link it first.
#
# link-wasm.sh's own first step is fetch-libpdfium.sh wasm32-eh, which exits 2
# while embedpdf/runtime hasn't published that target (build/runtime-build.json,
# "status": "pending"). This is incomplete coverage and must fail the gate;
# EPDF_SCAFFOLD_PIN_FILE can supply a local artifact. Any other
# nonzero exit is a real failure and should still stop the script.
if bash build/link-wasm.sh; then
  WEB_LINKED=1
else
  code=$?
  if [[ $code -eq 2 ]]; then
    echo "skipped: wasm32-eh is pending (build/runtime-build.json); the web module can't link yet" >&2
    WEB_LINKED=0
    SKIPPED+=("web (em++ module): wasm32-eh is pending; supply EPDF_SCAFFOLD_PIN_FILE with a built artifact")
  else
    exit "$code"
  fi
fi

echo "== npm test =="
if [[ "$WEB_LINKED" == 1 ]]; then
  npm test
else
  # Same suites as npm test's glob (package.json's "test" script), minus
  # packages/web/*.test.mjs: that one imports the module link-wasm.sh above
  # didn't produce, and ERR_MODULE_NOT_FOUND there would read as a bug rather
  # than the pending artifact it actually is.
  node --test build/*.test.mjs vectors/*.test.mjs packages/node/*.test.mjs apps/node-cli/*.test.mjs apps/web-demo/*.test.mjs
fi

echo "== swift (macOS destination) =="
# packages/swift/EpdfOps/{Frameworks,Sources/EpdfOps/Generated,Sources/EpdfOpsFFI}
# are gitignored (.gitignore:8-9); build-swift.sh is what stages them.
bash build/build-swift.sh
bash build/test-swift.sh

echo "== kotlin (jvm desktop) =="
# /usr/bin/java is the macOS stub that errors with nothing configured, and
# /usr/libexec/java_home fails outright because the JBR Android Studio ships
# is never registered as a system JVM. Every gradlew
# invocation below needs JAVA_HOME set explicitly rather than discovered.
export JAVA_HOME="${JAVA_HOME:-/Applications/Android Studio.app/Contents/jbr/Contents/Home}"
if [[ ! -x "$JAVA_HOME/bin/java" ]]; then
  echo "skipped: no JDK at $JAVA_HOME. Fix with: install Android Studio, or set JAVA_HOME to a JDK 21+." >&2
  SKIPPED+=("kotlin (jvm desktop): no JDK at $JAVA_HOME")
  SKIPPED+=("android (jvm unit test, no device or APK): gated on the same JAVA_HOME, not reached")
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
    SKIPPED+=("android (jvm unit test, no device or APK): no Android SDK at $ANDROID_HOME")
  else
    ( cd apps/android && ./gradlew :app:testDebugUnitTest )
  fi
fi

echo
echo "Not run by this script:"
echo "  Browser and mobile UI smoke tests: no UI automation is wired into this script."
echo "  Manual simulator/emulator and browser results: docs/research/platform-gap-audit.md."

if [[ ${#SKIPPED[@]} -gt 0 ]]; then
  echo
  echo "Required suites skipped: this run is incomplete and cannot pass."
  printf '  %s\n' "${SKIPPED[@]}"
  exit 1
fi
