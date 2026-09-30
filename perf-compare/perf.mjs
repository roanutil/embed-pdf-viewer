// doctor -> materialize -> build -> run -> report. Flags go to run.mjs, with
// or without a leading `--` (pnpm may or may not pass one through).
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const here = fileURLToPath(new URL('.', import.meta.url));
const node = (script, args = []) =>
  execFileSync(process.execPath, [script, ...args], { cwd: here, stdio: 'inherit' });

const argv = process.argv.slice(2);
const sep = argv.indexOf('--');
const runArgs = sep === -1 ? argv : argv.slice(sep + 1);
node('scripts/doctor.mjs');
node('scripts/materialize.mjs');
node('scripts/build-entry.mjs');
node('run.mjs', runArgs);
node('report.mjs', ['--latest']);
