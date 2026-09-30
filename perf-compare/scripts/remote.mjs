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
// awake for the length of the run. Not `-d` or `-u`: with the display
// assertions `Tracing.start` would never answer, three runs out of three.
// The same Node as this machine, through mise, not whatever the remote's
// PATH finds first.
const node = `mise exec node@${process.versions.node} -- node`;
// The run is detached from this ssh session: a full run outlasts a dropped
// connection, and whatever runs this script may stop it before the end. Its
// output goes to a log, which is followed from here; its exit status to a
// file next to it.
const log = `.work/remote-run.log`;
const exitFile = `.work/remote-run.exit`;
const run = `PERF_HARNESS_COMMIT=${commit} PERF_HARNESS_DIRTY=${dirty ? 1 : 0} caffeinate -ims ${node} run.mjs ${quoted}`;
const script = [
  `cd ~/${remoteRoot}`,
  `mise install node@${process.versions.node} >/dev/null 2>&1`,
  `${node} node_modules/playwright-core/cli.js install chromium >/dev/null`,
  `rm -f ${exitFile}`,
  `(nohup sh -c '${run.replaceAll("'", "'\\''")}; echo \\$? > ${exitFile}' > ${log} 2>&1 < /dev/null &)`,
].join(' && ');
const before = new Set(ssh(`ls ~/${remoteRoot}/results`, { stdio: ['ignore', 'pipe', 'inherit'] }).split('\n'));
ssh(`zsh -lc "${script.replaceAll('"', '\\"')}"`);
const quiet = { stdio: ['ignore', 'pipe', 'inherit'] };
let shown = 0;
for (;;) {
  await new Promise((r) => setTimeout(r, 15_000));
  let out;
  try {
    out = ssh(`cd ~/${remoteRoot} && tail -c +${shown + 1} ${log}; echo; echo "@@exit:$(cat ${exitFile} 2>/dev/null)"`, quiet);
  } catch (error) {
    console.log(`(poll failed, retrying: ${error.message})`);
    continue;
  }
  const at = out.lastIndexOf('\n@@exit:');
  const text = out.slice(0, at);
  process.stdout.write(text);
  shown += Buffer.byteLength(text);
  const status = out.slice(at + '\n@@exit:'.length).trim();
  if (status === '') continue;
  if (status !== '0') throw new Error(`the remote run exited ${status}; see ~/${remoteRoot}/${log} on ${host}`);
  break;
}
const after = ssh(`ls ~/${remoteRoot}/results`, { stdio: ['ignore', 'pipe', 'inherit'] }).split('\n');
const fresh = after.filter((d) => d && !before.has(d));
for (const run of fresh) {
  rsync([`${host}:${remoteRoot}/results/${run}/`, path.join(here, 'results', run) + '/']);
  console.log(`fetched results/${run}`);
}
if (!fresh.length) throw new Error('the remote run produced no results directory');
const local = readdirSync(path.join(here, 'results'));
console.log(`local results: ${local.length} runs`);
