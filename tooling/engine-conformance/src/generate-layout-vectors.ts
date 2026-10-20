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

import type {
  PageGeometryRun,
  PageGeometrySnapshot,
} from '../../../packages/engine/core/src/dto/PageGeometrySnapshot';
import type { PdfPoint, PdfQuad, PdfRect } from '../../../packages/engine/core/src/geometry/primitives';
import type { PdfCoordinates } from '../../../packages/engine/core/src/pageSpace/coordinates';
import { createPdfTextLayout } from '../../../packages/engine/core/src/text/layout';
import type { TextRange } from '../../../packages/engine/core/src/text/TextRange';

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

/*
 * Upstream reshaped the layout in a10ed2659 ("Engine API v3, round 3b: text
 * and search") and 00d2c8760 ("Quads: one shape"). The free functions
 * (`buildPageTextLayout`, `textGlyphAt`, `expandTextRangeTo*`,
 * `textSegmentsForRange`, `textGlyphQuad`) are internal now; the way in is
 * `createPdfTextLayout(snapshot)`, a `TextLayout` in the file's coordinates
 * whose methods are those same functions. The geometry DTO renamed its
 * fields (`charStart` to `start`, `looseBox`/`looseQuad` to `loose`,
 * `tightBox`/`tightQuad` to `tight`, flag bits to `space`/`empty`) and quads
 * name their corners. The adapters below translate at the edges, so the
 * input vectors and this file's output keep their shape.
 */

type LegacyQuad = { p1: PdfPoint; p2: PdfPoint; p3: PdfPoint; p4: PdfPoint };

/** The committed geometry vectors' bit flags (old `PageGeometryGlyph.flags`). */
const LEGACY_FLAG_SPACE = 1;
const LEGACY_FLAG_EMPTY = 2;

/**
 * `p1..p4` were upper-start, upper-end, lower-start, lower-end in the
 * glyph's own frame (old `PageGeometrySnapshot.ts`), which is exactly
 * `upperLeft, upperRight, lowerLeft, lowerRight` now.
 */
function namedQuad(q: LegacyQuad): PdfQuad {
  return { upperLeft: q.p1, upperRight: q.p2, lowerLeft: q.p3, lowerRight: q.p4 };
}

function legacyQuad(q: PdfQuad): LegacyQuad {
  return { p1: q.upperLeft, p2: q.upperRight, p3: q.lowerLeft, p4: q.lowerRight };
}

function glyphFlags(flags: number): { space?: true; empty?: true } {
  return {
    ...(flags & LEGACY_FLAG_SPACE ? { space: true as const } : {}),
    ...(flags & LEGACY_FLAG_EMPTY ? { empty: true as const } : {}),
  };
}

/**
 * A committed geometry snapshot in the current DTO shape. A run that already
 * has `start` is the new shape and passes through, so this keeps working once
 * the geometry vectors are regenerated.
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
function currentSnapshot(raw: { runs: any[] }): PageGeometrySnapshot<PdfCoordinates> {
  const runs = raw.runs.map((run): PageGeometryRun<PdfCoordinates> => {
    if ('start' in run) return run;
    const base = { rect: run.rect as PdfRect, start: run.charStart as number };
    const fontSize = run.fontSize === undefined ? {} : { fontSize: run.fontSize as number };
    if ('rotation' in run) {
      return {
        ...base,
        rotation: run.rotation,
        ascentFlip: run.ascentFlip,
        ...fontSize,
        glyphs: run.glyphs.map(
          (g: { looseQuad: LegacyQuad; tightQuad?: LegacyQuad; flags: number }) => ({
            loose: namedQuad(g.looseQuad),
            ...(g.tightQuad ? { tight: namedQuad(g.tightQuad) } : {}),
            ...glyphFlags(g.flags),
          }),
        ),
      };
    }
    return {
      ...base,
      ...fontSize,
      glyphs: run.glyphs.map((g: { looseBox: PdfRect; tightBox?: PdfRect; flags: number }) => ({
        loose: g.looseBox,
        ...(g.tightBox ? { tight: g.tightBox } : {}),
        ...glyphFlags(g.flags),
      })),
    };
  });
  return { runs };
}

/**
 * How many orientation frames the layout builds. `TextLayout` does not
 * expose its frames, and the vectors carry the count, so this repeats the
 * frame keying of the internal `frameForRun` in `core/src/text/layout.ts`:
 * frame 0 for upright text, then one per (baseline direction, ascent
 * handedness) of each rotated run's first non-empty glyph.
 */
const FRAME_DOT_TOLERANCE = Math.cos(0.0087);
const EDGE_EPSILON = 1e-6;

