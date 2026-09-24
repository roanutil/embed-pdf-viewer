/**
 * Emits `vectors/text/layout.json`: what `core/src/text/layout.ts` produces
 * for the corpus pages.
 *
 *   pnpm --filter @embedpdf/tooling-engine-conformance run layout
 *   pnpm --filter @embedpdf/tooling-engine-conformance run layout:check
 *
 * `multiplatform-port-order.md` G5 calls layout.ts a "deliberate duplicate
 * with shared vectors": web selection plugins consume it, so it stays in
 * TypeScript and the native copy is pinned to it rather than replacing it.
 * These are those vectors.
 *
 * The file is worth having before the Rust side exists. Layout is where glyph
 * geometry becomes hit targets, word and line ranges, and merged visual
 * segments, and the merge is adapted from Chromium's MergeAdjacentRects. A
 * port written against prose would be a reimplementation; a port written
 * against these is a transcription somebody can check.
 *
 * Input is the committed geometry vectors rather than a live read, so this
 * generator needs no PDFium and cannot drift from what the geometry runner
 * already pins.
 */
import { mkdir, readFile, readdir, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import type { PageGeometrySnapshot } from '../../../packages/engine/core/src/dto/PageGeometrySnapshot';
import {
  buildPageTextLayout,
  expandTextRangeToLine,
  expandTextRangeToWord,
  textGlyphAt,
  textGlyphQuad,
  textSegmentsForRange,
} from '../../../packages/engine/core/src/text/layout';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const vectorsRoot = join(packageRoot, 'vectors', 'text');
const geometryRoot = join(packageRoot, 'vectors', 'pages.geometry');

/** Bump when the envelope changes shape. The Rust runner asserts it. */
const SCHEMA_VERSION = 1;

/**
 * How many glyphs a page contributes probes for.
 *
 * Every glyph on a 5,000-glyph page would make a vector nobody reviews, so
 * the sweep is bounded and spread across the page rather than taken from the
 * front: an off-by-one at a run boundary is what this is looking for, and
 * those are everywhere rather than at the start.
 */
const PROBES_PER_PAGE = 24;

function probeIndices(glyphCount: number): number[] {
  if (glyphCount === 0) return [];
  if (glyphCount <= PROBES_PER_PAGE) {
    return Array.from({ length: glyphCount }, (_, i) => i);
  }
  const step = glyphCount / PROBES_PER_PAGE;
  const out = new Set<number>();
  // The ends always, then an even spread. The ends are where a range
  // expansion runs off the page.
  out.add(0);
  out.add(glyphCount - 1);
  for (let i = 0; i < PROBES_PER_PAGE; i++) {
    out.add(Math.min(glyphCount - 1, Math.floor(i * step)));
  }
  return [...out].sort((a, b) => a - b);
}

/** Round to the corpus quantum, so two languages agree on the last bit. */
const q = (value: number): number => Math.round(value * 10_000) / 10_000;

function quantizeDeep(value: unknown): unknown {
  if (typeof value === 'number') return q(value);
  if (Array.isArray(value)) return value.map(quantizeDeep);
  if (value && typeof value === 'object') {
    return Object.fromEntries(
      Object.entries(value as Record<string, unknown>).map(([k, v]) => [k, quantizeDeep(v)]),
    );
  }
  return value;
}

function runPage(snapshot: PageGeometrySnapshot) {
  const layout = buildPageTextLayout(snapshot);
  const glyphCount = layout.glyphs.length;
  const probes = probeIndices(glyphCount);

  return {
    glyphCount,
    frameCount: layout.frames.length,
    runCount: layout.runs.length,
    /**
     * Per-glyph: its quad, and the word and line it belongs to.
     *
     * `expandTextRangeTo*` return INCLUSIVE `[from, to]` pairs, unlike
     * everything else here, which is half-open. The counts below convert.
     */
    glyphs: probes.map((glyph) => ({
      glyph,
      quad: quantizeDeep(textGlyphQuad(layout, glyph)),
      word: expandTextRangeToWord(layout, glyph),
      line: expandTextRangeToLine(layout, glyph),
    })),
    /**
     * Segments for ranges built from the probes: each glyph alone, each
     * glyph's word, and each glyph's line. Single glyphs catch the
     * degenerate cases; words and lines catch the merge.
     */
    segments: probes.flatMap((glyph) => {
      const [wordStart, wordEnd] = expandTextRangeToWord(layout, glyph);
      const [lineStart, lineEnd] = expandTextRangeToLine(layout, glyph);
      // `+ 1` because the expansions are inclusive and
      // `textSegmentsForRange` takes a count.
      return [
        { start: glyph, count: 1 },
        { start: wordStart, count: wordEnd - wordStart + 1 },
        { start: lineStart, count: lineEnd - lineStart + 1 },
      ].map((range) => ({
        ...range,
        segments: quantizeDeep(textSegmentsForRange(layout, range.start, range.count)),
      }));
    }),
    /**
     * Hit tests at each probe glyph's own centre, which must return that
     * glyph, plus a point well outside the page, which must return nothing.
     */
    hits: [
      ...probes.flatMap((glyph) => {
        const quad = textGlyphQuad(layout, glyph);
        if (!quad) return [];
        const x = q((quad.p1.x + quad.p4.x) / 2);
        const y = q((quad.p1.y + quad.p4.y) / 2);
        return [{ x, y, glyph: textGlyphAt(layout, { x, y }) }];
      }),
      { x: -10_000, y: -10_000, glyph: textGlyphAt(layout, { x: -10_000, y: -10_000 }) },
    ],
  };
}

async function main(): Promise<void> {
  const check = process.argv.includes('--check');

  const files = (await readdir(geometryRoot)).filter((f) => f.endsWith('.json')).sort();
  const cases = [];
  for (const file of files) {
    const parsed = JSON.parse(await readFile(join(geometryRoot, file), 'utf8'));
    cases.push({
      case: parsed.case,
      fixture: parsed.fixture,
      covers: parsed.covers,
      pages: parsed.pages.map((page: { index: number; snapshot: PageGeometrySnapshot }) => ({
        index: page.index,
        layout: runPage(page.snapshot),
      })),
    });
  }

  const payload = { schemaVersion: SCHEMA_VERSION, kind: 'text.layout', cases };
  const text = `${JSON.stringify(payload, null, 2)}\n`;
  const absolute = join(vectorsRoot, 'layout.json');

  let current: string | undefined;
  try {
    current = await readFile(absolute, 'utf8');
  } catch {
    current = undefined;
  }
  if (current === text) {
    process.stdout.write('  ok      text/layout.json\n');
    return;
  }
  if (check) {
    process.stdout.write(`  DIFFERS text/layout.json${current === undefined ? ' (missing)' : ''}\n`);
    process.stderr.write(
      '\nLayout vectors differ. Regenerate with:\n' +
        '  pnpm --filter @embedpdf/tooling-engine-conformance run layout\n' +
        'and commit the diff.\n',
    );
    process.exitCode = 1;
    return;
  }
  await mkdir(dirname(absolute), { recursive: true });
  await writeFile(absolute, text);
  process.stdout.write('  written text/layout.json\n');
}

main().catch((error: unknown) => {
  process.stderr.write(
    `generate-layout-vectors: ${error instanceof Error ? (error.stack ?? error.message) : String(error)}\n`,
  );
  process.exitCode = 1;
});
