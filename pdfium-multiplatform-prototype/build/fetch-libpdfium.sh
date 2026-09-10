#!/usr/bin/env bash
# Fetch one prebuilt libembedpdf artifact from the fork's releases, verified by
# sha256. Adapted from packages/engine/runtime/scripts/fetch-libpdfium.sh; the
# only additions are the `status: pending` branch, which is how this scaffold
# reports targets without a published artifact in the selected pin.
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

RECORD="$(node -e "
const fs = require('node:fs');
const pin = JSON.parse(fs.readFileSync(process.argv[1], 'utf8'));
const artifact = pin.artifacts && pin.artifacts[process.argv[2]];
const field = (s) => String(s).replace(/[\t\n]/g, '');
let status = 'ok';
let url = '';
let sha256 = '';
if (!artifact) status = 'unknown';
else if (artifact.status === 'pending') status = 'pending';
else if (!artifact.url || !artifact.sha256) status = 'incomplete';
else {
  url = field(artifact.url);
  sha256 = field(artifact.sha256);
}
process.stdout.write([status, url, sha256].join('\t'));
" "$PIN_FILE" "$TARGET")"

IFS=$'\t' read -r STATUS URL SHA256 <<<"$RECORD"

case "$STATUS" in
  unknown)
    echo "unknown target: $TARGET" >&2
    exit 1
    ;;
  pending)
    cat >&2 <<MSG
no published libembedpdf for $TARGET in the selected pin: $PIN_FILE

The local fork builds this target, but the committed release pin is pending.
To use a local build, set EPDF_SCAFFOLD_PIN_FILE to a separate JSON pin with
this target's file:// archive URL and sha256 (without status: pending).
See pdfium-multiplatform-prototype/README.md, "Building with local artifacts".

A reproducible published build requires completing the fork's release matrix,
publishing the artifacts, and updating build/runtime-build.json.
MSG
    exit 2
    ;;
  incomplete)
    echo "missing url or sha256 for target: $TARGET" >&2
    exit 1
    ;;
esac

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
    LOCAL_ARCHIVE="$(node --input-type=module -e 'import { fileURLToPath } from "node:url"; console.log(fileURLToPath(process.argv[1]));' "$URL")"
    cp "$LOCAL_ARCHIVE" "$ARCHIVE"
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