function frameCountOf(snapshot: PageGeometrySnapshot<PdfCoordinates>): number {
  const frames: { baseline: PdfPoint; ascent: PdfPoint }[] = [
    { baseline: { x: 1, y: 0 }, ascent: { x: 0, y: 1 } },
  ];
  for (const run of snapshot.runs) {
    if (!('rotation' in run)) continue;
    const seed = run.glyphs.find((g) => !g.empty);
    if (!seed) continue;
    const q = seed.loose;
    const bx = q.upperRight.x - q.upperLeft.x;
    const by = q.upperRight.y - q.upperLeft.y;
    const len = Math.hypot(bx, by);
    if (len <= EDGE_EPSILON) continue;
    const baseline = { x: bx / len, y: by / len };
    const ax = q.upperLeft.x - q.lowerLeft.x;
    const ay = q.upperLeft.y - q.lowerLeft.y;
    const sign = baseline.x * ay - baseline.y * ax >= 0 ? 1 : -1;
    const ascent = { x: -baseline.y * sign, y: baseline.x * sign };
    const known = frames.some(
      (f) =>
        f.baseline.x * baseline.x + f.baseline.y * baseline.y >= FRAME_DOT_TOLERANCE &&
        f.ascent.x * ascent.x + f.ascent.y * ascent.y > 0,
    );
    if (!known) frames.push({ baseline, ascent });
  }
  return frames.length;
}

/** `wordAt`/`lineAt` give a `TextRange`; the vectors keep inclusive `[from, to]`. */
function inclusive(range: TextRange | null, glyph: number): [number, number] {
  return range === null ? [glyph, glyph] : [range.start, range.start + range.count - 1];
}

function runPage(raw: { runs: unknown[] }) {
  const snapshot = currentSnapshot(raw as { runs: never[] });
  const layout = createPdfTextLayout(snapshot);
  const glyphCount = layout.charCount;
  const probes = probeIndices(glyphCount);
  const glyphQuad = (glyph: number): LegacyQuad | null => {
    const quad = layout.charQuad(glyph);
    return quad ? legacyQuad(quad) : null;
  };
  const wordOf = (glyph: number) => inclusive(layout.wordAt(glyph), glyph);
  const lineOf = (glyph: number) => inclusive(layout.lineAt(glyph), glyph);

  return {
    glyphCount,
    frameCount: frameCountOf(snapshot),
    // The layout builds one run per snapshot run.
    runCount: snapshot.runs.length,
    /**
     * Per-glyph: its quad, and the word and line it belongs to.
     *
     * `expandTextRangeTo*` return INCLUSIVE `[from, to]` pairs, unlike
     * everything else here, which is half-open. The counts below convert.
     */
    glyphs: probes.map((glyph) => ({
      glyph,
      quad: quantizeDeep(glyphQuad(glyph)),
      word: wordOf(glyph),
      line: lineOf(glyph),
    })),
    /**
     * Segments for ranges built from the probes: each glyph alone, each
     * glyph's word, and each glyph's line. Single glyphs catch the
     * degenerate cases; words and lines catch the merge.
     */
    segments: probes.flatMap((glyph) => {
      const [wordStart, wordEnd] = wordOf(glyph);
      const [lineStart, lineEnd] = lineOf(glyph);
      // `+ 1` because the expansions are inclusive and
      // `textSegmentsForRange` takes a count.
      return [
        { start: glyph, count: 1 },
        { start: wordStart, count: wordEnd - wordStart + 1 },
        { start: lineStart, count: lineEnd - lineStart + 1 },
      ].map((range) => ({
        ...range,
        segments: quantizeDeep(
          layout
            .segments(range)
            .map((segment) => ({ ...segment, quad: legacyQuad(segment.quad) })),
        ),
      }));
    }),
    /**
     * Hit tests at each probe glyph's own centre, which must return that
     * glyph, plus a point well outside the page, which must return nothing.
     */
    hits: [
      ...probes.flatMap((glyph) => {
        const quad = glyphQuad(glyph);
        if (!quad) return [];
        const cx = q((quad.p1.x + quad.p4.x) / 2);
        const cy = q((quad.p1.y + quad.p4.y) / 2);
        // The centre, where the tight and loose boxes both contain the point,
        // plus two near the loose box's corners, where they do not. Hit
        // testing prefers the TIGHT box, and mutation testing found that
        // untested when every probe sat in the middle of a glyph.
        const lx = q(Math.min(quad.p1.x, quad.p3.x));
        const ly = q(Math.min(quad.p3.y, quad.p4.y));
        const hx = q(Math.max(quad.p2.x, quad.p4.x));
        const hy = q(Math.max(quad.p1.y, quad.p2.y));
        return [
          { x: cx, y: cy },
          { x: lx, y: ly },
          { x: hx, y: hy },
        ].map((p) => ({ ...p, glyph: layout.charAt(p) }));
      }),
      { x: -10_000, y: -10_000, glyph: layout.charAt({ x: -10_000, y: -10_000 }) },
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
      pages: parsed.pages.map((page: { index: number; snapshot: { runs: unknown[] } }) => ({
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
