// node scripts/summary.mjs <results/run-id> [arm ...]: the numbers to quote
// from a run's report.md (written by report.mjs, run here if missing).
//
// For each pair in the report, the verdict counts, and every row that isn't
// "no detectable difference". For a pair whose first arm is the baseline
// (`main`), also the median ratio over the latency rows and over the
// main-thread rows (`mainJsMs`, `mainTaskMs`). Rows reported as a difference
// in ms (settle times) have no ratio and count only toward the verdicts.
// Arms after the run id narrow the output to pairs naming one of them.
import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { loadConfig } from '../lib/config.mjs';

const [runArg, ...only] = process.argv.slice(2);
if (!runArg) throw new Error('usage: node scripts/summary.mjs <results/run-id> [arm ...]');
const here = fileURLToPath(new URL('..', import.meta.url));
const dir = path.resolve(here, runArg);
const reportFile = path.join(dir, 'report.md');
if (!existsSync(reportFile)) execFileSync('node', ['report.mjs', dir], { cwd: here, stdio: 'ignore' });
const report = readFileSync(reportFile, 'utf8');
const baseline = loadConfig().baseline;

const median = (xs) => {
  if (xs.length === 0) return NaN;
  const s = [...xs].sort((a, b) => a - b);
  const m = s.length >> 1;
  return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2;
};
const cells = (line) => line.split('|').slice(1, -1).map((c) => c.trim());

const sections = report.split(/^## /m).slice(1);
for (const section of sections) {
  const head = section.slice(0, section.indexOf('\n'));
  const pair = head.match(/^`([^`]+)` vs `([^`]+)`$/);
  if (!pair) continue;
  const [, a, b] = pair;
  if (only.length && !only.includes(a) && !only.includes(b)) continue;
  const rows = section
    .split('\n')
    .filter((l) => /^\| [a-zA-Z]/.test(l) && !l.startsWith('| Row'))
    .map(cells)
    .filter((c) => c.length >= 5);
  if (rows.length === 0) continue;
  const verdicts = {};
  for (const r of rows) verdicts[r[4]] = (verdicts[r[4]] ?? 0) + 1;
  console.log(`\n${a} vs ${b}: ${Object.entries(verdicts).map(([v, n]) => `${n} ${v}`).join(', ')}`);
  if (a === baseline) {
    const lat = [];
    const cpu = [];
    for (const r of rows) {
      const ratio = Number.parseFloat(r[3]);
      if (!/^\d/.test(r[3]) || Number.isNaN(ratio)) continue;
      (/mainJs|mainTask/.test(r[0]) ? cpu : lat).push(ratio);
    }
    console.log(`  latency median ${median(lat).toFixed(3)}x, main-thread median ${median(cpu).toFixed(3)}x`);
  } else {
    for (const r of rows.filter((r) => r[4] !== 'no detectable difference')) {
      console.log(`  ${r[0]}: ${r[1]} -> ${r[2]}, ${r[3]} (${r[4]})`);
    }
  }
}
