/**
 * Zero-dependency sampling and statistics. Runs unchanged in Node and in a
 * browser, because both provide `performance.now()` as a global.
 *
 * The measurement strategy is batch means. We auto-tune a batch size so one
 * batch takes at least TARGET_BATCH_MS, time whole batches, and divide. Timing
 * individual calls would let `performance.now()` dominate a 50 ns function.
 *
 * The consequence, and it is a real caveat: the reported p95 is the p95 of BATCH
 * MEANS, not of individual calls. It captures run-to-run drift, not per-call
 * tail latency. Do not read it as a worst-case frame time.
 */

const TARGET_BATCH_MS = 2;
const MAX_BATCH = 5_000_000;
const MAX_BATCHES = 2000;

/**
 * Every measured function returns a number and we accumulate it here. Without
 * this V8 is entitled to delete the call entirely, and a benchmark of nothing
 * looks wonderful.
 */
let sink = 0;

export const sinkValue = () => sink;

function quantile(sorted, q) {
  if (sorted.length === 0) return NaN;
  if (sorted.length === 1) return sorted[0];
  const pos = (sorted.length - 1) * q;
  const lo = Math.floor(pos);
  const hi = Math.ceil(pos);
  return sorted[lo] + (sorted[hi] - sorted[lo]) * (pos - lo);
}

export function summarize(samples) {
  const s = [...samples].sort((a, b) => a - b);
  return {
    median: quantile(s, 0.5),
    p95: quantile(s, 0.95),
    min: s[0],
    max: s[s.length - 1],
    batches: s.length,
  };
}

function tuneBatch(fn) {
  let batch = 1;
  for (let guard = 0; guard < 40; guard++) {
    const t0 = performance.now();
    let acc = 0;
    for (let i = 0; i < batch; i++) acc += fn();
    const dt = performance.now() - t0;
    sink += acc;
    if (dt >= TARGET_BATCH_MS) return batch;
    if (batch >= MAX_BATCH) return MAX_BATCH;
    // dt can be exactly 0 on a coarse timer; grow hard rather than dividing by it.
    const factor = dt <= 0 ? 8 : Math.max(2, Math.ceil((TARGET_BATCH_MS / dt) * 1.5));
    batch = Math.min(batch * factor, MAX_BATCH);
  }
  return batch;
}

/**
 * @param {() => number} fn  must return a number, which is accumulated
 * @returns {{ns:number, p95:number, min:number, max:number, batches:number, batch:number, iters:number}}
 */
export function measure(fn, opts = {}) {
  const { warmupIters = 10000, warmupMs = 200, budgetMs = 500 } = opts;

  // Warm up to V8's optimizing tier, stopping at whichever limit comes first.
  const w0 = performance.now();
  let warmed = 0;
  let acc = 0;
  while (warmed < warmupIters && performance.now() - w0 < warmupMs) {
    acc += fn();
    warmed++;
  }
  sink += acc;

  const batch = tuneBatch(fn);

  const perIterNs = [];
  let iters = 0;
  const s0 = performance.now();
  while (performance.now() - s0 < budgetMs && perIterNs.length < MAX_BATCHES) {
    const t0 = performance.now();
    let a = 0;
    for (let i = 0; i < batch; i++) a += fn();
    const dt = performance.now() - t0;
    sink += a;
    perIterNs.push((dt * 1e6) / batch);
    iters += batch;
  }

  const s = summarize(perIterNs);
  return { ns: s.median, p95: s.p95, min: s.min, max: s.max, batches: s.batches, batch, iters, warmed };
}

/** Environment stamp. Absolute numbers are machine-specific; a results file
 *  without this is a trap for whoever reads it in six months. */
export async function environment() {
  if (typeof process === 'undefined' || !process.versions?.node) {
    return {
      runtime: 'browser',
      userAgent: typeof navigator !== 'undefined' ? navigator.userAgent : 'unknown',
      cores: typeof navigator !== 'undefined' ? navigator.hardwareConcurrency : null,
    };
  }
  const os = await import('node:os');
  return {
    runtime: 'node',
    node: process.version,
    platform: `${os.platform()} ${os.release()}`,
    arch: os.arch(),
    cpu: os.cpus()[0]?.model ?? 'unknown',
    cores: os.cpus().length,
    loadavg: os.loadavg()[0],
    note: 'macOS thermal throttling is not controllable; rounds are interleaved to spread drift.',
  };
}

/** Rounds a display list to a comparable form for the fairness gate. */
export function normalizeDisplayList(items) {
  const r = (v) => Math.round(v * 1e9) / 1e9;
  return items
    .map((i) => ({
      id: i.id,
      quad: i.quad.map((p) => [r(p.x), r(p.y)]),
      color: i.color,
      selected: !!i.selected,
      projected: !!i.projected,
    }))
    .sort((a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0));
}

/**
 * Indices of display items that differ between two normalized lists.
 *
 * `normalizeDisplayList` rounds to 1e-9 so the lists print comparably, but a
 * byte compare of the rounded output can still fail when two implementations
 * land one ulp apart on either side of a rounding boundary. Coordinates are
 * compared within `tolerance`; ids, colours and flags must match exactly. A
 * length mismatch reports every index only one side has.
 */
export function displayListDifferences(a, b, tolerance = 1e-9) {
  const out = [];
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    if (!sameItem(a[i], b[i], tolerance)) out.push(i);
  }
  return out;
}

function sameItem(x, y, tolerance) {
  if (!x || !y) return false;
  if (x.id !== y.id || x.color !== y.color || x.selected !== y.selected || x.projected !== y.projected) return false;
  if (x.quad.length !== y.quad.length) return false;
  return x.quad.every(([px, py], i) => {
    const [qx, qy] = y.quad[i];
    return Math.abs(px - qx) <= tolerance && Math.abs(py - qy) <= tolerance;
  });
}
