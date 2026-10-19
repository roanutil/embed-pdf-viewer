#!/usr/bin/env bash
# The canonical identity of a finished runtime payload: a hash over every
# input that determines the compiled sidecar contents. The C++ fork work is
# captured via engine-runtime-build.json (the runtime pin — the fork's CI
# bakes EPDF_* into libembedpdf.a); the rest is this repo's generators and
# compile scripts.
#
# Used by build-engine-runtime.yml to name the `runtime-payloads-<hash>`
# release it publishes, and by fetch-payload.sh to derive which release a
# checkout needs. KEEP THE FILE LIST IN SYNC with the actions/cache key in
# .github/workflows/build-engine-runtime.yml.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

INPUTS=(
  "engine-runtime-build.json"
  "scripts/build-target.sh"
  "scripts/fetch-libpdfium.sh"
  "build/generate-functions.mjs"
  "build/generate-runtime-methods.mjs"
  "build/generate-napi-binding.mjs"
  "build/CMakeLists.txt"
  "build/compile.sh"
  "build/compile.esm.sh"
)

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

ACC=""
for rel in "${INPUTS[@]}"; do
  f="$ROOT/$rel"
  if [[ -f "$f" ]]; then
    ACC+="$rel:$(sha256_file "$f")\n"
  else
    # Missing inputs contribute deterministically (mirrors hashFiles semantics).
    ACC+="$rel:-\n"
  fi
done

# The Rust engine core is linked into the wasm32 payload, so its sources are
# payload inputs too. Without them a Rust-only change keeps the hash and
# fetches the payload built before it.
#
# RUST_CRATES is every workspace crate epdf-wasm links (normal and build
# dependencies). scripts/publish-engine-payload.mjs checks it against
# `cargo metadata` before it publishes, so a new dependency fails there
# rather than going unhashed. Tests, benches, examples and docs don't reach
# the payload and are left out.
REPO_ROOT="$(cd "$ROOT/../../.." && pwd)"
RUST_CRATES=(build-support epdf-engine epdf-wasm pdfium pdfium-sys)
RUST_INPUTS=("Cargo.toml" "Cargo.lock" "rust-toolchain.toml")
for crate in "${RUST_CRATES[@]}"; do
  while IFS= read -r rel; do
    RUST_INPUTS+=("$rel")
  done < <(git -C "$REPO_ROOT" ls-files "crates/$crate" |
    grep -v -E '^crates/[^/]+/(tests|benches|examples)/|\.md$' | LC_ALL=C sort)
done
for rel in "${RUST_INPUTS[@]}"; do
  f="$REPO_ROOT/$rel"
  if [[ -f "$f" ]]; then
    ACC+="$rel:$(sha256_file "$f")\n"
  else
    ACC+="$rel:-\n"
  fi
done

# The payload links the fork archive built from runtime-src's commit (no
# release publishes wasm32-eh, so it isn't in engine-runtime-build.json), with
# the toolchain constants perf-compare/scripts/build-arm.mjs builds it with.
ACC+="runtime-src:$(git -C "$REPO_ROOT" ls-files -s packages/engine/runtime/runtime-src | awk '{print $2}')\n"
ACC+="$(grep -E '^const (FORK_TARGET|FORK_RECIPE|EH_EMSDK_VERSION) = ' "$REPO_ROOT/perf-compare/scripts/build-arm.mjs")\n"

if command -v sha256sum >/dev/null 2>&1; then
  printf '%b' "$ACC" | sha256sum | awk '{print substr($1, 1, 16)}'
else
  printf '%b' "$ACC" | shasum -a 256 | awk '{print substr($1, 1, 16)}'
fi
