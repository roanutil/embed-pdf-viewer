#!/bin/sh
# Run cargo-mutants without taking the machine down.
#
#   scripts/mutants.sh -p epdf-plugin-foo --test-package epdf-plugin-foo [--file src/x.rs]
#
# Two cargo-mutants runs at `-j 4` from parallel worktrees held every core at
# 100% until the machine crashed, twice. So this wrapper enforces three rules:
#
#   1. One run at a time on the machine. The lock is a directory under /tmp
#      shared by every worktree; a second caller waits for the first to finish.
#      A lock left by a process that no longer exists is taken over.
#   2. One mutant at a time (`-j 1`), each built with at most
#      EPDF_MUTANTS_BUILD_JOBS compile jobs (default 4 of this machine's 16).
#   3. Lowest scheduling priority (`nice -n 19`), so the desktop stays usable.
#
# Pass anything else cargo-mutants accepts. A `-j`/`--jobs` argument is
# refused rather than silently overridden.

set -u

for arg in "$@"; do
  case "$arg" in
    -j | -j* | --jobs | --jobs=*)
      echo "scripts/mutants.sh: do not pass $arg; mutants run one at a time" >&2
      exit 2
      ;;
  esac
done

LOCK=/tmp/epdf-cargo-mutants.lock
BUILD_JOBS=${EPDF_MUTANTS_BUILD_JOBS:-4}

while ! mkdir "$LOCK" 2>/dev/null; do
  holder=$(cat "$LOCK/pid" 2>/dev/null || echo "")
  if [ -n "$holder" ] && ! kill -0 "$holder" 2>/dev/null; then
    echo "scripts/mutants.sh: taking over a stale lock from pid $holder" >&2
    rm -rf "$LOCK"
    continue
  fi
  echo "scripts/mutants.sh: another mutation run holds $LOCK (pid ${holder:-?}); waiting" >&2
  sleep 30
done
echo $$ >"$LOCK/pid"
trap 'rm -rf "$LOCK"' EXIT INT TERM HUP

CARGO_BUILD_JOBS=$BUILD_JOBS nice -n 19 cargo mutants -j 1 "$@"
