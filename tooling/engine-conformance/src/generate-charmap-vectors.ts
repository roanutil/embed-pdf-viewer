/**
 * Emits `vectors/text/charmap.json`: what `boundaryTextOffset`,
 * `charBoundaryAtTextOffset`, `charRangeForTextOffsets`, `sliceText`
 * `charMapViolation`, and `toPageText` produce.
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
  sliceText,
  toPageText,
} from '../../../packages/engine/core/src/text/charmap';
import type { PageTextSnapshot } from '../../../packages/engine/core/src/dto/PageTextSnapshot';

import { CHARMAP_CASES, CHARMAP_VIOLATION_CASES, type CharMapCase } from './charmap-cases';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const vectorsRoot = join(packageRoot, 'vectors', 'text');

/** Bump when the envelope changes shape. The Rust runner asserts it. */
const SCHEMA_VERSION = 2;

function snapshotOf(entry: CharMapCase): PageTextSnapshot {
  return entry.charMap && entry.charMap.length > 0
    ? { text: entry.text, charCount: entry.charCount, charMap: entry.charMap }
    : { text: entry.text, charCount: entry.charCount };
}

/**
 * Upstream's charmap helpers take and return `TextRange { start, count }`
 * (a10ed2659), and `sliceTextByChars` became `sliceText`. The vectors keep
 * the half-open `{ start, end }` shape, so these two adapt at the edge. A
 * negative count behaves as the old `end <= start` did: an empty result.
 */
function charRangeForHalfOpen(snapshot: PageTextSnapshot, start: number, end: number) {
  const range = charRangeForTextOffsets(snapshot, { start, count: end - start });
  return { start: range.start, end: range.start + range.count };
}

function sliceHalfOpen(snapshot: PageTextSnapshot, start: number, end: number): string {
  return sliceText(snapshot, { start, count: end - start });
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
      offsets.map((end) => ({ start, end, range: charRangeForHalfOpen(snapshot, start, end) })),
    ),
    /** Every half-open character range's text. */
    slices: boundaries.flatMap((start) =>
      boundaries.map((end) => ({ start, end, text: sliceHalfOpen(snapshot, start, end) })),
    ),
    /** The map validates, which every case in this list must. */
    violation: charMapViolation(entry.charCount, entry.text.length, entry.charMap),
  };
}

/** Raw extraction cases exercise the cleanup before selection and search consume it. */
const PAGE_TEXT_CASES: readonly CharMapCase[] = [
  {
    name: 'unchanged-text',
    covers: 'Ordinary text retains the identity map.',
    text: 'café',
    charCount: 4,
  },
  {
    name: 'crlf',
    covers: 'CR in CRLF loses its text but keeps its character slot.',
    text: 'A\r\nB',
    charCount: 4,
  },
  {
    name: 'lone-cr',
    covers: 'A lone CR becomes LF without changing index spaces.',
    text: 'A\rB',
    charCount: 3,
  },
  {
    name: 'joined-hyphen-marker',
    covers: 'A PDFium line-end hyphen marker contributes no text.',
    text: 'co\uFFFEoperate',
    charCount: 10,
  },
  {
    name: 'bmp-noncharacters',
    covers: 'The reserved BMP range and plane-ending markers are removed.',
    text: '\uFDD0A\uFDEF\uFFFE\uFFFF',
    charCount: 5,
  },
  {
    name: 'astral-noncharacters',
    covers: 'Plane-ending noncharacters beyond the BMP lose both UTF-16 units.',
    text: 'A\u{1FFFE}B\u{10FFFF}C',
    charCount: 5,
    charMap: [
      [2, 3],
      [4, 6],
    ],
  },
  {
    name: 'existing-drops-with-crlf',
    covers: 'Existing zero-width slots survive cleanup beside a CRLF.',
    text: 'A\r\nB',
    charCount: 6,
    charMap: [
      [1, 0],
      [4, 2],
    ],
  },
  {
    name: 'astral-crlf-and-marker',
    covers: 'Supplementary text keeps its two-unit anchor while CR and markers drop.',
    text: '\u{1F600}\r\n\uFFFEZ',
    charCount: 5,
    charMap: [[1, 2]],
  },
  {
    name: 'bmp-range-boundaries',
    covers: 'Reserved range boundaries drop, while their neighboring scalar values stay.',
    text: '\uFDCF\uFDD0\uFDEF\uFDF0',
    charCount: 4,
  },
  {
    name: 'astral-range-boundaries',
    covers: 'A plane-ending marker drops without losing adjacent supplementary text.',
    text: '\u{1FFFD}\u{1FFFE}\u{1FFFF}\u{20000}',
    charCount: 4,
    charMap: [
      [1, 2],
      [2, 4],
      [3, 6],
      [4, 8],
    ],
  },
  {
    name: 'all-noncharacters',
    covers: 'Cleaning a page to empty text retains every character slot.',
    text: '\uFFFE\u{10FFFF}',
    charCount: 2,
    charMap: [[2, 3]],
  },
  {
    name: 'cr-before-astral',
    covers: 'A CR before a surrogate pair becomes LF and preserves the pair.',
    text: '\r\u{1F600}',
    charCount: 2,
    charMap: [[2, 3]],
  },
];

function runPageTextCase(entry: CharMapCase) {
  const extracted = snapshotOf(entry);
  const violation = charMapViolation(extracted.charCount, extracted.text.length, extracted.charMap);
  if (violation !== null) throw new Error(`raw case ${entry.name}: ${violation}`);
  const cleaned = toPageText(extracted);
  return {
    name: entry.name,
    covers: entry.covers,
    extracted,
    cleaned: runCase({ ...entry, ...cleaned }),
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
    process.stdout.write(
      `  DIFFERS text/${relative}${current === undefined ? ' (missing)' : ''}\n`,
    );
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
    pageTextCases: PAGE_TEXT_CASES.map(runPageTextCase),
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
