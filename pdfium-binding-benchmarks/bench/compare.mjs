#!/usr/bin/env node
/**
 * Medians across interleaved rounds, printed as a markdown table ready to
 * paste into RESULTS.md. Ratios are against arm a, which is what ships today.
 *
 * `--native` reads only the `native-<arm>-r<round>.json` files bench/node.mjs
 * writes under a native run, ignoring the wasm `<arm>-r<round>.json` files
 * that may also be sitting in bench/results/ from an earlier wasm run (and
 * vice versa without the flag), and skips the wasm-size table: `wasmBytes` is
 * null in every native result file, since there is no wasm module to size.
 *
 * `--out <dir>` reads a directory other than bench/results/, matching
 * bench/run.mjs's own `--out`. Without it, `run.mjs --out /tmp/x` followed by
 * the `Now run: node bench/compare.mjs` that run.mjs prints tabulated the 56
 * committed files instead of the run that just finished, and said nothing.
 */
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { resolve } from 'node:path';
import { WORKLOAD_TABLE } from './workloads.mjs';

const here = import.meta.dirname;
const arg = (name, fallback) => {
  const i = process.argv.indexOf(`--${name}`);
  return i === -1 ? fallback : process.argv[i + 1];
};
const RESULTS = resolve(arg('out', resolve(here, 'results')));
const ARMS = ['a', 'b', 'c', 'd'];
const LABEL = {
  a: 'a direct',
  b: 'b rust thin',
  c: 'c rust coarse',
  d: 'd c++ coarse',
};

const native = process.argv.includes('--native');

const files = (existsSync(RESULTS) ? readdirSync(RESULTS) : []).filter((f) =>
  native ? f.startsWith('native-') && f.endsWith('.json') : !f.startsWith('native-') && f.endsWith('.json'),
);
if (files.length === 0) {
  console.error(
    `no ${native ? 'native ' : ''}results in ${RESULTS}; ` +
      `run: node bench/run.mjs${native ? ' --native' : ''}`,
  );
  process.exit(1);
}

const byArm = {};
let header = null;
for (const f of files) {
  const j = JSON.parse(readFileSync(resolve(RESULTS, f), 'utf8'));
  header ??= j;
  (byArm[j.arm] ??= []).push(j);
}

const median = (xs) => {
  const s = [...xs].sort((x, y) => x - y);
  if (s.length === 0) return NaN;
  const mid = s.length >> 1;
  return s.length % 2 ? s[mid] : (s[mid - 1] + s[mid]) / 2;
};

// Order by the declaration in workloads.mjs, not by iteration order over
// files, so the table reads in the same order every run regardless of
// readdir order.
const declaredIds = WORKLOAD_TABLE.map((w) => w.id);
const seenIds = new Set(
  Object.values(byArm).flatMap((rs) => rs.flatMap((r) => Object.keys(r.cells))),
);
const ids = declaredIds.filter((id) => seenIds.has(id));

const fmt = (ns) => {
  if (!Number.isFinite(ns)) return '—';
  if (ns < 1_000) return `${ns.toFixed(1)} ns`;
  if (ns < 1_000_000) return `${(ns / 1_000).toFixed(2)} µs`;
  return `${(ns / 1_000_000).toFixed(3)} ms`;
};

console.log(`\n${native ? 'NATIVE · ' : ''}Node ${header.env.node} · ${header.env.cpu} · ${header.env.platform}`);
console.log(`emsdk ${header.emsdk} · ${header.rustc}`);
console.log(
  `fixture: page ${header.fixture.pageIndex}, ${header.fixture.glyphCount} glyphs · ` +
    `${Object.values(byArm)[0].length} rounds\n`,
);

console.log(`| workload | ${ARMS.map((a) => LABEL[a]).join(' | ')} | b/a | c/a | d/a |`);
console.log(`|---|${ARMS.map(() => '---').join('|')}|---|---|---|`);

for (const id of ids) {
  const med = {};
  for (const a of ARMS) {
    const rows = (byArm[a] ?? []).map((r) => r.cells[id]?.ns).filter(Number.isFinite);
    med[a] = rows.length ? median(rows) : NaN;
  }
  const ratio = (a) =>
    Number.isFinite(med[a]) && Number.isFinite(med.a)
      ? `${(med[a] / med.a).toFixed(2)}x`
      : '—';
  console.log(
    `| \`${id}\` | ${ARMS.map((a) => fmt(med[a])).join(' | ')} | ` +
      `${ratio('b')} | ${ratio('c')} | ${ratio('d')} |`,
  );
}

if (!native) {
  console.log(`\n| arm | wasm bytes | delta vs a |`);
  console.log(`|---|---|---|`);
  const baseBytes = byArm.a?.[0]?.wasmBytes ?? NaN;
  for (const a of ARMS) {
    const b = byArm[a]?.[0]?.wasmBytes;
    if (!Number.isFinite(b)) continue;
    const delta = a === 'a' ? '—' : `+${(((b - baseBytes) / 1024).toFixed(1))} KiB`;
    console.log(`| ${LABEL[a]} | ${b.toLocaleString()} | ${delta} |`);
  }
}

// p95 table: the harness's p95 is the p95 of BATCH MEANS, not of individual
// calls (see viewer-rust-port-benchmarks/bench/harness.mjs's header comment). It
// shows run-to-run drift within a round, not per-call tail latency.
console.log(`\n| workload | ${ARMS.map((a) => LABEL[a]).join(' | ')} (p95 of batch means) |`);
console.log(`|---|${ARMS.map(() => '---').join('|')}|`);
for (const id of ids) {
  const med = {};
  for (const a of ARMS) {
    const rows = (byArm[a] ?? []).map((r) => r.cells[id]?.p95).filter(Number.isFinite);
    med[a] = rows.length ? median(rows) : NaN;
  }
  console.log(`| \`${id}\` | ${ARMS.map((a) => fmt(med[a])).join(' | ')} |`);
}

const spread = [];
for (const id of ids) {
  for (const a of ARMS) {
    const rows = (byArm[a] ?? []).map((r) => r.cells[id]?.ns).filter(Number.isFinite);
    if (rows.length < 2) continue;
    spread.push({ id, arm: a, ratio: Math.max(...rows) / Math.min(...rows) });
  }
}
spread.sort((x, y) => y.ratio - x.ratio);
console.log(`\nworst round-to-round spread: ${spread
  .slice(0, 3)
  .map((s) => `${s.arm}/${s.id} ${s.ratio.toFixed(2)}x`)
  .join(', ')}`);
console.log('A spread above about 1.3x indicates unstable timings; it does not identify the cause.');
