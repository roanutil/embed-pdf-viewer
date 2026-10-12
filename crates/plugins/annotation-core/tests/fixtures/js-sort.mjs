// Regenerates js-sort.json: what V8's `Array.prototype.sort` does with a
// comparator that answers NaN, for `tests/js_sort.rs` to hold the Rust port
// (`js::sort_by`) to. Run with `node js-sort.mjs > js-sort.json`.
//
// Each case is rebuilt on both sides from the same mulberry32 stream, so the
// fixture stores only the seed, the shape and a hash of the sorted order.

const mulberry32 = (seed) => () => {
  seed = (seed + 0x6d2b79f5) | 0;
  let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
  t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
  return (t ^ (t >>> 14)) >>> 0;
};

export function keys(kind, n, seed) {
  const next = mulberry32(seed);
  const out = [];
  if (kind === 'noisy') {
    for (let i = 0; i < n; i++) {
      const r = next();
      out.push(r % 5 === 0 ? NaN : r % 20);
    }
  } else if (kind === 'runs') {
    let v = 0;
    while (out.length < n) {
      const len = 1 + (next() % 200);
      const down = next() % 4 === 0;
      for (let i = 0; i < len && out.length < n; i++) {
        const r = next();
        v += down ? -(r % 3) : r % 3;
        out.push(r % 100 === 0 ? NaN : v);
      }
    }
  } else if (kind === 'sorted-nan') {
    for (let i = 0; i < n; i++) out.push(i % 37 === 36 ? NaN : i);
  } else {
    for (let i = 0; i < n; i++) out.push(next() % 1000);
  }
  return out;
}

export const hash = (order) =>
  order.reduce((h, i) => Math.imul(h ^ i, 16777619) >>> 0, 2166136261);

const SIZES = [0, 1, 2, 3, 7, 31, 32, 63, 64, 65, 100, 127, 128, 129, 500, 1000, 2049, 5000];
const KINDS = ['noisy', 'runs', 'sorted-nan', 'clean'];

const cases = [];
for (const kind of KINDS) {
  for (const n of SIZES) {
    const seed = n * 7919 + kind.length;
    const k = keys(kind, n, seed);
    const order = k.map((_, i) => i).sort((a, b) => k[a] - k[b]);
    cases.push({ kind, n, seed, hash: hash(order) });
  }
}
process.stdout.write(`${JSON.stringify({ v8: process.versions.v8, cases }, null, 1)}\n`);
