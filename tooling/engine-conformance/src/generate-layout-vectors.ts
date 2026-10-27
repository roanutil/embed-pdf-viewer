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
import type { PageQuad } from '../../../packages/engine/core/src/geometry/pageSpace';
import type { PdfPoint } from '../../../packages/engine/core/src/geometry/primitives';
import { createTextLayout, mirroredRun } from '../../../packages/engine/core/src/pageSpace/text';
import type { TextRange } from '../../../packages/engine/core/src/text/TextRange';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const vectorsRoot = join(packageRoot, 'vectors', 'text');
const geometryRoot = join(packageRoot, 'vectors', 'pages.geometry');

/** Bump when the envelope changes shape. The Rust runner asserts it. */
const SCHEMA_VERSION = 2;

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
 * and search") and 00d2c8760 ("Quads: one shape"), and moved text geometry
 * to page space in 3bad82c8d ("Page space, text and search"). The committed
 * geometry vectors are what `page.text.layout()` answers, page space with
 * y down, so they go to `createTextLayout` (`core/src/pageSpace/text.ts`),
 * the page-space layout that call returns. It runs `createPdfTextLayout` on
 * the runs mirrored top to bottom and flips each answer back. Quads keep
 * their corner names and segment rects are page boxes (`x, y, width,
 * height`), so the vectors are page space too.
 */

/**
 * How many orientation frames the layout builds. `TextLayout` does not
 * expose its frames, and the vectors carry the count, so this repeats the
 * frame keying of the internal `frameForRun` in `core/src/text/layout.ts`:
 * frame 0 for upright text, then one per (baseline direction, ascent
 * handedness) of each rotated run's first non-empty glyph. The layout keys
 * frames on the y-up runs, so this reads them through upstream's
 * `mirroredRun`, the same flip `createTextLayout` applies.
 */
const FRAME_DOT_TOLERANCE = Math.cos(0.0087);
const EDGE_EPSILON = 1e-6;

function frameCountOf(snapshot: PageGeometrySnapshot): number {
  const frames: { baseline: PdfPoint; ascent: PdfPoint }[] = [
    { baseline: { x: 1, y: 0 }, ascent: { x: 0, y: 1 } },
  ];
  for (const run of snapshot.runs.map(mirroredRun)) {
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

const isFiniteNumber = (value: unknown): value is number =>
  typeof value === 'number' && Number.isFinite(value);

const isPoint = (value: unknown): boolean =>
  !!value &&
  typeof value === 'object' &&
  isFiniteNumber((value as { x?: unknown }).x) &&
  isFiniteNumber((value as { y?: unknown }).y);

const isQuad = (value: PageQuad): boolean =>
  isPoint(value.upperLeft) &&
  isPoint(value.upperRight) &&
  isPoint(value.lowerLeft) &&
  isPoint(value.lowerRight);

function runPage(raw: { runs: unknown[] }, where: string) {
  const snapshot = raw as PageGeometrySnapshot;
  const layout = createTextLayout(snapshot);
  const glyphCount = layout.charCount;
  const probes = probeIndices(glyphCount);
  const wordOf = (glyph: number) => inclusive(layout.wordAt(glyph), glyph);
  const lineOf = (glyph: number) => inclusive(layout.lineAt(glyph), glyph);

  const quads = probes.map((glyph) => layout.charQuad(glyph));
  // A layout fed the wrong space reads `undefined` corners: every quad comes
  // out without numbers and every probe at NaN, and the vector still writes.
  // Refuse that instead of recording it.
  const broken = quads.filter((quad) => quad !== null && !isQuad(quad));
  if (broken.length > 0) {
    throw new Error(`${where}: ${broken.length} glyph quads have no finite corners`);
  }

  const result = {
    glyphCount,
    frameCount: frameCountOf(snapshot),
    // The layout builds one run per snapshot run.
    runCount: snapshot.runs.length,
    /**
     * Per-glyph: its quad, and the word and line it belongs to.
     *
     * `wordAt`/`lineAt` ranges are recorded as INCLUSIVE `[from, to]` pairs,
     * unlike everything else here, which is half-open. The counts below
     * convert.
     */
    glyphs: probes.map((glyph, i) => ({
      glyph,
      quad: quantizeDeep(quads[i]),
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
      return [
        { start: glyph, count: 1 },
        { start: wordStart, count: wordEnd - wordStart + 1 },
        { start: lineStart, count: lineEnd - lineStart + 1 },
      ].map((range) => ({
        ...range,
        segments: quantizeDeep(layout.segments(range)),
      }));
    }),
    /**
     * Hit tests at each probe glyph's own centre, which must return that
     * glyph, plus a point well outside the page, which must return nothing.
     */
    hits: [
      ...quads.flatMap((quad) => {
        if (!quad) return [];
        const cx = q((quad.upperLeft.x + quad.lowerRight.x) / 2);
        const cy = q((quad.upperLeft.y + quad.lowerRight.y) / 2);
        // The centre, where the tight and loose boxes both contain the point,
        // plus two near the loose box's corners, where they do not. Hit
        // testing prefers the TIGHT box, and mutation testing found that
        // untested when every probe sat in the middle of a glyph. Page space
        // runs y down, so the low corner is the largest y of the lower edge.
        const lx = q(Math.min(quad.upperLeft.x, quad.lowerLeft.x));
        const ly = q(Math.max(quad.lowerLeft.y, quad.lowerRight.y));
        const hx = q(Math.max(quad.upperRight.x, quad.lowerRight.x));
        const hy = q(Math.min(quad.upperLeft.y, quad.upperRight.y));
        return [
          { x: cx, y: cy },
          { x: lx, y: ly },
          { x: hx, y: hy },
        ].map((p) => ({ ...p, glyph: layout.charAt(p) }));
      }),
      { x: -10_000, y: -10_000, glyph: layout.charAt({ x: -10_000, y: -10_000 }) },
    ],
  };

  const probeHits = result.hits.slice(0, -1);
  if (probeHits.some((hit) => !isFiniteNumber(hit.x) || !isFiniteNumber(hit.y))) {
    throw new Error(`${where}: a hit probe has no finite position`);
  }
  if (probeHits.length > 0 && probeHits.every((hit) => hit.glyph === null)) {
    throw new Error(`${where}: no hit probe landed on a glyph`);
  }
  return result;
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
        layout: runPage(page.snapshot, `${parsed.case} page ${page.index}`),
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
