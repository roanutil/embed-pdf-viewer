#!/usr/bin/env bash
# The canonical identity of the prebuilt iOS engine XCFramework: a hash over
# every input that determines its contents. Prints 16 hex chars.
#
# scripts/publish-engine-payload.mjs names the `apple-engine-<hash>` release
# it publishes with this, and the Swift package derives which release a
# checkout needs from it.
#
# APPLE_CRATES is every workspace crate epdf-viewer-host links (normal and
# build dependencies). scripts/publish-engine-payload.mjs checks it against
# `cargo metadata` before it publishes, so a new dependency fails there
# rather than going unhashed. Tests, benches, examples and docs don't reach
# the framework and are left out.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

APPLE_CRATES=(acrojs actions annotation annotation-core build-support commands epdf-engine epdf-ffi epdf-kernel epdf-uniffi epdf-viewer-host form geometry i18n interaction link measurement metadata page-edit pdfium pdfium-sys plugin-conformance redaction render search selection shell signature signature-core stage stage-core stamp ui-core view-manager)
INPUTS=(
  "Cargo.toml"
  "Cargo.lock"
  "rust-toolchain.toml"
  "platforms/apple/build-xcframework.sh"
  "platforms/apple/package-engine-framework.sh"
)
for crate in "${APPLE_CRATES[@]}"; do
  while IFS= read -r rel; do
    INPUTS+=("$rel")
  done < <(git -C "$REPO_ROOT" ls-files "crates/$crate" |
    grep -v -E '^crates/[^/]+/(tests|benches|examples)/|\.md$' | LC_ALL=C sort)
done

ACC=""
for rel in "${INPUTS[@]}"; do
  f="$REPO_ROOT/$rel"
  if [[ -f "$f" ]]; then
    ACC+="$rel:$(sha256_file "$f")\n"
  else
    # Missing inputs contribute deterministically (mirrors hashFiles semantics).
    ACC+="$rel:-\n"
  fi
done

# The framework links the fork archive built from runtime-src's commit.
ACC+="runtime-src:$(git -C "$REPO_ROOT" ls-files -s packages/engine/runtime/runtime-src | awk '{print $2}')\n"

if command -v sha256sum >/dev/null 2>&1; then
  printf '%b' "$ACC" | sha256sum | awk '{print substr($1, 1, 16)}'
else
  printf '%b' "$ACC" | shasum -a 256 | awk '{print substr($1, 1, 16)}'
fi
