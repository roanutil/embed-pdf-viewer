#!/usr/bin/env node
/**
 * Rounds are INTERLEAVED (round 1: a, b, c, d; round 2: a, b, c, d; ...). A
 * laptop's clock speed drifts over a multi-minute run, and running all of a
 * then all of b would hand the drift to whoever went last. Only medians across
 * interleaved rounds are comparable; a single round's numbers are not.
 */
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readdirSync, rmSync } from 'node:fs';
import { resolve } from 'node:path';
import { canonicalFor, compare } from './gate.mjs';

const here = import.meta.dirname;
const RESULTS = resolve(here, 'results');
const ARMS = ['a', 'b', 'c', 'd'];

const arg = (name, fallback) => {
  const i = process.argv.indexOf(`--${name}`);
  return i === -1 ? fallback : process.argv[i + 1];
};
const rounds = Number(arg('rounds', '7'));
const budgetMs = Number(arg('budget', '500'));

for (const a of ARMS) {
  const f = resolve(here, `../build/out/${a}/arm.mjs`);
  if (!existsSync(f)) {
    console.error(`arm ${a} is not built: ${f} is missing.\nRun: bash build/link-wasm.sh`);
    process.exit(1);
  }
}

const version = (cmd, args) => {
  try {
    return execFileSync(cmd, args, { encoding: 'utf8' }).trim().split('\n')[0];
  } catch {
    return 'unknown';
  }
};
const emsdk = version('em++', ['--version']);
const rustc = version('rustc', ['--version']);

// The gate runs FIRST, over all four arms. A wrong arm can be fast, and no
// timing table shows it. `compare()` returns `{ok, compared}`, not a bool.
const canonical = {};
for (const a of ARMS) canonical[a] = await canonicalFor(a);
let ok = true;
for (const a of ARMS.slice(1)) ok = compare('a', canonical.a, a, canonical[a]).ok && ok;
if (!ok) {
  console.error('\nThe arms do not produce the same answers. Timings would be meaningless.');
  process.exit(1);
}
console.log(
  `fairness gate: ${ARMS.join(', ')} agree on ${canonical.a.glyphCount} glyphs, ` +
    `page ${canonical.a.pageIndex}\n`,
);

rmSync(RESULTS, { recursive: true, force: true });
mkdirSync(RESULTS, { recursive: true });

for (let r = 1; r <= rounds; r++) {
  console.log(`── round ${r}/${rounds} ──`);
  for (const a of ARMS) {
    execFileSync('node', [resolve(here, 'node.mjs')], {
      stdio: ['ignore', 'inherit', 'inherit'],
      env: {
        ...process.env,
        BENCH_ARM: a,
        BENCH_ROUND: String(r),
        BENCH_BUDGET_MS: String(budgetMs),
        BENCH_EMSDK: emsdk,
        BENCH_RUSTC: rustc,
      },
    });
  }
}

console.log(
  `\n${readdirSync(RESULTS).length} result files in bench/results. ` +
    `Now run: node bench/compare.mjs`,
);
