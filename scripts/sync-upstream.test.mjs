import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

const script = resolve('scripts/sync-upstream.sh');
const implBranch = 'research/multi-platform-rust-core-impl';
const pluginsBranch = `${implBranch}-plugins`;
const runtimeBranch = 'research/multi-platform-rust-core';
const sub = 'packages/engine/runtime/runtime-src';

function fixture() {
  const root = mkdtempSync(join(tmpdir(), 'sync-upstream-'));
  const env = { ...process.env, GIT_CONFIG_GLOBAL: join(root, 'gitconfig'), GIT_CONFIG_NOSYSTEM: '1',
    GIT_AUTHOR_NAME: 'Test', GIT_AUTHOR_EMAIL: 'test@example.com', GIT_COMMITTER_NAME: 'Test',
    GIT_COMMITTER_EMAIL: 'test@example.com', GIT_TERMINAL_PROMPT: '0' };
  const git = (cwd, ...args) => execFileSync('git', args, { cwd, env, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();
  git(root, 'config', '--global', 'protocol.file.allow', 'always');
  git(root, 'config', '--global', 'init.defaultBranch', 'main');
  const runtime = join(root, 'runtime'); mkdirSync(runtime); git(runtime, 'init');
  const commit = (cwd, path, value) => { writeFileSync(join(cwd, path), value); git(cwd, 'add', path); git(cwd, 'commit', '-m', path); return git(cwd, 'rev-parse', 'HEAD'); };
  const base = commit(runtime, 'base', 'base');
  git(runtime, 'checkout', '-b', runtimeBranch);
  const forkBase = commit(runtime, 'fork', 'fork');
  const forkRuntime = join(root, 'runtime-fork.git'); git(root, 'clone', '--bare', runtime, forkRuntime);
  git(runtime, 'checkout', 'main');
  const upstreamRuntime = commit(runtime, 'upstream', 'upstream');
  const override = commit(runtime, 'annotation', 'annotation');
  const viewer = join(root, 'viewer'); mkdirSync(viewer); git(viewer, 'init');
  git(viewer, 'submodule', 'add', forkRuntime, sub);
  git(join(viewer, sub), 'checkout', forkBase);
  mkdirSync(join(viewer, 'packages/engine/runtime/scripts'), { recursive: true });
  writeFileSync(join(viewer, 'packages/engine/runtime/scripts/runtime-payload-hash.sh'), 'echo fixture\n');
  git(viewer, 'add', '.'); git(viewer, 'commit', '-m', 'viewer base');
  git(viewer, 'branch', implBranch);
  git(join(viewer, sub), 'fetch', runtime, 'main'); git(join(viewer, sub), 'checkout', upstreamRuntime);
  git(viewer, 'add', sub); commit(viewer, 'main.txt', 'upstream');
  const main = git(viewer, 'rev-parse', 'HEAD');
  const impl = join(root, 'impl'); git(viewer, 'worktree', 'add', impl, implBranch); commit(impl, 'impl.txt', 'impl');
  git(viewer, 'branch', pluginsBranch, implBranch);
  const plugins = join(root, 'plugins'); git(viewer, 'worktree', 'add', plugins, pluginsBranch); commit(plugins, 'plugins.txt', 'plugins');
  for (const path of [impl, plugins]) git(path, 'submodule', 'update', '--init');
  for (const [url, path] of [
    ['https://github.com/embedpdf/embed-pdf-viewer.git', viewer],
    ['https://github.com/embedpdf/runtime.git', runtime],
    ['https://github.com/roanutil/embed-pdf-runtime.git', forkRuntime],
  ]) git(root, 'config', '--global', `url.file://${path}.insteadOf`, url);
  const pushes = join(root, 'pushes'); writeFileSync(pushes, '');
  const hook = (bare, name) => writeFileSync(join(bare, 'hooks/pre-receive'), `#!/bin/sh\necho ${name} >> '${pushes}'\ncat >/dev/null\n`, { mode: 0o755 });
  hook(forkRuntime, 'runtime');
  for (const name of ['fork', 'private-fork']) {
    const bare = join(root, `${name}.git`); git(root, 'init', '--bare', bare); git(viewer, 'remote', 'add', name, bare); hook(bare, name);
  }
  const run = (...args) => spawnSync('bash', [script, ...args], { cwd: plugins, env, encoding: 'utf8' });
  const route = (...args) => run('--route', '--impl-worktree', impl, '--plugins-worktree', plugins, ...args);
  const head = path => git(path, 'rev-parse', 'HEAD');
  const ancestor = (path, before, after = 'HEAD') => git(path, 'merge-base', '--is-ancestor', before, after);
  return { root, env, git, commit, runtime, forkRuntime, impl, plugins, viewer, main, base, upstreamRuntime, override, pushes,
    run, route, head, ancestor, cleanup: () => rmSync(root, { recursive: true, force: true }) };
}
function withFixture(name, body) {
  test(name, () => { const f = fixture(); try { body(f); } finally { f.cleanup(); } });
}
function success(result) { assert.equal(result.status, 0, result.stdout + result.stderr); }

withFixture('route merges main into impl and impl into plugins with a fork runtime override', f => {
  success(f.route('--runtime-commit', f.override));
  f.ancestor(f.impl, f.main); f.ancestor(f.plugins, f.head(f.impl));
  const pin = f.git(f.impl, 'rev-parse', `HEAD:${sub}`);
  assert.equal(pin, f.git(f.plugins, 'rev-parse', `HEAD:${sub}`));
  f.ancestor(join(f.impl, sub), f.override, pin);
  assert.equal(readFileSync(join(f.plugins, 'main.txt'), 'utf8'), 'upstream');
  assert.equal(readFileSync(f.pushes, 'utf8'), '');
  const heads = [f.head(f.impl), f.head(f.plugins)]; success(f.route('--runtime-commit', f.override));
  assert.deepEqual([f.head(f.impl), f.head(f.plugins)], heads);
});
withFixture('route pushes runtime before impl and mirrors plugins to both viewer remotes', f => {
  success(f.route('--push'));
  assert.deepEqual(readFileSync(f.pushes, 'utf8').trim().split('\n'), ['runtime', 'private-fork', 'fork', 'private-fork']);
  for (const name of ['fork', 'private-fork']) assert.equal(f.git(f.viewer, 'ls-remote', name, `refs/heads/${pluginsBranch}`).split('\t')[0], f.head(f.plugins));
});
withFixture('route refuses a wrong destination branch before changing impl or runtime', f => {
  f.git(f.plugins, 'checkout', '-b', 'human-work');
  const before = [f.head(f.impl), f.head(join(f.impl, sub))];
  const result = f.route(); assert.notEqual(result.status, 0); assert.match(result.stderr, /expected.*plugins/);
  assert.deepEqual([f.head(f.impl), f.head(join(f.impl, sub))], before);
});
withFixture('route refuses dirty plugins before changing impl', f => {
  writeFileSync(join(f.plugins, 'plugins.txt'), 'dirty'); const before = f.head(f.impl);
  const result = f.route(); assert.notEqual(result.status, 0); assert.match(result.stderr, /commit or discard/); assert.equal(f.head(f.impl), before);
});
withFixture('route stops on impl conflicts without merging or pushing plugins', f => {
  f.commit(f.impl, 'main.txt', 'impl conflict'); const before = f.head(f.plugins);
  const result = f.route('--push'); assert.notEqual(result.status, 0); assert.match(result.stderr, /merge conflicts/);
  assert.equal(f.head(f.plugins), before); assert.equal(readFileSync(f.pushes, 'utf8'), '');
  assert.equal(f.git(f.impl, 'diff', '--name-only', '--diff-filter=U'), 'main.txt');
});
withFixture('single-branch local source mode remains available', f => {
  success(f.run('--from', implBranch)); f.ancestor(f.plugins, f.head(f.impl));
});

withFixture('route refuses a runtime override that omits the source runtime', f => {
  const before = [f.head(f.impl), f.head(f.plugins)];
  const result = f.route('--runtime-commit', f.base); assert.notEqual(result.status, 0);
  assert.match(result.stderr, /does not contain.*source runtime/);
  assert.deepEqual([f.head(f.impl), f.head(f.plugins)], before);
});
withFixture('route does not push viewers when the runtime push is rejected', f => {
  writeFileSync(join(f.forkRuntime, 'hooks/pre-receive'), '#!/bin/sh\nexit 1\n', { mode: 0o755 });
  const result = f.route('--push'); assert.notEqual(result.status, 0);
  assert.equal(readFileSync(f.pushes, 'utf8'), '');
  assert.equal(f.git(f.viewer, 'ls-remote', 'private-fork', `refs/heads/${implBranch}`), '');
});
withFixture('route stops before mirroring plugins when the impl push is rejected', f => {
  writeFileSync(join(f.root, 'private-fork.git/hooks/pre-receive'), '#!/bin/sh\nexit 1\n', { mode: 0o755 });
  const result = f.route('--push'); assert.notEqual(result.status, 0);
  assert.equal(readFileSync(f.pushes, 'utf8'), 'runtime\n');
});
withFixture('route preserves plugins runtime commits outside the impl merge', f => {
  const runtime = join(f.plugins, sub); f.git(runtime, 'checkout', runtimeBranch);
  const before = f.commit(runtime, 'plugins-runtime', 'keep');
  const result = f.route(); assert.notEqual(result.status, 0); assert.match(result.stderr, /local commits/);
  assert.equal(f.head(runtime), before);
});
withFixture('route refuses detached plugins before changing impl', f => {
  f.git(f.plugins, 'checkout', '--detach'); const before = f.head(f.impl);
  const result = f.route(); assert.notEqual(result.status, 0); assert.match(result.stderr, /detached HEAD/); assert.equal(f.head(f.impl), before);
});
withFixture('route refuses dirty plugins runtime before changing impl', f => {
  writeFileSync(join(f.plugins, sub, 'untracked'), 'keep'); const before = f.head(f.impl);
  const result = f.route(); assert.notEqual(result.status, 0); assert.match(result.stderr, /uncommitted changes/); assert.equal(f.head(f.impl), before);
});
withFixture('route refuses an existing plugins merge before changing impl', f => {
  f.git(f.plugins, 'merge', '--no-commit', '--no-ff', 'main'); const before = f.head(f.impl);
  const result = f.route(); assert.notEqual(result.status, 0); assert.match(result.stderr, /merge is in progress/); assert.equal(f.head(f.impl), before);
});
withFixture('route resumes after an impl conflict is committed', f => {
  f.commit(f.impl, 'main.txt', 'impl conflict'); assert.notEqual(f.route().status, 0);
  f.git(f.impl, 'add', 'main.txt'); f.git(f.impl, 'commit', '-m', 'resolve main');
  success(f.route()); f.ancestor(f.plugins, f.head(f.impl));
});
withFixture('explicit viewer remotes replace the plugins mirror defaults', f => {
  success(f.route('--push', '--viewer-remote', 'fork'));
  assert.deepEqual(readFileSync(f.pushes, 'utf8').trim().split('\n'), ['runtime', 'private-fork', 'fork']);
});

withFixture('runtime conflicts stop before either viewer changes', f => {
  const target = f.commit(f.runtime, 'fork', 'upstream conflict');
  const before = [f.head(f.impl), f.head(f.plugins)]; const result = f.route('--runtime-commit', target, '--push');
  assert.notEqual(result.status, 0); assert.match(result.stderr, /runtime merge conflicts/);
  assert.deepEqual([f.head(f.impl), f.head(f.plugins)], before);
  assert.equal(f.git(join(f.impl, sub), 'diff', '--name-only', '--diff-filter=U'), 'fork');
  assert.equal(readFileSync(f.pushes, 'utf8'), '');
});
withFixture('an unavailable runtime override stops before viewer merges or pushes', f => {
  const before = [f.head(f.impl), f.head(f.plugins)]; const result = f.route('--runtime-commit', 'missing-ref', '--push');
  assert.notEqual(result.status, 0); assert.deepEqual([f.head(f.impl), f.head(f.plugins)], before);
  assert.equal(readFileSync(f.pushes, 'utf8'), '');
});
withFixture('single-branch URL source mode still merges upstream', f => {
  success(f.run('--from', 'https://github.com/embedpdf/embed-pdf-viewer.git', 'main'));
  f.ancestor(f.plugins, f.main);
});
withFixture('all three route merges disable the commit graph', f => {
  const trace = join(f.root, 'git-trace'); f.env.GIT_TRACE2_EVENT = trace; success(f.route());
  const merges = readFileSync(trace, 'utf8').trim().split('\n').map(line => JSON.parse(line))
    .filter(event => event.event === 'start' && event.argv.includes('merge') && event.argv.includes('-q'));
  assert.equal(merges.length, 3);
  for (const { argv } of merges) assert.deepEqual(argv.slice(1, 4), ['-c', 'core.commitGraph=false', 'merge']);
});
