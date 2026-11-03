#!/usr/bin/env bash
# Syncs the runtime fork, then a viewer branch or the main → impl → plugins route.
# Route mode requires both worktrees on their named branches. Conflicts remain
# in place for resolution; rerunning skips completed merges. Pushes are opt-in.
#
# Single branch: scripts/sync-upstream.sh [--from <local-branch> | --from <url> <branch>]
# Route: scripts/sync-upstream.sh --route --impl-worktree <path> --plugins-worktree <path>
# Options: --runtime-commit <ref>, --push, --viewer-remote <remote> (repeatable).
# Route pushes impl to private-fork and plugins to fork and private-fork by default.
set -euo pipefail

UPSTREAM_VIEWER=https://github.com/embedpdf/embed-pdf-viewer.git
UPSTREAM_RUNTIME=https://github.com/embedpdf/runtime.git
FORK_RUNTIME=https://github.com/roanutil/embed-pdf-runtime.git
FORK_BRANCH=research/multi-platform-rust-core
VIEWER_REMOTES=()
ROUTE=0
IMPL_WORKTREE=
PLUGINS_WORKTREE=
IMPL_BRANCH=research/multi-platform-rust-core-impl
PLUGINS_BRANCH=research/multi-platform-rust-core-impl-plugins
RUNTIME_COMMIT=
# merge-ort opens a submodule's alternates; its commit graph can fail even when
# verification passes. Disable it for every git command, including runtime merges.
git() { command git -c core.commitGraph=false "$@"; }
SUB=packages/engine/runtime/runtime-src

