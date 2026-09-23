/**
 * The parts of the measurement that are about being fair rather than about
 * geometry: statistics, arm rotation, and the toolchain record.
 *
 * Separated so the bits most likely to be got wrong are the bits under test.
 * The rules come from `multiplatform-plan.md` Section 9, which is itself the
 * output of a study that found three measurement bugs, none of which a timing
 * table would have revealed.
 */
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';

/** Milliseconds per iteration, summarized. */
export interface Distribution {
  readonly samples: number;
  readonly median: number;
  readonly p95: number;
  readonly min: number;
  readonly max: number;
}

/**
 * Median and p95 by nearest-rank on the sorted sample, which is what the
 * committed tables in `pdfium-binding-benchmarks/RESULTS.md` report.
 *
 * No mean. A mean over a laptop's timings is dominated by whatever else the
 * machine was doing, and Section 3 already records that the published p95
 * values there are batch-mean percentiles rather than call tail latency. Say
 * which statistic it is, every time.
 */
export function distribution(samples: readonly number[]): Distribution {
  if (samples.length === 0) throw new Error('a distribution needs at least one sample');
  const sorted = [...samples].sort((a, b) => a - b);
  const at = (q: number) => sorted[Math.min(sorted.length - 1, Math.ceil(q * sorted.length) - 1)];
  return {
    samples: sorted.length,
    median: at(0.5),
    p95: at(0.95),
    min: sorted[0],
    max: sorted[sorted.length - 1],
  };
}

/**
 * The arm order for one round, rotated by the round index.
 *
 * A laptop's clock speed drifts over a multi-minute run and background load
 * decays within each round, so a fixed order hands the first position to the
 * same arm every time. Rotating spreads each arm across every position. With
 * two arms this alternates, which is the whole of it; the function exists so
 * that adding a third arm does not mean remembering why.
 */
export function rotate<T>(arms: readonly T[], round: number): T[] {
  const shift = (round - 1) % arms.length;
  return [...arms.slice(shift), ...arms.slice(0, shift)];
}

/**
 * A value that cannot be optimized away.
 *
 * The reads under test return a large object graph, and an engine that can
 * prove nothing reads it is entitled to skip building it. Folding a number out
 * of every snapshot and returning it to a caller that prints it keeps the work
 * real. Section 9 calls this "consumed results".
 */
export function consume(snapshot: { runs: { charStart: number; glyphs: unknown[] }[] }): number {
  let total = 0;
  for (const run of snapshot.runs) total += run.charStart + run.glyphs.length;
  return total;
}

export interface Toolchain {
  readonly node: string;
  readonly rustc: string;
  readonly emcc: string;
  readonly git: string;
  /** sha256 of the wasm the measurement actually loaded. */
  readonly wasm: string;
}

function firstLine(command: string, args: string[]): string {
  try {
    return execFileSync(command, args, { encoding: 'utf8' }).trim().split('\n')[0];
  } catch {
    return 'unknown';
  }
}

/**
 * What produced these numbers, recorded beside them.
 *
 * A timing without a toolchain is not a baseline, because the next person
 * cannot tell a regression from a different compiler. The wasm hash is the
 * load-bearing one: it identifies the exact module, including whether the Rust
 * core was linked into it.
 */
export function toolchain(wasmPath: string): Toolchain {
  return {
    node: process.version,
    rustc: firstLine('rustc', ['--version']),
    emcc: firstLine('emcc', ['--version']),
    git: firstLine('git', ['rev-parse', 'HEAD']),
    wasm: createHash('sha256').update(readFileSync(wasmPath)).digest('hex'),
  };
}

/**
 * Deep equality that ignores object key order, returning the first path that
 * differs.
 *
 * Key order is not part of these DTOs. `PdfRect` is declared
 * `left, bottom, right, top` in `core/src/geometry/primitives.ts`, which is
 * what the Rust struct emits, while the TypeScript reader happens to build its
 * literals `left, right, bottom, top`. Both are the same rectangle. Comparing
 * serialized text would fail on that and call it a divergence, which is how a
 * benchmark ends up reporting a bug that is not there.
 *
 * Numbers compare with `Object.is`, so this is exact: both arms read the same
 * PDFium in the same module, and a float that differs at all differs for a
 * reason worth finding.
 */
export function firstDifference(
  path: string,
  expected: unknown,
  actual: unknown,
): string | undefined {
  if (Array.isArray(expected) || Array.isArray(actual)) {
    if (!Array.isArray(expected) || !Array.isArray(actual)) {
      return `${path}: one side is an array and the other is not`;
    }
    if (expected.length !== actual.length) {
      return `${path}: ${expected.length} items against ${actual.length}`;
    }
    for (let i = 0; i < expected.length; i++) {
      const found = firstDifference(`${path}[${i}]`, expected[i], actual[i]);
      if (found) return found;
    }
    return undefined;
  }
  if (expected && actual && typeof expected === 'object' && typeof actual === 'object') {
    for (const key of new Set([...Object.keys(expected), ...Object.keys(actual)])) {
      const found = firstDifference(
        `${path}.${key}`,
        (expected as Record<string, unknown>)[key],
        (actual as Record<string, unknown>)[key],
      );
      if (found) return found;
    }
    return undefined;
  }
  return Object.is(expected, actual)
    ? undefined
    : `${path}: ${JSON.stringify(expected)} against ${JSON.stringify(actual)}`;
}
