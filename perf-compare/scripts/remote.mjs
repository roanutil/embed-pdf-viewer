// Run the timed part on another machine, so the one doing the builds can
// keep working without its load showing up in the numbers.
//
//   PERF_REMOTE=user@host node scripts/remote.mjs [run.mjs flags]
//
// Builds stay here. This copies the harness, its node_modules, the fixtures,
// the stamps and the built entry of each arm the run needs to
// `~/perf-remote/perf-compare/` on the remote, runs `run.mjs` there, and
// brings its `results/<run-id>/` back into this checkout's `results/`. The
// remote needs mise (for this machine's Node version) and rsync; Chromium is
// installed there on first use.
// It never touches a checkout on the remote, so arms built from commits
// that exist only here work too.
import { execFileSync, spawnSync } from 'node:child_process';
import { existsSync, readdirSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { loadConfig } from '../lib/config.mjs';
import { workDir } from './materialize.mjs';

const here = fileURLToPath(new URL('..', import.meta.url));
const host = process.env.PERF_REMOTE;
if (!host) throw new Error('set PERF_REMOTE=user@host');
const runArgs = process.argv.slice(2);
const remoteRoot = 'perf-remote/perf-compare';

const config = loadConfig();
const armsFlag = runArgs.indexOf('--arms');
const arms = armsFlag === -1 ? config.armOrder : runArgs[armsFlag + 1].split(',');
for (const id of arms) {
  if (!existsSync(path.join(workDir(), 'dist', id, 'index.html'))) {
    throw new Error(`no entry build for arm ${id}; run scripts/build-entry.mjs first`);
  }
}

const git = (args) => execFileSync('git', ['-C', here, ...args], { encoding: 'utf8' }).trim();
const commit = git(['rev-parse', 'HEAD']);
const dirty = git(['status', '--porcelain', '--', '.']) !== '';

const ssh = (command, { stdio = 'inherit' } = {}) => {
  const r = spawnSync('ssh', ['-o', 'BatchMode=yes', host, command], { stdio, encoding: 'utf8' });
  if (r.status !== 0) throw new Error(`ssh ${host}: \`${command}\` exited ${r.status}`);
  return r.stdout;
};
const rsync = (args) => {
  const r = spawnSync('rsync', ['-a', '--delete', ...args], { stdio: 'inherit' });
  if (r.status !== 0) throw new Error(`rsync exited ${r.status}`);
};

console.log(`remote: ${host}, harness ${commit.slice(0, 9)}${dirty ? ' (uncommitted changes)' : ''}`);
ssh(`mkdir -p ~/${remoteRoot}/.work/dist ~/${remoteRoot}/results`);
// The harness itself, without the build tree or anyone's results.
rsync([
  '--exclude=/.work/',
  '--exclude=/results/',
  `${here}/`,
  `${host}:${remoteRoot}/`,
]);
rsync([`${workDir()}/stamps/`, `${host}:${remoteRoot}/.work/stamps/`]);
// Only the arms this run times; other arms' old builds are left alone.
for (const id of arms) rsync([`${workDir()}/dist/${id}/`, `${host}:${remoteRoot}/.work/dist/${id}/`]);

const quoted = runArgs.map((a) => `'${a.replaceAll("'", "'\\''")}'`).join(' ');
// A login shell, for the PATH that finds mise; caffeinate keeps the machine
// awake for the length of the run.
// The same Node as this machine, through mise, not whatever the remote's
// PATH finds first.
const node = `mise exec node@${process.versions.node} -- node`;
const script = [
  `cd ~/${remoteRoot}`,
  `mise install node@${process.versions.node} >/dev/null 2>&1`,
  `${node} node_modules/playwright-core/cli.js install chromium >/dev/null`,
  `PERF_HARNESS_COMMIT=${commit} PERF_HARNESS_DIRTY=${dirty ? 1 : 0} caffeinate -dimsu ${node} run.mjs ${quoted}`,
].join(' && ');
const before = new Set(ssh(`ls ~/${remoteRoot}/results`, { stdio: ['ignore', 'pipe', 'inherit'] }).split('\n'));
ssh(`zsh -lc "${script.replaceAll('"', '\\"')}"`);
const after = ssh(`ls ~/${remoteRoot}/results`, { stdio: ['ignore', 'pipe', 'inherit'] }).split('\n');
const fresh = after.filter((d) => d && !before.has(d));
for (const run of fresh) {
  rsync([`${host}:${remoteRoot}/results/${run}/`, path.join(here, 'results', run) + '/']);
  console.log(`fetched results/${run}`);
}
if (!fresh.length) throw new Error('the remote run produced no results directory');
const local = readdirSync(path.join(here, 'results'));
console.log(`local results: ${local.length} runs`);
