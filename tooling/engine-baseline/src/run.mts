/**
 * The Phase 0 `pages.geometry` baseline.
 *
 * `multiplatform-plan.md` Section 10: "Phase 0 measurement of `pages.geometry`
 * runs through the unchanged service boundary with run materialization,
 * transfer and allocation counted." So this times
 * `doc.page(pon).geometry.read()`, not a reader in isolation: the borrow, the
 * envelope, the wire and the decode are all inside the measurement, because
 * they are all inside what a caller pays.
 *
 * Two arms, one runtime. `typescript` is the shipping `PageGeometryReader`;
 * `rust` is `RustPageGeometryReader`. Both run against the same module, so the
 * only difference between them is where the glyph loop happens.
 *
 * Run it:
 *
 *   # build a runtime with the Rust core linked in, per
 *   # tooling/engine-conformance/README.md, then swap it into
 *   # packages/engine/runtime/npm/wasm32/lib
 *   pnpm --filter @embedpdf/tooling-engine-baseline run baseline
 *
 * `--expose-gc` is required, not optional: the allocation pass is meaningless
 * without a forced collection between samples, and silently reporting
 * whatever the heap happened to look like would be worse than refusing.
 */
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

import type { PageGeometrySnapshot } from '@embedpdf/engine-core/runtime';
import { createPdfRuntime } from '@embedpdf/engine-runtime';
import { InlineTransport, LocalEngine } from '@embedpdf/engine';

import {
  consume,
  distribution,
  firstDifference,
  rotate,
  toolchain,
  type Distribution,
} from './protocol.mts';

const repoRoot = resolve(import.meta.dirname, '../../..');
const resultsDir = resolve(import.meta.dirname, '../results');

const ARMS = ['typescript', 'rust'] as const;
type Arm = (typeof ARMS)[number];

/**
 * The timing fixture and its dense page, plus the orientation fixture.
 *
 * `report.pdf` page 4 is the densest page in the corpus at 8,106 glyphs, which
 * is where per-glyph boundary crossings would show if they showed anywhere.
 * `embedded_orientations.pdf` is there so the rotated-run branch is timed at
 * all, not because one page of it is a workload.
 */
const FIXTURES = [
  { label: 'report p4', path: 'pdfium-multiplatform-prototype/fixtures/report.pdf', page: 4 },
  { label: 'report p0', path: 'pdfium-multiplatform-prototype/fixtures/report.pdf', page: 0 },
  {
    label: 'orientations p0',
    path: 'tooling/engine-conformance/fixtures/embedded_orientations.pdf',
    page: 0,
  },
] as const;

const ROUNDS = Number(process.env.BASELINE_ROUNDS ?? 7);
const ITERATIONS = Number(process.env.BASELINE_ITERATIONS ?? 20);
const WARMUP = Number(process.env.BASELINE_WARMUP ?? 5);

interface Reader {
  read(): Promise<PageGeometrySnapshot>;
  close(): Promise<void>;
}

async function openReader(arm: Arm, fixture: (typeof FIXTURES)[number]): Promise<Reader> {
  const runtime = await createPdfRuntime({ prefer: 'wasm' });
  if (arm === 'rust' && !runtime.rust) {
    throw new Error(
      'this runtime has no Rust core linked in, so the rust arm cannot run.\n' +
        'Build one with the recipe in tooling/engine-conformance/README.md and swap it into\n' +
        'packages/engine/runtime/npm/wasm32/lib.',
    );
  }
  const engine = LocalEngine.fromTransport({
    transport: new InlineTransport(runtime, { rustReads: arm === 'rust' }),
  });
  const bytes = new Uint8Array(readFileSync(resolve(repoRoot, fixture.path)));
  const doc = await engine.open({ kind: 'bytes', id: `baseline-${arm}`, bytes }, { scope: ['*'] });
  const { pages } = await doc.pages.list();
  const page = pages.find((candidate) => candidate.index === fixture.page);
  if (!page) throw new Error(`${fixture.label}: no page at index ${fixture.page}`);
  const handle = doc.page(page.pageObjectNumber);

  return {
    read: () => handle.geometry.read(),
    close: async () => {
      await doc.close();
      await engine.destroy();
    },
  };
}

/**
 * The answer gate, before any timing.
 *
 * A wrong arm can be fast and no timing table would show it. This is the same
 * comparison `packages/engine/main/test/geometry-rust-parity.test.ts` makes,
 * repeated here because a benchmark that trusts a test in another package to
 * have run is a benchmark that will eventually publish a number for two
 * different computations.
 */
async function gate(): Promise<number> {
  let compared = 0;
  for (const fixture of FIXTURES) {
    const snapshots: Record<string, PageGeometrySnapshot> = {};
    for (const arm of ARMS) {
      const reader = await openReader(arm, fixture);
      try {
        snapshots[arm] = await reader.read();
      } finally {
        await reader.close();
      }
    }
    const [first, ...rest] = ARMS;
    for (const arm of rest) {
      const difference = firstDifference(fixture.label, snapshots[first], snapshots[arm]);
      if (difference) {
        throw new Error(
          `${first} and ${arm} disagree at ${difference}. Timings would be meaningless.`,
        );
      }
    }
    compared += 1;
  }
  return compared;
}

