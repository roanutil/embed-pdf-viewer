#!/usr/bin/env bash
# Fetch one prebuilt libembedpdf artifact from the fork's releases, verified by
# sha256. Adapted from packages/engine/runtime/scripts/fetch-libpdfium.sh; the
# only additions are the `status: pending` branch, which is how this scaffold
# reports the iOS and Android artifacts embedpdf/runtime does not build yet.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="${1:-}"
PIN_FILE="${EPDF_SCAFFOLD_PIN_FILE:-$ROOT/build/runtime-build.json}"
OUT_DIR="${EPDF_SCAFFOLD_LIB_DIR:-$ROOT/build/libpdfium}"

if [[ -z "$TARGET" ]]; then
  echo "usage: $0 <target>" >&2
  exit 1
fi

sha256_file() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  elif command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    echo "missing sha256 checksum tool: install shasum or sha256sum" >&2
    exit 1
  fi
}

STATUS="$(node -e "
const fs = require('node:fs');
const pin = JSON.parse(fs.readFileSync(process.argv[1], 'utf8'));
const artifact = pin.artifacts && pin.artifacts[process.argv[2]];
if (!artifact) process.stdout.write('unknown');
else if (artifact.status === 'pending') process.stdout.write('pending');
else if (!artifact.url || !artifact.sha256) process.stdout.write('incomplete');
else process.stdout.write('ok');
" "$PIN_FILE" "$TARGET")"

case "$STATUS" in
  unknown)
    echo "unknown target: $TARGET" >&2
    exit 1
    ;;
  pending)
    cat >&2 <<MSG
no libembedpdf for $TARGET: embedpdf/runtime does not build it yet.

The fork's release matrix in .github/workflows/release-libpdfium.yml covers nine
targets and neither mobile one. Adding them is recommendation 6 of
docs/research/pdfium-rust-wrapper-decision.md and has its own design doc.

Until that lands, this target cannot be built. Everything that does not need it
still works: see rust-core-scaffold/README.md.
MSG
    exit 2
    ;;
  incomplete)
    echo "missing url or sha256 for target: $TARGET" >&2
    exit 1
    ;;
esac

URL="$(node -e "process.stdout.write(JSON.parse(require('node:fs').readFileSync(process.argv[1],'utf8')).artifacts[process.argv[2]].url)" "$PIN_FILE" "$TARGET")"
SHA256="$(node -e "process.stdout.write(JSON.parse(require('node:fs').readFileSync(process.argv[1],'utf8')).artifacts[process.argv[2]].sha256)" "$PIN_FILE" "$TARGET")"

DEST="$OUT_DIR/$TARGET"
CACHE="$ROOT/build/cache"
ARCHIVE="$CACHE/libembedpdf-runtime-$TARGET.tar.gz"

mkdir -p "$CACHE" "$DEST"

needs_refresh() {
  [[ ! -f "$ARCHIVE" ]] && return 0
  [[ "$(sha256_file "$ARCHIVE")" != "$SHA256" ]]
}

if needs_refresh; then
  echo "Downloading $TARGET from $URL" >&2
  if [[ "$URL" == file://* ]]; then
    cp "${URL#file://}" "$ARCHIVE"
  else
    curl -fL "$URL" -o "$ARCHIVE"
  fi
fi

ACTUAL_SHA="$(sha256_file "$ARCHIVE")"
if [[ "$ACTUAL_SHA" != "$SHA256" ]]; then
  echo "sha256 mismatch for $ARCHIVE" >&2
  echo "expected: $SHA256" >&2
  echo "actual:   $ACTUAL_SHA" >&2
  rm -f "$ARCHIVE"
  exit 1
fi

rm -rf "$DEST"
mkdir -p "$DEST"
tar -xzf "$ARCHIVE" -C "$DEST"

echo "$DEST"
