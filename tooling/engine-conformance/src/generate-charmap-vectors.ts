/**
 * Emits `vectors/text/charmap.json`: what `boundaryTextOffset`,
 * `charBoundaryAtTextOffset`, `charRangeForTextOffsets`, `sliceTextByChars`
 * and `charMapViolation` produce.
 *
 *   pnpm --filter @embedpdf/tooling-engine-conformance run charmap
 *   pnpm --filter @embedpdf/tooling-engine-conformance run charmap:check
 *
 * Part of G5's entry gate. `crates/epdf-engine/tests/charmap.rs` replays it.
 * `core/src/text/charmap.ts` stays in TypeScript for the web selection
 * plugins, so the Rust copy is a deliberate duplicate and this file is what
 * keeps the two the same rule.
 *
 * The probes are exhaustive rather than sampled. Every case is a handful of
 * characters, and a sweep over every boundary and every offset catches an
 * off-by-one that chosen probes step straight over.
 */
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  boundaryTextOffset,
  charBoundaryAtTextOffset,
  charMapViolation,
  charRangeForTextOffsets,
  sliceTextByChars,
} from '../../../packages/engine/core/src/text/charmap';
import type { PageTextSnapshot } from '../../../packages/engine/core/src/dto/PageTextSnapshot';

import {
  CHARMAP_CASES,
  CHARMAP_VIOLATION_CASES,
  type CharMapCase,
} from './charmap-cases';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const vectorsRoot = join(packageRoot, 'vectors', 'text');

/** Bump when the envelope changes shape. The Rust runner asserts it. */
const SCHEMA_VERSION = 1;

function snapshotOf(entry: CharMapCase): PageTextSnapshot {
  return entry.charMap && entry.charMap.length > 0
    ? { text: entry.text, charCount: entry.charCount, charMap: entry.charMap }
    : { text: entry.text, charCount: entry.charCount };
}

function runCase(entry: CharMapCase) {
  const snapshot = snapshotOf(entry);
  const boundaries = Array.from({ length: entry.charCount + 1 }, (_, b) => b);
  const offsets = Array.from({ length: entry.text.length + 1 }, (_, o) => o);

  return {
    ...entry,
    textLength: entry.text.length,
    /** Every character boundary's text offset. */
    boundaryTextOffsets: boundaries.map((b) => boundaryTextOffset(snapshot, b)),
    /** Every text offset's boundary, under each bias. */
    startBiasBoundaries: offsets.map((o) => charBoundaryAtTextOffset(snapshot, o, 'start')),
    endBiasBoundaries: offsets.map((o) => charBoundaryAtTextOffset(snapshot, o, 'end')),
    /** Every half-open text range's character range. */
    charRanges: offsets.flatMap((start) =>
      offsets.map((end) => ({ start, end, range: charRangeForTextOffsets(snapshot, start, end) })),
    ),
    /** Every half-open character range's text. */
    slices: boundaries.flatMap((start) =>
      boundaries.map((end) => ({ start, end, text: sliceTextByChars(snapshot, start, end) })),
    ),
    /** The map validates, which every case in this list must. */
    violation: charMapViolation(entry.charCount, entry.text.length, entry.charMap),
  };
}

function serialize(payload: unknown): string {
  return `${JSON.stringify(payload, null, 2)}\n`;
}

async function emit(relative: string, text: string, check: boolean): Promise<boolean> {
  const absolute = join(vectorsRoot, relative);
  let current: string | undefined;
  try {
    current = await readFile(absolute, 'utf8');
  } catch {
    current = undefined;
  }
  if (current === text) {
    process.stdout.write(`  ok      text/${relative}\n`);
    return false;
  }
  if (check) {
    process.stdout.write(`  DIFFERS text/${relative}${current === undefined ? ' (missing)' : ''}\n`);
    return true;
  }
  await mkdir(dirname(absolute), { recursive: true });
  await writeFile(absolute, text);
  process.stdout.write(`  written text/${relative}\n`);
  return true;
}

async function main(): Promise<void> {
  const check = process.argv.includes('--check');

  const names = new Set<string>();
  for (const entry of CHARMAP_CASES) {
    if (names.has(entry.name)) throw new Error(`duplicate case name ${entry.name}`);
    names.add(entry.name);
  }

  const payload = {
    schemaVersion: SCHEMA_VERSION,
    kind: 'text.charmap',
    cases: CHARMAP_CASES.map(runCase),
    violations: CHARMAP_VIOLATION_CASES.map((entry) => ({
      ...entry,
      violation: charMapViolation(entry.charCount, entry.textLength, entry.charMap),
    })),
  };

  // Every case in the main list must be a valid map, or the probes below it
  // are describing behaviour on input the engine would refuse.
  for (const entry of payload.cases) {
    if (entry.violation !== null) {
      throw new Error(`case ${entry.name} has an invalid map: ${entry.violation}`);
    }
  }

  const differed = await emit('charmap.json', serialize(payload), check);
  if (check && differed) {
    process.stderr.write(
      '\nCharmap vectors differ. Regenerate with:\n' +
        '  pnpm --filter @embedpdf/tooling-engine-conformance run charmap\n' +
        'and commit the diff.\n',
    );
    process.exitCode = 1;
  }
}

main().catch((error: unknown) => {
  process.stderr.write(
    `generate-charmap-vectors: ${error instanceof Error ? (error.stack ?? error.message) : String(error)}\n`,
  );
  process.exitCode = 1;
});
