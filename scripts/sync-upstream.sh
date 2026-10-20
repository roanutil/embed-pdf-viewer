#!/usr/bin/env bash
# Merges upstream into the Rust port: the runtime fork first, then this branch.
#
#   scripts/sync-upstream.sh [--push] [--from <url> <branch> | --from <local-branch>]
#
# By default the source is upstream viewer main. --from picks another one. A
# value containing "://" is a URL, and the branch after it is fetched from
# there. Anything else is a local branch of this repository, used as it is with
# no fetch and no branch argument. The runtime step works the same for any
# source: if the fork branch already contains the runtime commit the source
# pins, it does nothing.
#
# Upstream bumps the runtime-src pointer in most merges, and this branch pins
# a commit on the runtime fork instead, so a plain `git merge` conflicts on it
# nearly every time. The resolution is always the same, so this does it:
#
#   1. Fetch the source (upstream viewer main) and read the runtime commit it
#      pins.
#   2. Merge that runtime commit into the fork's long-lived branch, inside the
#      runtime-src checkout. The fork stays upstream plus one build commit.
#   3. Merge the source into the current branch, with runtime-src
#      resolved to the fork merge from step 2 and .gitmodules kept as ours.
#   4. With --push, push the fork branch, then this branch to private-fork.
#      Without it nothing leaves the machine.
#
# A conflict in either merge stops the script with the merge in progress.
# Resolve it, commit, and rerun: finished steps are detected and skipped.
#
# Afterwards the payload hash has changed (it covers the runtime-src commit),
# so publish a payload with scripts/publish-engine-payload.mjs. A new fork
# commit means a fresh PDFium fork build, which takes hours when upstream
# changed DEPS.
set -euo pipefail

UPSTREAM_VIEWER=https://github.com/embedpdf/embed-pdf-viewer.git
UPSTREAM_RUNTIME=https://github.com/embedpdf/runtime.git
FORK_RUNTIME=https://github.com/roanutil/embed-pdf-runtime.git
FORK_BRANCH=research/multi-platform-rust-core
VIEWER_REMOTE=private-fork
SUB=packages/engine/runtime/runtime-src

