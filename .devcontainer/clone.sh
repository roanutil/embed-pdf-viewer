#!/usr/bin/env bash
set -euo pipefail

workspace="${EMBEDPDF_WORKSPACE:-/workspaces/embed-pdf-viewer}"
repository="${EMBEDPDF_REPOSITORY:-https://github.com/roanutil/embed-pdf-viewer.git}"
branch="${EMBEDPDF_BRANCH:-research/multi-platform-rust-core-impl}"

# Rebuilds reuse the volume, including local commits and uncommitted work.
if [ -d "$workspace/.git" ]; then
  printf 'Reusing repository at %s\n' "$workspace"
  exit 0
fi

git clone --branch "$branch" -- "$repository" "$workspace"
