import { execFileSync } from 'node:child_process';
import os from 'node:os';

const failures = [];
const warnings = [];
const run = (cmd, args) => {
  try {
    return execFileSync(cmd, args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();
  } catch {
    return null;
  }
};
const need = (label, actual, expected) => {
  if (actual === null) failures.push(`${label}: not found (need ${expected})`);
  else if (!actual.includes(expected)) failures.push(`${label}: found "${actual}", need ${expected}`);
};

need('node', process.version, 'v24.20.0');
need('pnpm', run('pnpm', ['--version']), '10.34.0');
need('rustc', run('rustc', ['--version']), '1.98.0');
const targets = run('rustup', ['target', 'list', '--installed']) ?? '';
for (const t of ['wasm32-unknown-emscripten', 'wasm32-unknown-unknown']) {
  if (!targets.split('\n').includes(t)) failures.push(`rustup target ${t}: missing (rustup target add ${t})`);
}
need('wasm-bindgen', run('wasm-bindgen', ['--version']), '0.2.127');
need('wasm-opt', run('wasm-opt', ['--version']), 'version 133');
if (run('git', ['--version']) === null) failures.push('git: not found');

if (process.platform === 'darwin') {
  const batt = run('pmset', ['-g', 'batt']) ?? '';
  if (batt.includes("'Battery Power'")) warnings.push('running on battery: plug in before a real run');
}
const freeGiB = os.freemem() / 2 ** 30;
if (freeGiB < 4) warnings.push(`only ${freeGiB.toFixed(1)} GiB free memory`);

for (const w of warnings) console.warn(`warn: ${w}`);
for (const f of failures) console.error(`fail: ${f}`);
if (failures.length) process.exit(1);
console.log('doctor: ok');
