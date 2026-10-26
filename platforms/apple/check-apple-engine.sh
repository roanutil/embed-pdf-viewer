#!/usr/bin/env bash
# Fails when the Swift package pins a different prebuilt iOS engine than the
# tree would build, so an engine change can't merge with a stale pin.
#
# Usage: check-apple-engine.sh [--manifest <path>]
#   --manifest  the Package.swift to read the pin from
#               (default: platforms/apple/EmbedPDF/Package.swift)
#
# Exit 0: the pin is current. Prints `current apple-engine-<hash>`.
# Exit 1: the pin is stale and the tree's release isn't published yet.
# Exit 2: the pin is stale but the tree's release exists; only the pin needs
#         committing.
#
# scripts/publish-engine-payload.mjs branches on these codes.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
RUNTIME_REPO="roanutil/embed-pdf-runtime"
MANIFEST="$REPO_ROOT/platforms/apple/EmbedPDF/Package.swift"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --manifest)
      MANIFEST="${2:?--manifest needs a path}"
      shift 2
      ;;
    *)
      echo "unknown argument: $1" >&2
      exit 64
      ;;
  esac
done

current="apple-engine-$(bash "$REPO_ROOT/platforms/apple/apple-engine-hash.sh")"
pinned="$(sed -n 's/^let engineRelease = "\(.*\)"$/\1/p' "$MANIFEST")"
if [[ -z "$pinned" ]]; then
  echo "no engineRelease line in $MANIFEST" >&2
  exit 64
fi

if [[ "$pinned" == "$current" ]]; then
  echo "current $current"
  exit 0
fi

echo "pinned:  $pinned"
echo "current: $current"
if gh release view "$current" --repo "$RUNTIME_REPO" >/dev/null 2>&1; then
  echo "commit the bump: rerun the publish script to rewrite Package.swift"
  exit 2
fi
echo "publish it: node scripts/publish-engine-payload.mjs --only apple"
exit 1
