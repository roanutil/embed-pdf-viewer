#!/usr/bin/env node
/**
 * One arm, one round, one results file. Spawned by run.mjs. Each arm gets its
 * own process because each loads a different Emscripten module under the same
 * export name (native arms: a different native addon), and one process
 * holding four PDFium instances would share neither a clean heap nor a
 * comparable one.
 */
import { mkdirSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { environment, measure, sinkValue } from '../../viewer-rust-port-benchmarks/bench/harness.mjs';
import { loadArm, loadNativeArm, wasmBytes } from './arm.mjs';
import { openFixture } from './fixture.mjs';
import { makeWorkloads } from './workloads.mjs';

const here = import.meta.dirname;
const armName = process.env.BENCH_ARM;
const round = Number(process.env.BENCH_ROUND ?? '1');
const budgetMs = Number(process.env.BENCH_BUDGET_MS ?? '500');
const native = process.env.BENCH_NATIVE === '1';
// The arm order run.mjs used for this round (Finding 1), so the rotation is
// auditable from the results files themselves and not just from run.mjs's
// console output.
const order = (process.env.BENCH_ORDER ?? '').split(',').filter(Boolean);

const arm = native ? await loadNativeArm(armName) : await loadArm(armName);
const fx = openFixture(arm);
const workloads = makeWorkloads(arm, fx);

const cells = {};
for (const [id, wl] of Object.entries(workloads)) {
  if (id === 'dispose' || !wl.arms.includes(armName)) continue;
  cells[id] = measure(wl.run, { budgetMs });
  process.stdout.write(
    `  ${armName} ${id.padEnd(16)} ${cells[id].ns.toFixed(1).padStart(12)} ns` +
      `  p95 ${cells[id].p95.toFixed(1).padStart(12)} ns\n`,
  );
}

const out = {
  arm: armName,
  round,
  order,
  native,
  env: await environment(),
  emsdk: process.env.BENCH_EMSDK ?? 'unknown',
  rustc: process.env.BENCH_RUSTC ?? 'unknown',
  fixture: { pageIndex: fx.pageIndex, glyphCount: fx.glyphCount },
  // Meaningless for a native addon; keep the field, but honestly null rather
  // than measuring the wrong artifact (or worse, silently reusing the wasm
  // module's byte count) under a native run.
  wasmBytes: native ? null : await wasmBytes(armName),
  sink: sinkValue(),
  cells,
};

// `BENCH_OUT` exists so a test run can write somewhere disposable instead of
// over the committed result set in bench/results/. run.mjs sets it from --out.
const dir = process.env.BENCH_OUT ? resolve(process.env.BENCH_OUT) : resolve(here, 'results');
mkdirSync(dir, { recursive: true });
const filename = native ? `native-${armName}-r${round}.json` : `${armName}-r${round}.json`;
writeFileSync(resolve(dir, filename), `${JSON.stringify(out, null, 2)}\n`);

workloads.dispose();
fx.close();
arm.close();
