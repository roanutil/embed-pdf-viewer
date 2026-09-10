#!/usr/bin/env node
/**
 * The round-robin driver.
 *
 * Each POC benchmarks itself in its own process, because all three define
 * packages called `@poc/geometry`, `@poc/shapes` and `@poc/core` and one Node
 * process cannot import three different modules under the same specifier.
 *
 * Rounds are INTERLEAVED (round 1: p1, p2, p3; round 2: p1, p2, p3; ...) rather
 * than run per-POC to completion. A laptop's clock speed drifts over a multi
 * minute run, and running all of p1 then all of p2 would hand the drift to
 * whoever went last. Only medians across interleaved rounds are comparable.
 */
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync } from 'node:fs';
import { resolve } from 'node:path';
import { displayListDifferences } from './harness.mjs';
import { parseRunOptions } from './run-options.mjs';

const here = import.meta.dirname;
const root = resolve(here, '..');
const RESULTS = resolve(here, 'results');

const POCS = [
  { dir: 'typescript-baseline', wasm: null },
  { dir: 'rust-geometry-only', wasm: 'packages/geometry/wasm/poc_geometry_bg.wasm' },
  { dir: 'rust-geometry-and-state', wasm: 'packages/core/wasm/poc_core_bg.wasm' },
];

let options;
try {
  options = parseRunOptions(process.argv.slice(2));
} catch (error) {
  console.error(`Invalid benchmark arguments: ${error.message}`);
  process.exit(1);
}
const { rounds, budgetMs, allocRounds } = options;

for (const poc of POCS) {
  if (poc.wasm && !existsSync(resolve(root, poc.dir, poc.wasm))) {
    console.error(
      `${poc.dir}: ${poc.wasm} is missing.\n` +
        `Run \`pnpm build\` in ${poc.dir} first (it builds the wasm before the demo).`,
    );
    process.exit(1);
  }
}

/**
 * Clears only the per-round files THIS run regenerates.
 *
 * Not `rmSync(RESULTS, { recursive: true })`. `results/cold-start.json` is
 * hand-collected from a browser run (see `bench/README.md`) and is the one
 * results file the repo commits, so the recursive form deleted it and
 * `compare.mjs` then dropped the whole Cold start section from `RESULTS.md`
 * without saying anything. The patterns here are the ones `.gitignore` lists as
 * regenerable.
 */
const PER_ROUND = /-(?:probe|timing|alloc)-r\d+\.json$/;
mkdirSync(RESULTS, { recursive: true });
for (const f of readdirSync(RESULTS)) {
  if (PER_ROUND.test(f)) rmSync(resolve(RESULTS, f));
}

function runOne(dir, mode, round) {
  execFileSync('node', ['--expose-gc', '--import', 'tsx', 'bench/node.ts'], {
    cwd: resolve(root, dir),
    stdio: ['ignore', 'inherit', 'inherit'],
    env: { ...process.env, BENCH_ROUND: String(round), BENCH_BUDGET_MS: String(budgetMs), BENCH_MODE: mode },
  });
}

function readProbe(dir) {
  const file = resolve(RESULTS, `${dir}-probe-r1.json`);
  return JSON.parse(readFileSync(file, 'utf8')).probe;
}

/**
 * The fairness gate. rust-geometry-and-state builds models by dispatching into Rust while typescript-baseline
 * and rust-geometry-only build them in TypeScript, so different shape sets are a live risk
 * and nothing about the timings would reveal it. Compare display lists first.
 *
 * FIRST means first. This used to read the probe out of each POC's round-1
 * timing file, so a run whose implementations disagreed published three sets of
 * timings before it aborted, and `bench/README.md` promised a guarantee the
 * order did not deliver. The probe is its own pass now.
 */
function fairnessGate() {
  const probes = POCS.map((p) => ({ dir: p.dir, probe: readProbe(p.dir) }));
  let ok = true;
  for (const { dir, probe } of probes.slice(1)) {
    // Coordinates are compared within a tolerance rather than byte for byte:
    // the probes are rounded to 1e-9 for display, and two values one ulp apart
    // can round to different decimals while being the same answer.
    const differing = displayListDifferences(probes[0].probe, probe);
    if (differing.length) {
      ok = false;
      console.error(`\nFAIRNESS GATE FAILED: ${dir} disagrees with ${probes[0].dir}.`);
      const a = probes[0].probe;
      for (const i of differing) {
        console.error(`  item ${i}\n    ${probes[0].dir}: ${JSON.stringify(a[i])}\n    ${dir}: ${JSON.stringify(probe[i])}`);
      }
    }
  }
  if (!ok) {
    console.error('\nThe three implementations do not produce the same scene. Timings would be meaningless.');
    process.exit(1);
  }
  console.log(`\nfairness gate: all three agree on ${probes[0].probe.length} display items\n`);
}

console.log(`rounds=${rounds} budget=${budgetMs}ms alloc-rounds=${allocRounds}\n`);

console.log('── fairness probe ──');
for (const poc of POCS) runOne(poc.dir, 'probe', 1);
fairnessGate();

for (let r = 1; r <= rounds; r++) {
  console.log(`── timing round ${r}/${rounds} ──`);
  for (const poc of POCS) runOne(poc.dir, 'timing', r);
}

for (let r = 1; r <= allocRounds; r++) {
  console.log(`── alloc round ${r}/${allocRounds} ──`);
  for (const poc of POCS) runOne(poc.dir, 'alloc', r);
}

const written = readdirSync(RESULTS).filter((f) => PER_ROUND.test(f)).length;
console.log(`\n${written} result files in bench/results. Now run: node bench/compare.mjs`);
