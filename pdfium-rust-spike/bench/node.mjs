#!/usr/bin/env node
/**
 * One arm, one round, one results file. Spawned by run.mjs. Each arm gets its
 * own process because each loads a different Emscripten module under the same
 * export name, and one process holding four PDFium instances would share
 * neither a clean heap nor a comparable one.
 */
import { mkdirSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { environment, measure, sinkValue } from '../../ts-wasm-interop-poc/bench/harness.mjs';
import { loadArm, wasmBytes } from './arm.mjs';
import { openFixture } from './fixture.mjs';
import { makeWorkloads } from './workloads.mjs';

const here = import.meta.dirname;
const armName = process.env.BENCH_ARM;
const round = Number(process.env.BENCH_ROUND ?? '1');
const budgetMs = Number(process.env.BENCH_BUDGET_MS ?? '500');

const arm = await loadArm(armName);
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
  env: await environment(),
  emsdk: process.env.BENCH_EMSDK ?? 'unknown',
  rustc: process.env.BENCH_RUSTC ?? 'unknown',
  fixture: { pageIndex: fx.pageIndex, glyphCount: fx.glyphCount },
  wasmBytes: await wasmBytes(armName),
  sink: sinkValue(),
  cells,
};

const dir = resolve(here, 'results');
mkdirSync(dir, { recursive: true });
writeFileSync(resolve(dir, `${armName}-r${round}.json`), `${JSON.stringify(out, null, 2)}\n`);

workloads.dispose();
fx.close();
arm.close();