PUSH=0
FROM_URL="$UPSTREAM_VIEWER"
FROM_BRANCH=main
FROM_LOCAL=0
usage() {
  echo "usage: $0 [--route --impl-worktree <path> --plugins-worktree <path>] [--push] [--runtime-commit <ref>] [--viewer-remote <remote>] [--from <url> <branch> | --from <local-branch>]" >&2
  exit 1
}
while (($#)); do
  case "$1" in
    --route) ROUTE=1; shift ;;
    --impl-worktree) [[ $# -ge 2 && -n "$2" ]] || usage; IMPL_WORKTREE="$2"; shift 2 ;;
    --plugins-worktree) [[ $# -ge 2 && -n "$2" ]] || usage; PLUGINS_WORKTREE="$2"; shift 2 ;;
    --runtime-commit) [[ $# -ge 2 && -n "$2" && "$2" != -* ]] || usage; RUNTIME_COMMIT="$2"; shift 2 ;;
    --viewer-remote) [[ $# -ge 2 && -n "$2" && "$2" != -* ]] || usage; VIEWER_REMOTES+=("$2"); shift 2 ;;
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

check_worktree() {
  local expected="${1:-}"
  BRANCH="$(git symbolic-ref --short HEAD)" || die "detached HEAD; check out the branch to sync"
  [[ -z "$expected" || "$BRANCH" == "$expected" ]] || die "expected $expected in $PWD; found $BRANCH"

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

  [[ -z "$(git -C "$SUB" status --porcelain)" ]] || die "$SUB has uncommitted changes"
  [[ ! -f "$(git -C "$SUB" rev-parse --git-path MERGE_HEAD)" ]] ||
    die "a merge is in progress in $SUB; finish it with git commit, then rerun"
}

sync_branch() (
  ROOT="$(git rev-parse --show-toplevel)"
  cd "$ROOT"
  check_worktree
  # 1. The source viewer, and the runtime commit it pins.
  if ((FROM_LOCAL)); then
    VIEWER_TARGET="$(git rev-parse -q --verify "refs/heads/$FROM_BRANCH^{commit}")" ||
      die "no local branch $FROM_BRANCH"
  else
    echo "· fetching $FROM_URL $FROM_BRANCH"
    git fetch -q "$FROM_URL" "$FROM_BRANCH"
    VIEWER_TARGET="$(git rev-parse FETCH_HEAD)"
  fi
  SOURCE_RUNTIME_TARGET="$(git ls-tree "$VIEWER_TARGET" "$SUB" | awk '$1 == "160000" {print $3}')"
  [[ -n "$SOURCE_RUNTIME_TARGET" ]] || die "$SOURCE_NAME does not pin a runtime submodule"
  RUNTIME_TARGET="$SOURCE_RUNTIME_TARGET"
  if [[ -n "$RUNTIME_COMMIT" ]]; then
    RUNTIME_TARGET="$RUNTIME_COMMIT"
  fi
  PINNED="$(git ls-tree HEAD "$SUB" | awk '{print $3}')"
  echo "  $SOURCE_NAME ${VIEWER_TARGET:0:9} pins runtime ${SOURCE_RUNTIME_TARGET:0:9}"
  [[ -z "$RUNTIME_COMMIT" ]] || echo "  runtime override: $RUNTIME_COMMIT"

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
    git cat-file -e "$RUNTIME_TARGET^{commit}" 2>/dev/null ||
      git fetch -q "$UPSTREAM_RUNTIME" "$RUNTIME_TARGET"
    RUNTIME_TARGET="$(git rev-parse --verify "$RUNTIME_TARGET^{commit}")"
    if ! git merge-base --is-ancestor "$SOURCE_RUNTIME_TARGET" "$RUNTIME_TARGET" 2>/dev/null &&
       ! git merge-base --is-ancestor "$SOURCE_RUNTIME_TARGET" "$BASE" 2>/dev/null; then
      die "runtime override does not contain the source runtime ${SOURCE_RUNTIME_TARGET:0:9}"
    fi
    git checkout -q -B "$FORK_BRANCH" "$BASE"

    if git merge-base --is-ancestor "$RUNTIME_TARGET" HEAD 2>/dev/null; then
      echo "✓ $FORK_BRANCH already has runtime ${RUNTIME_TARGET:0:9}"
    else
      echo "· merging runtime ${RUNTIME_TARGET:0:9} into $FORK_BRANCH"
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
    git merge -q --no-commit --no-ff \
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
    for remote in "${VIEWER_REMOTES[@]}"; do
      echo "· pushing $BRANCH to $remote"
      git push -q "$remote" "$BRANCH"
    done
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
)

if ((ROUTE)); then
  [[ -n "$IMPL_WORKTREE" && -n "$PLUGINS_WORKTREE" ]] || usage
  if ((FROM_LOCAL)) || [[ "$FROM_URL" != "$UPSTREAM_VIEWER" || "$FROM_BRANCH" != main ]]; then
    die "--route uses upstream main; --from is only available for a single branch"
  fi
  IMPL_WORKTREE="$(cd "$IMPL_WORKTREE" && pwd -P)"
  PLUGINS_WORKTREE="$(cd "$PLUGINS_WORKTREE" && pwd -P)"
  [[ "$IMPL_WORKTREE" != "$PLUGINS_WORKTREE" ]] || die "route requires distinct worktrees"
  (cd "$IMPL_WORKTREE"; check_worktree "$IMPL_BRANCH")
  (cd "$PLUGINS_WORKTREE"; check_worktree "$PLUGINS_BRANCH")
  [[ "$(git -C "$IMPL_WORKTREE" rev-parse --path-format=absolute --git-common-dir)" == "$(git -C "$PLUGINS_WORKTREE" rev-parse --path-format=absolute --git-common-dir)" ]] ||
    die "route worktrees must belong to the same viewer repository"
  if ((${#VIEWER_REMOTES[@]} == 0)); then VIEWER_REMOTES=(fork private-fork); fi
  REQUESTED_PUSH="$PUSH"
  PUSH=0
  (cd "$IMPL_WORKTREE"; sync_branch)
  RUNTIME_NEW="$(git -C "$IMPL_WORKTREE/$SUB" rev-parse HEAD)"
  # The first merge is local until both viewer steps succeed. Transfer its
  # runtime objects directly, preserving every local runtime commit.
  (
    cd "$PLUGINS_WORKTREE/$SUB"
    git fetch -q "$IMPL_WORKTREE/$SUB" "$RUNTIME_NEW"
    for need in "$(git -C "$PLUGINS_WORKTREE" rev-parse "HEAD:$SUB")" "$(git rev-parse HEAD)" "$(git rev-parse --verify "refs/heads/$FORK_BRANCH" 2>/dev/null || git rev-parse HEAD)"; do
      git merge-base --is-ancestor "$need" "$RUNTIME_NEW" ||
        die "plugins runtime has local commits outside the impl runtime merge; reconcile them before rerunning"
    done
    git checkout -q -B "$FORK_BRANCH" "$RUNTIME_NEW"
  )
  (cd "$PLUGINS_WORKTREE"; FROM_LOCAL=1; FROM_BRANCH="$IMPL_BRANCH";
    SOURCE_NAME="$IMPL_BRANCH"; MERGE_MSG_SOURCE="$IMPL_BRANCH"; RUNTIME_COMMIT=; sync_branch)
  if ((REQUESTED_PUSH)); then
    git -C "$IMPL_WORKTREE/$SUB" push -q "$FORK_RUNTIME" "$FORK_BRANCH"
    git -C "$IMPL_WORKTREE" push -q private-fork "$IMPL_BRANCH"
    for remote in "${VIEWER_REMOTES[@]}"; do
      git -C "$PLUGINS_WORKTREE" push -q "$remote" "$PLUGINS_BRANCH"
    done
  fi
else
  [[ -z "$IMPL_WORKTREE" && -z "$PLUGINS_WORKTREE" ]] || usage
  if ((${#VIEWER_REMOTES[@]} == 0)); then VIEWER_REMOTES=(private-fork); fi
  sync_branch
fi