interface RoundSample {
  readonly arm: Arm;
  readonly fixture: string;
  readonly round: number;
  /** Milliseconds for one `geometry.read()`. */
  readonly perRead: number[];
  /** Folded from every snapshot so the work cannot be elided. */
  readonly checksum: number;
}

async function timeOne(
  arm: Arm,
  fixture: (typeof FIXTURES)[number],
  round: number,
): Promise<RoundSample> {
  const reader = await openReader(arm, fixture);
  try {
    let checksum = 0;
    for (let i = 0; i < WARMUP; i++) checksum += consume(await reader.read());

    const perRead: number[] = [];
    for (let i = 0; i < ITERATIONS; i++) {
      const started = performance.now();
      const snapshot = await reader.read();
      perRead.push(performance.now() - started);
      checksum += consume(snapshot);
    }
    return { arm, fixture: fixture.label, round, perRead, checksum };
  } finally {
    await reader.close();
  }
}

/**
 * Bytes the heap keeps for one snapshot, with a forced collection either side.
 *
 * Run materialization and the decode are the whole question for the Rust arm:
 * it receives JSON and has to build the same object graph the TypeScript
 * reader builds directly. Section 10 asks for allocation to be counted, and
 * this is the honest cheap version of that, not a heap profile.
 */
async function measureAllocation(arm: Arm, fixture: (typeof FIXTURES)[number]): Promise<number> {
  const reader = await openReader(arm, fixture);
  try {
    for (let i = 0; i < WARMUP; i++) consume(await reader.read());
    global.gc!();
    const before = process.memoryUsage().heapUsed;
    const held: PageGeometrySnapshot[] = [];
    for (let i = 0; i < 10; i++) held.push(await reader.read());
    global.gc!();
    const after = process.memoryUsage().heapUsed;
    // Touch the retained snapshots so they cannot be collected before the
    // second reading.
    let total = 0;
    for (const snapshot of held) total += consume(snapshot);
    if (total === 0) throw new Error('empty snapshots: the allocation reading would be noise');
    return Math.round((after - before) / held.length);
  } finally {
    await reader.close();
  }
}

async function main(): Promise<void> {
  if (typeof global.gc !== 'function') {
    console.error('run with --expose-gc: the allocation pass needs a forced collection');
    process.exit(1);
  }

  // The file the runtime loads, by path rather than by specifier: the
  // package's `./embedpdf.wasm` export is not a CJS-resolvable module, and what
  // matters is hashing the exact bytes that were measured.
  const wasmPath = resolve(repoRoot, 'packages/engine/runtime/npm/wasm32/lib/embedpdf.wasm');
  const chain = toolchain(wasmPath);

  const compared = await gate();
  console.log(`answer gate: ${ARMS.join(' and ')} agree on ${compared} fixture page(s)\n`);

  const samples: RoundSample[] = [];
  for (let round = 1; round <= ROUNDS; round++) {
    const order = rotate(ARMS, round);
    console.log(`── round ${round}/${ROUNDS} (order: ${order.join(', ')}) ──`);
    for (const arm of order) {
      for (const fixture of FIXTURES) {
        const sample = await timeOne(arm, fixture, round);
        samples.push(sample);
        console.log(
          `   ${arm.padEnd(10)} ${fixture.label.padEnd(16)} ` +
            `median ${distribution(sample.perRead).median.toFixed(3)}ms`,
        );
      }
    }
  }

  const allocation: Record<string, Record<string, number>> = {};
  for (const arm of ARMS) {
    allocation[arm] = {};
    for (const fixture of FIXTURES) {
      allocation[arm][fixture.label] = await measureAllocation(arm, fixture);
    }
  }

  const summary: Record<string, Record<string, Distribution & { bytesPerRead: number }>> = {};
  for (const arm of ARMS) {
    summary[arm] = {};
    for (const fixture of FIXTURES) {
      const perRead = samples
        .filter((s) => s.arm === arm && s.fixture === fixture.label)
        .flatMap((s) => s.perRead);
      summary[arm][fixture.label] = {
        ...distribution(perRead),
        bytesPerRead: allocation[arm][fixture.label],
      };
    }
  }

  mkdirSync(resultsDir, { recursive: true });
  const record = {
    recordedAt: new Date().toISOString(),
    toolchain: chain,
    protocol: { rounds: ROUNDS, iterations: ITERATIONS, warmup: WARMUP, arms: ARMS },
    // Every raw sample, not just the summary: Section 9 asks for raw
    // measurements to be retained, and a summary nobody can recompute is a
    // claim rather than a measurement.
    samples,
    summary,
  };
  writeFileSync(
    resolve(resultsDir, 'geometry-service-boundary.json'),
    `${JSON.stringify(record, null, 2)}\n`,
  );

  console.log('\nmedian / p95 ms per geometry.read(), and heap bytes retained per snapshot\n');
  for (const fixture of FIXTURES) {
    console.log(`  ${fixture.label}`);
    for (const arm of ARMS) {
      const s = summary[arm][fixture.label];
      console.log(
        `    ${arm.padEnd(10)} ${s.median.toFixed(3)} / ${s.p95.toFixed(3)} ms   ` +
          `${(s.bytesPerRead / 1024).toFixed(0)} KiB`,
      );
    }
  }
  console.log(`\nwritten results/geometry-service-boundary.json (wasm ${chain.wasm.slice(0, 12)})`);
}

await main();
