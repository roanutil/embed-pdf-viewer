#!/usr/bin/env bash
# The canonical identity of the prebuilt iOS engine XCFramework: a hash over
# every input that determines its contents. Prints 16 hex chars.
#
# scripts/publish-engine-payload.mjs names the `apple-engine-<hash>` release
# it publishes with this, and the Swift package derives which release a
# checkout needs from it.
#
# APPLE_CRATES is every workspace crate epdf-viewer-host links (normal and
# build dependencies), as its directory under crates/, which for the plugins
# is plugins/<name>. scripts/publish-engine-payload.mjs checks it against
# `cargo metadata` before it publishes, so a new dependency fails there
# rather than going unhashed. Tests, benches, examples and docs don't reach
# the framework and are left out. A listed crate with no files is a path that
# doesn't exist, and fails rather than hashing nothing.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

APPLE_CRATES=(build-support epdf-engine epdf-ffi epdf-kernel epdf-uniffi epdf-viewer-host pdfium pdfium-sys plugin-conformance plugins/acrojs plugins/actions plugins/annotation plugins/annotation-core plugins/attachment plugins/commands plugins/fonts plugins/form plugins/form-core plugins/geometry plugins/history plugins/i18n plugins/interaction plugins/link plugins/measurement plugins/metadata plugins/outline plugins/page-edit plugins/redaction plugins/render plugins/search plugins/selection plugins/shell plugins/signature plugins/signature-core plugins/stage plugins/stage-core plugins/stamp plugins/ui-core plugins/view-manager)
INPUTS=(
  "Cargo.toml"
  "Cargo.lock"
  "rust-toolchain.toml"
  "platforms/apple/build-xcframework.sh"
  "platforms/apple/package-engine-framework.sh"
  # FORK_RECIPE and the iOS sync policy decide how the PDFium slices build.
  "perf-compare/scripts/build-arm.mjs"
)
for crate in "${APPLE_CRATES[@]}"; do
  count=0
  while IFS= read -r rel; do
    INPUTS+=("$rel")
    count=$((count + 1))
  done < <(git -C "$REPO_ROOT" ls-files "crates/$crate" |
    grep -v -E "^crates/$crate/(tests|benches|examples)/|\.md$" | LC_ALL=C sort)
  if ((count == 0)); then
    echo "APPLE_CRATES lists $crate, but crates/$crate has no tracked files" >&2
    exit 1
  fi
done
# uniffi-bindgen generates the Swift the framework compiles. It isn't linked,
# so it isn't in APPLE_CRATES.
while IFS= read -r rel; do
  INPUTS+=("$rel")
done < <(git -C "$REPO_ROOT" ls-files crates/uniffi-bindgen | grep -v '\.md$' | LC_ALL=C sort)

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
