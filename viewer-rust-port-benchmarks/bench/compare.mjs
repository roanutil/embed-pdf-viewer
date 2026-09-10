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
const ORDER = ['typescript-baseline', 'rust-geometry-only', 'rust-geometry-and-state'];
const SHORT = { 'typescript-baseline': 'typescript-baseline', 'rust-geometry-only': 'rust-geometry-only', 'rust-geometry-and-state': 'rust-geometry-and-state' };

const files = readdirSync(RESULTS).filter((f) => f.endsWith('.json'));
if (files.length === 0) {
  console.error('no results. Run: node bench/run.mjs');
  process.exit(1);
}

const timing = [];
const alloc = [];
let cold = null;
// Match `mode` exactly rather than treating everything that is not cold-start
// or alloc as timing. `bench/run.mjs` writes `*-probe-r1.json` for the fairness
// gate and only clears per-round files at the START of a run, so the probes are
// still on disk when compare.mjs runs. A probe payload has no `frame` array,
// and `collect()` below would throw `run[workload] is not iterable` on it.
for (const f of files) {
  const j = JSON.parse(readFileSync(resolve(RESULTS, f), 'utf8'));
  if (j.mode === 'cold-start') cold = j;
  else if (j.mode === 'alloc') {
    if (j.allocationMetric !== 'heapUsed+arrayBuffers') {
      throw new Error(`${f}: obsolete allocation metric; rerun allocation rounds before comparing`);
    }
    alloc.push(j);
  }
  else if (j.mode === 'timing') timing.push(j);
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
  lines.push(`| ${cols} | typescript-baseline | rust-geometry-only | rust-geometry-and-state | rust-geometry-only/typescript-baseline | rust-geometry-and-state/typescript-baseline | worst spread |`);
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
    `for that cell. A large value indicates unstable timings without identifying the cause.`,
);

