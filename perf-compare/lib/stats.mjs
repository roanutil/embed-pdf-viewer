const FLOOR = 0.03;

function quantile(sorted, q) {
  if (sorted.length === 1) return sorted[0];
  const pos = (sorted.length - 1) * q;
  const lo = Math.floor(pos);
  const hi = Math.ceil(pos);
  return sorted[lo] + (sorted[hi] - sorted[lo]) * (pos - lo);
}

function sorted(values) {
  if (!values.length) throw new Error('A summary needs at least one sample');
  if (values.some((v) => !Number.isFinite(v))) throw new Error('Samples must be finite');
  return [...values].sort((a, b) => a - b);
}

export function summarize(values) {
  const s = sorted(values);
  return { n: s.length, median: quantile(s, 0.5), p95: quantile(s, 0.95), min: s[0], max: s[s.length - 1] };
}

// mulberry32: small, seedable, and good enough for resampling indices.
function rng(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const median = (values) => quantile(sorted(values), 0.5);

function resample(values, next) {
  const out = new Array(values.length);
  for (let i = 0; i < values.length; i++) out[i] = values[Math.floor(next() * values.length)];
  return out;
}

export function bootstrapRatio(reference, candidate, { resamples = 10_000, seed = 1 } = {}) {
  if (![...reference, ...candidate].every((v) => v > 0)) {
    throw new Error('A ratio needs positive samples; use bootstrapDifference');
  }
  const next = rng(seed);
  const ratios = new Array(resamples);
  for (let i = 0; i < resamples; i++) {
    ratios[i] = median(resample(candidate, next)) / median(resample(reference, next));
  }
  ratios.sort((a, b) => a - b);
  return {
    kind: 'ratio',
    ratio: median(candidate) / median(reference),
    lo: quantile(ratios, 0.025),
    hi: quantile(ratios, 0.975),
  };
}

// For metrics that can be zero (longTaskMs, settleMs, counts) or negative
// (heapSoakSlopeBytes), where a ratio is undefined or points the wrong way.
export function bootstrapDifference(reference, candidate, { resamples = 10_000, seed = 1 } = {}) {
  const next = rng(seed);
  const diffs = new Array(resamples);
  for (let i = 0; i < resamples; i++) {
    diffs[i] = median(resample(candidate, next)) - median(resample(reference, next));
  }
  diffs.sort((a, b) => a - b);
  return {
    kind: 'difference',
    diff: median(candidate) - median(reference),
    lo: quantile(diffs, 0.025),
    hi: quantile(diffs, 0.975),
    scale: Math.abs(median(reference)),
  };
}

function checkFinite(interval) {
  const point = interval.kind === 'difference' ? interval.diff : interval.ratio;
  if (![point, interval.lo, interval.hi].every(Number.isFinite)) {
    throw new Error(`Interval must be finite: ${JSON.stringify(interval)}`);
  }
}

export function noiseBand(interval) {
  const centre = interval.kind === 'difference' ? 0 : 1;
  return Math.max(Math.abs(centre - interval.lo), Math.abs(interval.hi - centre));
}

// Every metric is lower-is-better, so "slower" means the candidate is worse,
// whatever the unit.
export function verdict(interval, noise) {
  checkFinite(interval);
  const { lo, hi } = interval;
  if (interval.kind === 'difference') {
    // The 3% floor is relative to the reference median. With a zero
    // reference it is 0, and any difference the interval resolves counts.
    const threshold = Math.max(FLOOR * interval.scale, noise);
    if (lo > 0 && interval.diff > threshold) return 'slower';
    if (hi < 0 && -interval.diff > threshold) return 'faster';
    return 'same';
  }
  const threshold = Math.max(FLOOR, noise);
  if (lo > 1 && interval.ratio - 1 > threshold) return 'slower';
  if (hi < 1 && 1 - interval.ratio > threshold) return 'faster';
  return 'same';
}

// The A/A pair is judged against the fixed floor alone. Using its own band
// would make it pass by construction: the band is at least as wide as the
// pair's distance from 1.0.
export function calibrate(aa) {
  return { ok: verdict(aa, 0) === 'same', noise: noiseBand(aa) };
}

export function comparePair(interval, calibration) {
  return calibration.ok ? verdict(interval, calibration.noise) : 'unreliable';
}

export function slope(ys) {
  const n = ys.length;
  const mx = (n - 1) / 2;
  const my = ys.reduce((a, b) => a + b, 0) / n;
  let num = 0;
  let den = 0;
  ys.forEach((y, x) => {
    num += (x - mx) * (y - my);
    den += (x - mx) ** 2;
  });
  return den === 0 ? 0 : num / den;
}
