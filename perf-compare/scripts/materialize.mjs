import { execFileSync } from 'node:child_process';
import { appendFileSync, existsSync, mkdirSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { loadConfig } from '../lib/config.mjs';

const repo = fileURLToPath(new URL('../..', import.meta.url));
export const workDir = () => process.env.PERF_WORK_DIR ?? path.join(repo, 'perf-compare/.work');
export const worktreePath = (id) => path.join(workDir(), 'worktrees', id);

const git = (args, cwd = repo) => execFileSync('git', args, { cwd, encoding: 'utf8' }).trim();
const hasCommit = (sha) => {
  try {
    git(['cat-file', '-e', `${sha}^{commit}`]);
    return true;
  } catch {
    return false;
  }
};

// A pin can live on any remote (a research branch pushed only to a private
// fork, say), so each configured remote is tried until one has it.
function fetchCommit(sha) {
  for (const remote of git(['remote']).split('\n').filter(Boolean)) {
    try {
      git(['fetch', '--quiet', remote, sha]);
      return;
    } catch {}
  }
  throw new Error(`no remote has ${sha}; push its branch first`);
}

const dirtyTracked = (dir, submodules = 'none') =>
  git(['status', '--porcelain', '--untracked-files=no', `--ignore-submodules=${submodules}`], dir);

// Every build and every cached stamp is trusted only after this passes, so
// meta.json's SHA always names the source that was built. Untracked files are
// allowed, because the entry and kernel shim are untracked.
export function assertPinned(id, sha) {
  const dir = worktreePath(id);
  const head = git(['rev-parse', 'HEAD'], dir);
  if (head !== sha) throw new Error(`worktree ${id} is at ${head}, not the pinned ${sha}; run materialize.mjs`);
  if (dirtyTracked(dir)) throw new Error(`worktree ${id} has tracked changes; remove ${dir} and rerun`);
  // `git submodule status` prefixes `+` for a commit other than the recorded
  // one, `-` for an uninitialized submodule and `U` for a conflict.
  const subs = git(['submodule', 'status', '--recursive'], dir);
  const subsDirty = git(['submodule', 'foreach', '--quiet', '--recursive', 'git status --porcelain --untracked-files=no'], dir);
  if (/^[-+U]/m.test(subs) || subsDirty) {
    throw new Error(`worktree ${id} has a modified submodule; remove ${dir} and rerun`);
  }
}

// A pinned tree can record a submodule commit that only exists in a fork the
// host checkout has fetched (the impl pin's runtime-src commit is one), so each
// top-level submodule borrows objects from the host's copy when it has one.
// Without a host copy this is a plain `submodule update`.
function updateSubmodules(dir) {
  const hostModules = path.resolve(repo, git(['rev-parse', '--git-common-dir']), 'modules');
  let paths = '';
  try {
    paths = git(['config', '-f', '.gitmodules', '--get-regexp', '^submodule\\..*\\.path$'], dir);
  } catch {
    return; // no .gitmodules
  }
  for (const line of paths.split('\n').filter(Boolean)) {
    const sub = line.slice(line.indexOf(' ') + 1);
    const reference = path.join(hostModules, sub);
    const args = ['submodule', 'update', '--init', '--quiet'];
    if (existsSync(reference)) {
      args.push('--reference', reference);
      // `--reference` only applies to a fresh clone. A clone left by an
      // earlier run gets the same alternate by hand.
      if (existsSync(path.join(dir, sub, '.git'))) {
        const alternates = path.join(git(['rev-parse', '--absolute-git-dir'], path.join(dir, sub)), 'objects/info/alternates');
        const line = path.join(reference, 'objects');
        const current = existsSync(alternates) ? readFileSync(alternates, 'utf8') : '';
        if (!current.split('\n').includes(line)) appendFileSync(alternates, `${line}\n`);
      }
    }
    git([...args, '--', sub], dir);
  }
  git(['submodule', 'update', '--init', '--recursive', '--quiet'], dir);
}

export function materialize(config) {
  mkdirSync(path.join(workDir(), 'worktrees'), { recursive: true });
  for (const [id, w] of Object.entries(config.worktrees)) {
    const dir = worktreePath(id);
    if (!existsSync(dir)) {
      if (!hasCommit(w.sha)) fetchCommit(w.sha);
      git(['worktree', 'add', '--detach', dir, w.sha]);
    }
    // Checked on every run, not only when HEAD moved: a worktree at the pinned
    // SHA with edited source would otherwise build that source while
    // meta.json reports the pin. Checking before checkout also keeps a
    // checkout from carrying edits across.
    // Submodule commits are left out here because the update below moves
    // them; assertPinned checks them, and edits inside them, afterwards.
    if (dirtyTracked(dir, 'all')) throw new Error(`worktree ${id} has tracked changes; remove ${dir} and rerun`);
    if (git(['rev-parse', 'HEAD'], dir) !== w.sha) git(['checkout', '--quiet', '--detach', w.sha], dir);
    updateSubmodules(dir);
    assertPinned(id, w.sha);
    console.log(`${id}: ${dir} at ${w.sha.slice(0, 9)}`);
  }
}

if (import.meta.url === `file://${process.argv[1]}`) materialize(loadConfig());
