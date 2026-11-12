#!/usr/bin/env bash
# Runs inside the linux/amd64 build container. Builds the fork's wasm32-eh
# PDFium from the runtime bundle at /work/in/runtime.bundle.
# Each step logs to /work/logs/<step>.log and writes <step>.done with rc/secs.
set -uo pipefail
export DEBIAN_FRONTEND=noninteractive
mkdir -p /work/logs /work/artifacts
L=/work/logs
date -u +%FT%TZ > $L/.start
step() {
  local name="$1"; shift
  local t0=$SECONDS
  "$@" > "$L/$name.log" 2>&1
  local rc=$?
  echo "$name rc=$rc secs=$((SECONDS - t0))" | tee "$L/$name.done"
  [[ $rc -eq 0 ]] || { echo "STEP FAILED: $name"; exit $rc; }
}

step apt bash -c 'apt-get update && apt-get install -y --no-install-recommends git python3 curl ca-certificates xz-utils bzip2 lsb-release file pkg-config tar patch'
step depot-tools bash -c '[[ -d /work/depot_tools ]] || git clone --depth 1 https://chromium.googlesource.com/chromium/tools/depot_tools.git /work/depot_tools'
export PATH="/work/depot_tools:$PATH" DEPOT_TOOLS_UPDATE=0
git config --global --add safe.directory '*'
step depot-tools-bootstrap /work/depot_tools/ensure_bootstrap

step clone bash -c '
  set -e
  [[ -d /work/runtime/.git ]] || git clone --filter=blob:none --branch fix/android-secure-random https://github.com/roanutil/embed-pdf-runtime.git /work/runtime
  cd /work/runtime
  git rev-parse -q --verify catchup/2026-11-12-runtime >/dev/null || git fetch /work/in/runtime.bundle catchup/2026-11-12-runtime:catchup/2026-11-12-runtime
  git checkout catchup/2026-11-12-runtime
  git rev-parse HEAD'

cd /work/runtime
step sync env PDF_RUNTIME_TARGET_OS_LIST=emscripten scripts/embedpdf-runtime/ensure-deps.sh
step emsdk-4.0.0 bash -c '
  set -e
  [[ -x third_party/emsdk/emsdk ]] || git clone https://github.com/emscripten-core/emsdk.git third_party/emsdk
  third_party/emsdk/emsdk install 4.0.0
  third_party/emsdk/emsdk activate 4.0.0'
export PATH="/work/runtime/third_party/emsdk/upstream/emscripten:/work/runtime/third_party/emsdk/upstream/bin:$PATH"
step wasm32-eh env PDF_RUNTIME_TARGET_OS_LIST=emscripten scripts/embedpdf-runtime/build-target.sh wasm32-eh
step package env PDF_RUNTIME_ARTIFACT_DIR=/work/artifacts scripts/embedpdf-runtime/package-target.sh wasm32-eh
(cd /work/artifacts && sha256sum ./*.tar.gz > SHA256SUMS && cat SHA256SUMS)
echo ALL DONE
