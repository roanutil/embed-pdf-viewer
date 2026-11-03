// Fork commit ancestry for publish-engine-payload.mjs, apart so it can be
// tested against a throwaway repository.
import { spawnSync } from 'node:child_process';

/**
 * Whether fork commit `sha` contains `commit`, in the runtime-src checkout at
 * `src`. A commit the checkout doesn't have throws, saying to fetch it: an
 * unknown commit can't be told apart from one the fork lacks.
 */
export function forkContains(src, sha, commit) {
  const { status, stderr, error } = spawnSync(
    'git',
    ['-C', src, 'merge-base', '--is-ancestor', commit, sha],
    { encoding: 'utf8' },
  );
  if (status === 0) return true;
  if (status === 1) return false;
  // A git that couldn't start leaves no stderr, only `error`.
  const reason = (stderr ?? error?.message ?? '').trim();
  throw new Error(
    `git merge-base can't answer in ${src} (${reason}): fetch ${commit} (and ${sha} if missing) ` +
      `into runtime-src, for example \`git -C ${src} fetch origin ${commit}\`, and run again`,
  );
}