md.push(
  section(
    'Per-frame projection',
    collect('frame', (r) => `n=${r.n} ${r.anchored ? 'anchored' : 'plain'} ${r.view}`),
    'cell',
  ),
);
// rust-geometry-and-state's undecoded frame, against its own decoded frame and typescript-baseline's.
{
  const dec = collect('frame', (r) => `${r.n}|${r.anchored}|${r.view}`);
  const raw = collect('frameRaw', (r) => `${r.n}|${r.anchored}|${r.view}`);
  if (raw.size) {
    const lines = ['\n### rust-geometry-and-state: what the object decode costs\n'];
    lines.push(
      'The Rust columns compare returning a numeric buffer with converting it into JavaScript display objects. ' +
        'Decode share is the relative difference between recorded medians, not a separately timed conversion. ' +
        'Both paths use one Wasm call once the metadata cache is current.\n',
    );
    lines.push('| cell | typescript-baseline (objects) | rust-geometry-and-state (objects) | rust-geometry-and-state (raw buffer) | decode share | raw vs typescript-baseline |');
    lines.push('|---|---|---|---|---|---|');
    for (const [key, byAdapter] of raw) {
      const p1 = median(dec.get(key)?.get('typescript-baseline') ?? []);
      const p3 = median(dec.get(key)?.get('rust-geometry-and-state') ?? []);
      const p3raw = median(byAdapter.get('rust-geometry-and-state') ?? []);
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
md.push(section('State updates (time per pointerMove)', collect('gesture', (r) => `n=${r.n}`), 'shapes'));

if (alloc.length) {
  const byAdapter = new Map();
  for (const a of alloc) {
    if (!byAdapter.has(a.adapter)) byAdapter.set(a.adapter, []);
    byAdapter.get(a.adapter).push(a);
  }
  const first = alloc[0];
  const calib = median(alloc.map((r) => r.calibration?.bytesPerObject ?? NaN));
  md.push(`\n### Allocation (${first.shapes} anchored shapes, window with no observed GC)\n`);
  md.push(
    `These are \`heapUsed + arrayBuffers\` deltas over a window with no observed GC, not an allocation counter. ` +
      `They include copied typed-array backing stores but exclude Rust's internal allocations. ` +
      `Heap calibration: a \`{x, y}\` object ` +
      `measures ${calib.toFixed(0)} bytes in this run. This is a calibration observation, not an accuracy bound. ` +
      `Backing-store bytes are accounted separately.\n`,
  );
  md.push(`| | bytes/frame | heap/frame | backing stores/frame | bytes/shape | window | vs typescript-baseline |`);
  md.push(`|---|---|---|---|---|---|---|`);
  const base = median((byAdapter.get('typescript-baseline') ?? []).map((r) => r.bytesPerFrame));
  for (const a of ORDER) {
    const runs = byAdapter.get(a) ?? [];
    if (!runs.length) continue;
    const bpf = median(runs.map((r) => r.bytesPerFrame));
    md.push(
      `| ${SHORT[a]} | ${bpf.toFixed(0)} | ` +
        `${median(runs.map((r) => r.heapBytesPerFrame)).toFixed(0)} | ` +
        `${median(runs.map((r) => r.arrayBufferBytesPerFrame)).toFixed(0)} | ${(bpf / first.shapes).toFixed(1)} | ` +
        `${median(runs.map((r) => r.frames)).toFixed(0)} frames | ${ratio(bpf, base)} |`,
    );
  }
}

if (!cold) {
  // Loud, because this section going missing is how the last regeneration lost
  // it: `run.mjs` used to delete the whole results directory, cold-start.json
  // included, and RESULTS.md came back a section short with no complaint.
  console.error(
    `\nWARNING: results/cold-start.json is missing, so RESULTS.md has no Cold start section.\n` +
      `  It is hand-collected from a browser run. See bench/README.md, and restore it with\n` +
      `  \`git checkout viewer-rust-port-benchmarks/bench/results/cold-start.json\` if it was deleted.`,
  );
}

if (cold) {
  md.push(`\n### Cold start\n`);
  md.push(`${cold.collectedBy}.\n`);
  md.push(`> ${cold.caveat}\n`);
  md.push('The ready interval is wasmReady minus boot. First frame is recorded by a React effect after the initial viewer render, relative to navigation start; it is not a paint measurement.\n');
  md.push(`| | wasm shipped | ready interval | first frame | first frame range |`);
  md.push(`|---|---|---|---|---|`);
  for (const a of ORDER) {
    const c = cold.pocs[a];
    if (!c) continue;
    md.push(
      `| ${SHORT[a]} | ${c.wasmBytes === 0 ? 'none' : `${(c.wasmBytes / 1024).toFixed(1)} KiB`} | ` +
        `${c.wasmLegMs.toFixed(1)} ms | ${c.firstFrameMs.toFixed(1)} ms | ` +
        `${c.firstFrameRange[0].toFixed(1)} to ${c.firstFrameRange[1].toFixed(1)} ms |`,
    );
  }
}

// Crossover: the smallest n where rust-geometry-and-state beats typescript-baseline, anchored + projecting.
const frames = collect('frame', (r) => `${r.n}|${r.anchored}|${r.view}`);
const crossover = (anchored) => {
  const ns = [...frames.keys()]
    .filter((k) => k.endsWith(`|${anchored}|projecting`))
    .map((k) => ({ n: Number(k.split('|')[0]), byAdapter: frames.get(k) }))
    .sort((a, b) => a.n - b.n);
  for (const { n, byAdapter } of ns) {
    const p1 = median(byAdapter.get('typescript-baseline') ?? []);
    const p3 = median(byAdapter.get('rust-geometry-and-state') ?? []);
    if (isFinite(p1) && isFinite(p3) && p3 < p1) return n;
  }
  return null;
};
md.push(`\n### Crossover\n`);
md.push(`At the projecting view, the first tested shape count where rust-geometry-and-state beats typescript-baseline is ${crossover(true) ?? 'none of the tested counts'} for anchored shapes and ${crossover(false) ?? 'none of the tested counts'} for plain shapes. This does not establish behavior outside those cases.`);

// Did rust-geometry-only win anything? The spec says it should not.
const p2wins = [];
for (const [key, byAdapter] of frames) {
  const p1 = median(byAdapter.get('typescript-baseline') ?? []);
  const p2 = median(byAdapter.get('rust-geometry-only') ?? []);
  const p3 = median(byAdapter.get('rust-geometry-and-state') ?? []);
  if (isFinite(p2) && p2 < p1 && p2 < p3) p2wins.push(key);
}
md.push(
  `\nrust-geometry-only has the lowest recorded median in ${p2wins.length} of ${frames.size} frame cells` +
    (p2wins.length ? `: ${p2wins.join(', ')}.` : '.'),
);

const text = md.join('\n') + '\n';
process.stdout.write(text);
const out = resolve(here, 'RESULTS.md');
writeFileSync(out, text);
process.stdout.write(`\nwritten to ${out}\n`);