PUSH=0
FROM_URL="$UPSTREAM_VIEWER"
FROM_BRANCH=main
FROM_LOCAL=0
usage() {
  echo "usage: $0 [--push] [--from <url> <branch> | --from <local-branch>]" >&2
  exit 1
}
while (($#)); do
  case "$1" in
    --push)
      PUSH=1
      shift
      ;;
    --from)
      [[ $# -ge 2 ]] || usage
      if [[ "$2" == *://* ]]; then
        [[ $# -ge 3 ]] || usage
        FROM_URL="$2"
        FROM_BRANCH="$3"
        shift 3
      else
        FROM_LOCAL=1
        FROM_BRANCH="$2"
        shift 2
      fi
      ;;
    *) usage ;;
  esac
done
if ((FROM_LOCAL)); then
  SOURCE_NAME="$FROM_BRANCH"
  MERGE_MSG_SOURCE="$FROM_BRANCH"
elif [[ "$FROM_URL" == "$UPSTREAM_VIEWER" ]]; then
  SOURCE_NAME="upstream $FROM_BRANCH"
  MERGE_MSG_SOURCE="embedpdf/embed-pdf-viewer $FROM_BRANCH"
else
  SOURCE_NAME="$FROM_BRANCH from $FROM_URL"
  MERGE_MSG_SOURCE="$FROM_BRANCH"
fi

die() {
  echo "✖ $*" >&2
  exit 1
}

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"
BRANCH="$(git symbolic-ref --short HEAD)" || die "detached HEAD; check out the branch to sync"

[[ -f "$(git rev-parse --git-path MERGE_HEAD)" ]] &&
  die "a merge is in progress here; finish it with \`git commit\`, then rerun"
# runtime-src may differ from the recorded pointer: that's a fork merge from an
# earlier run that stopped on a viewer conflict.
DIRTY="$(git status --porcelain --untracked-files=no | grep -v " $SUB\$" || true)"
[[ -z "$DIRTY" ]] || die "commit or discard these first:"$'\n'"$DIRTY"
case "$(git submodule status "$SUB" | cut -c1)" in
  " " | "+") ;;
  *)
    die "$SUB isn't checked out. From the repo root:
  git submodule sync $SUB && git submodule update --init $SUB"
    ;;
esac

# 1. The source viewer, and the runtime commit it pins.
if ((FROM_LOCAL)); then
  VIEWER_TARGET="$(git rev-parse -q --verify "refs/heads/$FROM_BRANCH^{commit}")" ||
    die "no local branch $FROM_BRANCH"
else
  echo "· fetching $FROM_URL $FROM_BRANCH"
  git fetch -q "$FROM_URL" "$FROM_BRANCH"
  VIEWER_TARGET="$(git rev-parse FETCH_HEAD)"
fi
RUNTIME_TARGET="$(git ls-tree "$VIEWER_TARGET" "$SUB" | awk '{print $3}')"
PINNED="$(git ls-tree HEAD "$SUB" | awk '{print $3}')"
echo "  $SOURCE_NAME ${VIEWER_TARGET:0:9} pins runtime ${RUNTIME_TARGET:0:9}"

# 2. The fork branch, inside runtime-src.
(
  cd "$SUB"
  [[ -z "$(git status --porcelain --untracked-files=no)" ]] ||
    die "$SUB has uncommitted changes"
  [[ -f "$(git rev-parse --git-path MERGE_HEAD)" ]] &&
    die "a merge is in progress in $SUB; finish it with \`git commit\`, then rerun"

  echo "· fetching $FORK_RUNTIME $FORK_BRANCH"
  git fetch -q "$FORK_RUNTIME" "$FORK_BRANCH"
  FORK_TIP="$(git rev-parse FETCH_HEAD)"

  # Start from whichever of the local branch and the fork's is ahead, so
  # unpushed local commits survive. Diverged is for a person to sort out.
  BASE="$FORK_TIP"
  if git show-ref -q --verify "refs/heads/$FORK_BRANCH"; then
    LOCAL="$(git rev-parse "refs/heads/$FORK_BRANCH")"
    if git merge-base --is-ancestor "$FORK_TIP" "$LOCAL"; then
      BASE="$LOCAL"
    elif ! git merge-base --is-ancestor "$LOCAL" "$FORK_TIP"; then
      die "local $FORK_BRANCH (${LOCAL:0:9}) and the fork's (${FORK_TIP:0:9}) have diverged in $SUB"
    fi
  fi
  # HEAD covers a fork merge from an earlier run that this branch hasn't
  # recorded yet; PINNED covers a fresh checkout.
  for need in "$PINNED" "$(git rev-parse HEAD)"; do
    if ! git merge-base --is-ancestor "$need" "$BASE" 2>/dev/null; then
      die "$FORK_BRANCH (${BASE:0:9}) doesn't contain ${need:0:9}, which $SUB has checked out or this branch pins; push it to the fork first"
    fi
  done
  git checkout -q -B "$FORK_BRANCH" "$BASE"

  if git merge-base --is-ancestor "$RUNTIME_TARGET" HEAD 2>/dev/null; then
    echo "✓ $FORK_BRANCH already has runtime ${RUNTIME_TARGET:0:9}"
  else
    echo "· merging runtime ${RUNTIME_TARGET:0:9} into $FORK_BRANCH"
    # By SHA: the commit upstream viewer pins can be on any upstream branch.
    git fetch -q "$UPSTREAM_RUNTIME" "$RUNTIME_TARGET"
    if ! git merge -q --no-edit \
      -m "Merge embedpdf/runtime ${RUNTIME_TARGET:0:9} into $FORK_BRANCH" "$RUNTIME_TARGET"; then
      die "runtime merge conflicts in $SUB:
$(git diff --name-only --diff-filter=U)
Resolve them there, \`git commit\`, and rerun."
    fi
  fi
)
RUNTIME_NEW="$(git -C "$SUB" rev-parse HEAD)"

# 3. Upstream viewer into this branch.
if git merge-base --is-ancestor "$VIEWER_TARGET" HEAD; then
  echo "✓ $BRANCH already has $SOURCE_NAME ${VIEWER_TARGET:0:9}"
  [[ "$PINNED" == "$RUNTIME_NEW" ]] ||
    die "$BRANCH pins runtime ${PINNED:0:9} but $SUB is at ${RUNTIME_NEW:0:9}; commit the pointer if that's intended"
else
  echo "· merging $SOURCE_NAME ${VIEWER_TARGET:0:9} into $BRANCH"
  # Without the commit-graph: git 2.56.0's merge-ort dies with "invalid commit
  # position. commit-graph is likely corrupt" when it opens runtime-src to
  # merge the pointer and that checkout borrows objects through alternates,
  # though `git commit-graph verify` passes in both repositories.
  git -c core.commitGraph=false merge -q --no-commit --no-ff \
    -m "Merge $MERGE_MSG_SOURCE into $BRANCH" "$VIEWER_TARGET" || true
  [[ -f "$(git rev-parse --git-path MERGE_HEAD)" ]] ||
    die "git merge stopped before merging; see its output above"
  # Always the fork merge, conflict or not: upstream's pointer names a commit
  # without the fork's build changes.
  git update-index --cacheinfo "160000,$RUNTIME_NEW,$SUB"
  if git diff --name-only --diff-filter=U | grep -qx .gitmodules; then
    git checkout -q --ours .gitmodules
    git add .gitmodules
  fi
  CONFLICTS="$(git diff --name-only --diff-filter=U)"
  if [[ -n "$CONFLICTS" ]]; then
    die "merge conflicts left (runtime-src and .gitmodules are already resolved):
$CONFLICTS
Resolve them, \`git commit\`, and rerun."
  fi
  git commit -q --no-edit
  echo "✓ merged; $SUB now pins ${RUNTIME_NEW:0:9}"
fi

# 4. Push, fork first so the pointer this branch records exists remotely.
if ((PUSH)); then
  echo "· pushing $FORK_BRANCH to $FORK_RUNTIME"
  git -C "$SUB" push -q "$FORK_RUNTIME" "$FORK_BRANCH"
  echo "· pushing $BRANCH to $VIEWER_REMOTE"
  git push -q "$VIEWER_REMOTE" "$BRANCH"
else
  echo "· not pushed; rerun with --push when the merges look right"
fi

HASH="$(bash packages/engine/runtime/scripts/runtime-payload-hash.sh)"
if git ls-remote --exit-code -q "$FORK_RUNTIME" "refs/tags/runtime-payloads-$HASH" >/dev/null; then
  echo "✓ payload $HASH is published"
else
  echo "· payload $HASH isn't published yet. After pushing:"
  echo "    PERF_WORK_DIR=<checkout with the fork cache>/perf-compare/.work node scripts/publish-engine-payload.mjs"
fi
