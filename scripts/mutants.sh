#!/bin/sh
# Run cargo-mutants without taking the machine down.
#
#   scripts/mutants.sh -p epdf-plugin-foo --test-package epdf-plugin-foo [--file src/x.rs]
#
# Two cargo-mutants runs at `-j 4` from parallel worktrees held every core at
# 100% until the machine crashed, twice. So this wrapper enforces three rules:
#
#   1. One run at a time on the machine, first come first served. Each caller
#      takes a ticket in a queue directory under /tmp shared by every
#      worktree, and runs only when its ticket is the oldest one left. A
#      ticket or lock left by a process that no longer exists is removed.
#      (A bare lock was not enough: a caller running file after file
#      re-took it before a waiter polling every 30 seconds ever saw it free.)
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

QUEUE=/tmp/epdf-cargo-mutants.queue
LOCK=/tmp/epdf-cargo-mutants.lock
BUILD_JOBS=${EPDF_MUTANTS_BUILD_JOBS:-4}

mkdir -p "$QUEUE"
# Seconds since the epoch, zero-padded so tickets sort by arrival; the pid
# breaks ties and names the owner.
TICKET="$QUEUE/$(printf '%012d' "$(date +%s)")-$$"
: >"$TICKET"
trap 'rm -f "$TICKET"; [ "$(cat "$LOCK/pid" 2>/dev/null)" = "$$" ] && rm -rf "$LOCK"' EXIT INT TERM HUP

alive() { kill -0 "$1" 2>/dev/null; }

# Say so once: a caller reads this output after the run, and a line every
# poll over an hour's wait is hundreds of lines of nothing.
announced=
while :; do
  # Drop tickets whose owner is gone.
  for t in "$QUEUE"/*; do
    [ -e "$t" ] || continue
    owner=${t##*-}
    alive "$owner" || rm -f "$t"
  done
  # Drop a lock whose holder is gone.
  holder=$(cat "$LOCK/pid" 2>/dev/null || echo "")
  if [ -n "$holder" ] && ! alive "$holder"; then
    rm -rf "$LOCK"
  fi
  oldest=$(ls "$QUEUE" | sort | head -n 1)
  if [ "$QUEUE/$oldest" = "$TICKET" ] && mkdir "$LOCK" 2>/dev/null; then
    echo $$ >"$LOCK/pid"
    break
  fi
  if [ -z "$announced" ]; then
    echo "scripts/mutants.sh: queued behind another run (lock pid ${holder:-none}); waiting" >&2
    announced=1
  fi
  sleep 15
done
rm -f "$TICKET"

CARGO_BUILD_JOBS=$BUILD_JOBS nice -n 19 cargo mutants -j 1 "$@"
