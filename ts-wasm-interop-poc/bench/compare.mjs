#!/usr/bin/env node
/**
 * Reads bench/results/*.json, takes medians ACROSS ROUNDS per cell, and prints
 * a comparison plus a markdown block ready to paste into the research doc.
 *
 * Medians across rounds, not means, and never a single round: a single round is
 * one sample of a drifting machine.
 */
import { readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

const here = import.meta.dirname;
const RESULTS = resolve(here, 'results');
const ORDER = ['poc-1-ts', 'poc-2-rust-geometry', 'poc-3-rust-core'];
const SHORT = { 'poc-1-ts': 'poc-1', 'poc-2-rust-geometry': 'poc-2', 'poc-3-rust-core': 'poc-3' };

const files = readdirSync(RESULTS).filter((f) => f.endsWith('.json'));
if (files.length === 0) {
  console.error('no results. Run: node bench/run.mjs');
  process.exit(1);
}

const timing = [];
const alloc = [];
let cold = null;
for (const f of files) {
  const j = JSON.parse(readFileSync(resolve(RESULTS, f), 'utf8'));
  if (j.mode === 'cold-start') cold = j;
  else if (j.mode === 'alloc') alloc.push(j);
  else timing.push(j);
}

const median = (xs) => {
  const s = [...xs].sort((a, b) => a - b);
  if (s.length === 0) return NaN;
  const m = (s.length - 1) / 2;
  return s.length % 2 ? s[m] : (s[Math.floor(m)] + s[Math.ceil(m)]) / 2;
};
const spread = (xs) => (xs.length < 2 ? 0 : (Math.max(...xs) - Math.min(...xs)) / median(xs));

/** cellKey -> adapter -> [ns from each round] */
function collect(workload, keyOf) {
  const table = new Map();
  for (const run of timing) {
    for (const row of run[workload]) {
      const key = keyOf(row);
      if (!table.has(key)) table.set(key, new Map());
      const byAdapter = table.get(key);
      if (!byAdapter.has(run.adapter)) byAdapter.set(run.adapter, []);
      byAdapter.get(run.adapter).push(workload === 'gesture' ? row.nsPerMove : row.ns);
    }
  }
  return table;
}

const fmt = (ns) => {
  if (!isFinite(ns)) return '     -';
  if (ns < 1000) return `${ns.toFixed(0)} ns`;
  if (ns < 1e6) return `${(ns / 1000).toFixed(2)} µs`;
  return `${(ns / 1e6).toFixed(2)} ms`;
};
const ratio = (a, b) => (isFinite(a) && isFinite(b) && b > 0 ? `${(a / b).toFixed(2)}x` : '-');

const md = [];
const env = timing[0]?.env ?? {};
const rounds = new Set(timing.map((t) => t.round)).size;

function section(title, table, cols) {
  const lines = [];
  lines.push(`\n### ${title}\n`);
  lines.push(`| ${cols} | poc-1 | poc-2 | poc-3 | poc-2/poc-1 | poc-3/poc-1 | worst spread |`);
  lines.push(`|---|---|---|---|---|---|---|`);
  for (const [key, byAdapter] of table) {
    const meds = ORDER.map((a) => median(byAdapter.get(a) ?? []));
    const spreads = ORDER.map((a) => spread(byAdapter.get(a) ?? []));
    lines.push(
      `| ${key} | ${fmt(meds[0])} | ${fmt(meds[1])} | ${fmt(meds[2])} | ` +
        `${ratio(meds[1], meds[0])} | ${ratio(meds[2], meds[0])} | ${(Math.max(...spreads) * 100).toFixed(0)}% |`,
    );
  }
  return lines.join('\n');
}

md.push(`# Interop benchmark results\n`);
md.push(
  `${rounds} interleaved rounds. ${env.runtime === 'node' ? `Node ${env.node}, ${env.platform}, ${env.cpu}` : env.userAgent ?? ''}.`,
);
md.push(
  `\nMedians across rounds. "worst spread" is the widest (max-min)/median of any single implementation ` +
    `for that cell: read a large value as "this row drifted, do not trust two significant figures".`,
);

md.push(
  section(
    'Per-frame projection',
    collect('frame', (r) => `n=${r.n} ${r.anchored ? 'anchored' : 'plain'} ${r.view}`),
    'cell',
  ),
);
// poc-3's undecoded frame, against its own decoded frame and poc-1's.
{
  const dec = collect('frame', (r) => `${r.n}|${r.anchored}|${r.view}`);
  const raw = collect('frameRaw', (r) => `${r.n}|${r.anchored}|${r.view}`);
  if (raw.size) {
    const lines = ['\n### poc-3: what the object decode costs\n'];
    lines.push(
      'poc-3 crosses the boundary once per frame either way. The only difference between these two ' +
        'columns is whether the returned `Float64Array` is turned into `DisplayList` objects, which ' +
        'is the same allocation poc-1 performs. If the decode dominates, moving the math to Rust ' +
        'cannot pay off until the renderer reads floats directly.\n',
    );
    lines.push('| cell | poc-1 (objects) | poc-3 (objects) | poc-3 (raw buffer) | decode share | raw vs poc-1 |');
    lines.push('|---|---|---|---|---|---|');
    for (const [key, byAdapter] of raw) {
      const p1 = median(dec.get(key)?.get('poc-1-ts') ?? []);
      const p3 = median(dec.get(key)?.get('poc-3-rust-core') ?? []);
      const p3raw = median(byAdapter.get('poc-3-rust-core') ?? []);
      const [n, anchored, view] = key.split('|');
      const share = isFinite(p3) && isFinite(p3raw) ? `${(((p3 - p3raw) / p3) * 100).toFixed(0)}%` : '-';
      lines.push(
        `| n=${n} ${anchored === 'true' ? 'anchored' : 'plain'} ${view} | ${fmt(p1)} | ${fmt(p3)} | ` +
          `${fmt(p3raw)} | ${share} | ${ratio(p3raw, p1)} |`,
      );
    }
    md.push(lines.join('\n'));
  }
}

md.push(section('Hit-test', collect('hit', (r) => `n=${r.n} ${r.kind}`), 'cell'));
md.push(section('Dispatch (ns per pointerMove)', collect('gesture', (r) => `n=${r.n}`), 'shapes'));

if (alloc.length) {
  const byAdapter = new Map();
  for (const a of alloc) {
    if (!byAdapter.has(a.adapter)) byAdapter.set(a.adapter, []);
    byAdapter.get(a.adapter).push(a);
  }
  const first = alloc[0];
  const calib = median(alloc.map((r) => r.calibration?.bytesPerObject ?? NaN));
  md.push(`\n### Allocation (${first.shapes} anchored shapes, GC-free window)\n`);
  md.push(
    `These are \`heapUsed\` deltas, not an allocation counter. Calibration: a \`{x, y}\` object ` +
      `measures ${calib.toFixed(0)} bytes here against a ~24 byte theoretical minimum, so treat the ` +
      `absolute figures as good to about 3x. The comparison between implementations is sound, since ` +
      `all three are measured the same way.\n`,
  );
  md.push(`| | bytes/frame | bytes/shape | window | vs poc-1 |`);
  md.push(`|---|---|---|---|---|`);
  const base = median((byAdapter.get('poc-1-ts') ?? []).map((r) => r.bytesPerFrame));
  for (const a of ORDER) {
    const runs = byAdapter.get(a) ?? [];
    if (!runs.length) continue;
    const bpf = median(runs.map((r) => r.bytesPerFrame));
    md.push(
      `| ${SHORT[a]} | ${bpf.toFixed(0)} | ${(bpf / first.shapes).toFixed(1)} | ` +
        `${median(runs.map((r) => r.frames)).toFixed(0)} frames | ${ratio(bpf, base)} |`,
    );
  }
}

if (cold) {
  md.push(`\n### Cold start\n`);
  md.push(`${cold.collectedBy}.\n`);
  md.push(`> ${cold.caveat}\n`);
  md.push(`| | wasm shipped | instantiate leg | first frame | first frame range |`);
  md.push(`|---|---|---|---|---|`);
  for (const a of ORDER) {
    const c = cold.pocs[a];
    if (!c) continue;
    md.push(
      `| ${SHORT[a]} | ${c.wasmBytes === 0 ? 'none' : `${(c.wasmBytes / 1024).toFixed(1)} kB`} | ` +
        `${c.wasmLegMs.toFixed(1)} ms | ${c.firstFrameMs.toFixed(1)} ms | ` +
        `${c.firstFrameRange[0].toFixed(1)} to ${c.firstFrameRange[1].toFixed(1)} ms |`,
    );
  }
}

// Crossover: the smallest n where poc-3 beats poc-1, anchored + projecting.
const frames = collect('frame', (r) => `${r.n}|${r.anchored}|${r.view}`);
const crossover = (anchored) => {
  const ns = [...frames.keys()]
    .filter((k) => k.endsWith(`|${anchored}|projecting`))
    .map((k) => ({ n: Number(k.split('|')[0]), byAdapter: frames.get(k) }))
    .sort((a, b) => a.n - b.n);
  for (const { n, byAdapter } of ns) {
    const p1 = median(byAdapter.get('poc-1-ts') ?? []);
    const p3 = median(byAdapter.get('poc-3-rust-core') ?? []);
    if (isFinite(p1) && isFinite(p3) && p3 < p1) return n;
  }
  return null;
};
md.push(`\n### Crossover\n`);
md.push(`poc-3 first beats poc-1 at n=${crossover(true) ?? 'never'} anchored, n=${crossover(false) ?? 'never'} plain, at the projecting view.`);

// Did poc-2 win anything? The spec says it should not.
const p2wins = [];
for (const [key, byAdapter] of frames) {
  const p1 = median(byAdapter.get('poc-1-ts') ?? []);
  const p2 = median(byAdapter.get('poc-2-rust-geometry') ?? []);
  const p3 = median(byAdapter.get('poc-3-rust-core') ?? []);
  if (isFinite(p2) && p2 < p1 && p2 < p3) p2wins.push(key);
}
md.push(
  `\npoc-2 wins ${p2wins.length} of ${frames.size} frame cells` +
    (p2wins.length ? `: ${p2wins.join(', ')}. The spec predicted zero, so treat these as suspect.` : `, as predicted.`),
);

const text = md.join('\n') + '\n';
process.stdout.write(text);
const out = resolve(here, 'RESULTS.md');
writeFileSync(out, text);
process.stdout.write(`\nwritten to ${out}\n`);
