#!/usr/bin/env node
/**
 * Rounds are INTERLEAVED (round 1: a, b, c, d; round 2: a, b, c, d; ...) AND,
 * within each round, the arm ORDER is rotated by the round index (round 1:
 * a,b,c,d; round 2: b,c,d,a; round 3: c,d,a,b; ...). A laptop's clock speed
 * drifts over a multi-minute run, and running all of a then all of b would
 * hand the drift to whoever went last; interleaving fixes that ACROSS rounds.
 * But the drift also decays WITHIN a round (background load is highest right
 * after the previous round's cooldown and lowest by the time the last arm
 * runs), and a fixed a,b,c,d order hands that decay to whichever arm always
 * goes first. Rotating the order spreads each arm across every position over
 * the seven rounds, so no arm is systematically first or last. Only medians
 * across interleaved, rotated rounds are comparable; a single round's numbers
 * are not.
 *
 * `--native` runs the same protocol over the native arms instead of the wasm
 * ones: same gate-before-timing rule, same interleaving, same per-round
 * rotation. It differs only in which build artifacts it checks for, which
 * canonical-answer loader the gate uses, and the `native-` prefix on its
 * result files (bench/compare.mjs reads the two sets of files separately).
 */
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readdirSync, rmSync } from 'node:fs';
import { relative, resolve } from 'node:path';
import { canonicalFor, canonicalForNative, compare } from './gate.mjs';
import { GATE_FIXTURES } from './fixture.mjs';
import { parseRunOptions } from './run-options.mjs';

const here = import.meta.dirname;
const ARMS = ['a', 'b', 'c', 'd'];

let options;
try {
  options = parseRunOptions(process.argv.slice(2));
} catch (error) {
  console.error(`Invalid benchmark arguments: ${error.message}`);
  process.exit(1);
}
const { native, rounds, budgetMs, out } = options;
// `--out` writes the round files somewhere other than bench/results/.
//
// bench/results/ holds 56 COMMITTED files, the raw data behind every table in
// RESULTS.md, and any run clears the ones it is about to replace. That makes a
// short run destructive: `run.mjs --rounds 2 --budget 100` swapped seven rounds
// of real data for two rounds of noise. Tests pass --out and never touch the
// committed set.
const RESULTS = resolve(out ?? resolve(here, 'results'));

if (native) {
  // Arm a is packages/engine/runtime's own shipping addon; arms b and c
  // share one napi-rs addon; arm d additionally needs the cmake-js addon for
  // its cc_* coarse entries. See bench/arm.mjs's loadNativeArm.
  const required = [
    resolve(here, '../../packages/engine/runtime/npm/darwin-arm64/lib/pdf-runtime.node'),
    resolve(here, '../build/out/native/napi-arms.node'),
    resolve(here, '../build/out/native/cc-ops.node'),
  ];
  for (const f of required) {
    if (!existsSync(f)) {
      console.error(`native build artifact missing: ${f}\nRun: bash build/build-native.sh`);
      process.exit(1);
    }
  }
} else {
  for (const a of ARMS) {
    const f = resolve(here, `../build/out/${a}/arm.mjs`);
    if (!existsSync(f)) {
      console.error(`arm ${a} is not built: ${f} is missing.\nRun: bash build/link-wasm.sh`);
      process.exit(1);
    }
  }
}

const version = (cmd, args) => {
  try {
    return execFileSync(cmd, args, { encoding: 'utf8' }).trim().split('\n')[0];
  } catch {
    return 'unknown';
  }
};
const emsdk = native ? 'n/a (native)' : version('em++', ['--version']);
const rustc = version('rustc', ['--version']);

// The gate runs FIRST, over all four arms. A wrong arm can be fast, and no
// timing table shows it. `compare()` returns `{ok, compared}`, not a bool.
const getCanonical = native ? canonicalForNative : canonicalFor;
const canonical = {};
for (const a of ARMS) canonical[a] = await getCanonical(a);
let ok = true;
let compared = Infinity;
for (const a of ARMS.slice(1)) {
  const result = compare('a', canonical.a, a, canonical[a]);
  ok = result.ok && ok;
  compared = Math.min(compared, result.compared);
}
if (!ok) {
  console.error('\nThe arms do not produce the same answers. Timings would be meaningless.');
  process.exit(1);
}
// Report every fixture actually compared, matching gate.mjs's own CLI
// wording (Finding 5): this line is the artifact people read, so it should
// say what `compare()` really checked, not just the timing fixture.
const fixtureSummary = GATE_FIXTURES.map(
  ({ label }) => `${label} (${canonical.a.fixtures[label].glyphCount} glyphs)`,
).join(', ');
console.log(
  `${native ? 'native ' : ''}fairness gate: ${ARMS.join(', ')} agree on ${fixtureSummary}, ` +
    `${compared} workload${compared === 1 ? '' : 's'} compared\n`,
);

// Clear only the files THIS run is about to write. Wiping the whole directory
// destroyed the wasm result set the first time a --native run followed a wasm
// one; commit a31c1123 re-ran wasm to restore it.
mkdirSync(RESULTS, { recursive: true });
for (const f of readdirSync(RESULTS)) {
  if (!f.endsWith('.json')) continue;
  if (f.startsWith('native-') === native) rmSync(resolve(RESULTS, f), { force: true });
}

for (let r = 1; r <= rounds; r++) {
  // Rotate by round index (Finding 1): round 1 is a,b,c,d; round 2 shifts
  // by one to b,c,d,a; and so on, cycling back to a,b,c,d every four rounds.
  // Applies identically under --native: rotation is not optional, and the
  // wasm run is the proof that position within a round biases results.
  const shift = (r - 1) % ARMS.length;
  const order = [...ARMS.slice(shift), ...ARMS.slice(0, shift)];
  console.log(`── round ${r}/${rounds} (order: ${order.join(', ')}) ──`);
  for (const a of order) {
    execFileSync('node', [resolve(here, 'node.mjs')], {
      stdio: ['ignore', 'inherit', 'inherit'],
      env: {
        ...process.env,
        BENCH_OUT: RESULTS,
        BENCH_ARM: a,
        BENCH_ROUND: String(r),
        BENCH_BUDGET_MS: String(budgetMs),
        BENCH_EMSDK: emsdk,
        BENCH_RUSTC: rustc,
        BENCH_NATIVE: native ? '1' : '',
        // Recorded in each round's results JSON so the rotation is auditable.
        BENCH_ORDER: order.join(','),
      },
    });
  }
}

const shown = relative(process.cwd(), RESULTS);
// Repeat --out in the suggested command. Without it the reader follows this
// line, compare.mjs reads bench/results/, and tabulates the committed data
// rather than the run that just finished.
const outFlag = out === undefined ? '' : ` --out ${out}`;
console.log(
  `\n${readdirSync(RESULTS).length} result files in ${shown && !shown.startsWith('..') ? shown : RESULTS}. ` +
    `Now run: node bench/compare.mjs${native ? ' --native' : ''}${outFlag}`,
);
