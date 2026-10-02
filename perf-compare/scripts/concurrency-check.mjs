// Can two runs share a host? Compares a solo run with runs that timed the
// same arms at the same time on the same host.
//
//   node scripts/concurrency-check.mjs <base arm> <arm> <solo run> <concurrent run> [<concurrent run> ...]
//
// Concurrency may slow every arm; that alone is fine. What it must not do is
// move the comparison. Per row (a flow's metric, positive samples only):
//
// - ratio: the concurrent runs' arm/base ratio, pooled, lies inside the solo
//   ratio's 95% interval or within 3% of it (the A/A rule);
// - shift: each arm's concurrent median over its solo median, how much the
//   load slowed it;
// - agreement: the concurrent runs against each other, per arm, as an A/A
//   pair: the interval includes 1 or the ratio is within 3% of it.
//
// It passes when the ratio holds on at least 95% of rows and the runs agree
// on at least 95%.
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { bootstrapRatio } from '../lib/stats.mjs';

const [base, arm, solo, ...concurrent] = process.argv.slice(2);
if (!concurrent.length) {
  console.error('usage: node scripts/concurrency-check.mjs <base arm> <arm> <solo run> <concurrent run> [...]');
  process.exit(2);
}

function samples(dir) {
  const out = new Map();
  for (const line of readFileSync(path.join(dir, 'samples.jsonl'), 'utf8').split('\n')) {
    if (!line) continue;
    const r = JSON.parse(line);
    if (r.error) continue;
    for (const [metric, v] of Object.entries(r.metrics ?? {})) {
      if (typeof v !== 'number') continue;
      const row = r.flow === 'open' ? `open(${r.fixture}) / ${metric}` : `${r.flow} / ${metric}`;
      const key = `${row}\u0000${r.arm}`;
      if (!out.has(key)) out.set(key, []);
      out.get(key).push(v);
    }
  }
  return out;
}

const median = (xs) => {
  const s = [...xs].sort((a, b) => a - b);
  const m = s.length >> 1;
  return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2;
};
const within = (r) => Math.abs(r - 1) <= 0.03;

const soloSamples = samples(solo);
const runs = concurrent.map(samples);
const pooled = (row, a) => runs.flatMap((s) => s.get(`${row}\u0000${a}`) ?? []);

const rows = [...new Set([...soloSamples.keys()].map((k) => k.split('\u0000')[0]))].sort();
const results = [];
let skipped = 0;
for (const row of rows) {
  const sb = soloSamples.get(`${row}\u0000${base}`);
  const sa = soloSamples.get(`${row}\u0000${arm}`);
  const cb = pooled(row, base);
  const ca = pooled(row, arm);
  if (!sb || !sa || !cb.length || !ca.length || ![...sb, ...sa, ...cb, ...ca].every((v) => v > 0)) {
    skipped++;
    continue;
  }
  const soloRatio = bootstrapRatio(sb, sa);
  const concurrentRatio = median(ca) / median(cb);
  const ratioHolds =
    (concurrentRatio >= soloRatio.lo && concurrentRatio <= soloRatio.hi) ||
    within(concurrentRatio / soloRatio.ratio);
  const shift = { [base]: median(cb) / median(sb), [arm]: median(ca) / median(sa) };
  let agree = true;
  if (runs.length > 1) {
    for (const a of [base, arm]) {
      const [x, y] = [runs[0].get(`${row}\u0000${a}`), runs[1].get(`${row}\u0000${a}`)];
      if (!x || !y) continue;
      const aa = bootstrapRatio(x, y);
      if (!((aa.lo <= 1 && aa.hi >= 1) || within(aa.ratio))) agree = false;
    }
  }
  results.push({ row, soloRatio, concurrentRatio, ratioHolds, shift, agree });
}

const pct = (n) => `${((100 * n) / results.length).toFixed(0)}%`;
const holds = results.filter((r) => r.ratioHolds).length;
const agrees = results.filter((r) => r.agree).length;
const shifts = (a) => results.map((r) => r.shift[a]).sort((x, y) => x - y);
console.log(`${results.length} rows compared, ${skipped} skipped (a zero or missing sample)`);
console.log(`ratio ${arm}/${base} holds under concurrency: ${holds} of ${results.length} (${pct(holds)})`);
if (runs.length > 1) console.log(`concurrent runs agree with each other: ${agrees} of ${results.length} (${pct(agrees)})`);
for (const a of [base, arm]) {
  const s = shifts(a);
  console.log(`${a} slowed by concurrency: median ${median(s).toFixed(3)}x, min ${s[0].toFixed(3)}x, max ${s.at(-1).toFixed(3)}x`);
}
const failing = results.filter((r) => !r.ratioHolds || !r.agree);
if (failing.length) {
  console.log('rows that fail:');
  for (const r of failing) {
    console.log(
      `  ${r.row}: solo ${r.soloRatio.ratio.toFixed(3)} [${r.soloRatio.lo.toFixed(3)}, ${r.soloRatio.hi.toFixed(3)}], ` +
        `concurrent ${r.concurrentRatio.toFixed(3)}${r.ratioHolds ? '' : ' (ratio moved)'}${r.agree ? '' : ' (runs disagree)'}`,
    );
  }
}
const pass = holds >= 0.95 * results.length && agrees >= 0.95 * results.length;
console.log(pass ? 'PASS: runs can share this host' : 'FAIL: run one at a time on this host');
process.exit(pass ? 0 : 1);
