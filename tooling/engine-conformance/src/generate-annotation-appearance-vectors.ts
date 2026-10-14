/**
 * Emits `vectors/annotation/appearances.json`: what
 * `annotations.renderAppearances` places where, per annotation, per mode.
 *
 *   pnpm --filter @embedpdf/tooling-engine-conformance run annotation-appearance
 *   pnpm --filter @embedpdf/tooling-engine-conformance run annotation-appearance:check
 *
 * G7's appearance gate. `crates/epdf-engine/tests/annotation_appearance.rs`
 * replays it.
 *
 * # Placement, not pixels
 *
 * The recorded facts are the ref, the mode, the placement rect and the raster
 * size. The pixels are deliberately not recorded, and the reason is the same
 * one `corpus.ts` gives for embedding fonts: the TypeScript reads through the
 * WASM build and Rust through the native dylib, so identical geometry can
 * still produce rasters that differ in the last bit of a filtered edge. A
 * digest would be a cross-language comparison of two rasterizers rather than
 * of two readers.
 *
 * What is recorded is where the rule actually lives. Whether an appearance is
 * emitted at all, which mode it came from, and whether its rect is the
 * annotation's `/Rect` or its logical unrotated box: that last one is the
 * rotation-stripping decision, and getting it wrong puts a picture in the
 * wrong place on the page.
 */
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { createLocalEngine } from '../../../packages/engine/main/src/index';
import { flattenPageRefs } from './wire-page-refs';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = resolve(packageRoot, '../..');
const vectorsRoot = join(packageRoot, 'vectors', 'annotation');

/** Bump when the envelope changes shape. The Rust runner asserts it. */
const SCHEMA_VERSION = 1;

interface AppearanceCase {
  readonly name: string;
  readonly fixture: string;
  readonly covers: string;
  readonly scale?: number;
  readonly modes?: ('normal' | 'rollover' | 'down')[];
}

const CASES: readonly AppearanceCase[] = [
  {
    name: 'normal',
    fixture: 'tooling/engine-conformance/fixtures/annotation_appearances.pdf',
    covers:
      'The default request: normal mode at scale 1. A stripped rotated square, a callout and a polygon that are the near misses, a highlight with no /AP that must be skipped, and a widget with no /AP key that is the one case an appearance is generated.',
  },
  {
    name: 'every-mode',
    fixture: 'tooling/engine-conformance/fixtures/annotation_appearances.pdf',
    covers:
      'All three modes requested. Only the square carries /R and /D, so this is what separates emitting the modes an annotation has from emitting the modes that were asked for.',
    modes: ['normal', 'rollover', 'down'],
  },
  {
    name: 'scaled',
    fixture: 'tooling/engine-conformance/fixtures/annotation_appearances.pdf',
    covers: 'Scale 3, so the raster sizes are a function of the rect rather than a constant.',
    scale: 3,
  },
  {
    name: 'no-appearances',
    fixture: 'tooling/engine-conformance/fixtures/annotation_kinds.pdf',
    covers:
      'A page full of annotations and not one /AP among them. The empty result is a different answer from a failed one.',
  },
];

interface Placed {
  readonly ref: unknown;
  readonly mode: string;
  readonly rect: unknown;
  readonly width: number;
  readonly height: number;
}

async function readCase(entry: AppearanceCase): Promise<unknown> {
  const bytes = new Uint8Array(await readFile(join(repoRoot, entry.fixture)));
  const engine = createLocalEngine({
    runtime: {
      prefer: 'wasm',
      wasm: {
        print: (message: string) => process.stderr.write(`${message}\n`),
        printErr: (message: string) => process.stderr.write(`${message}\n`),
      },
    },
  });
  try {
    const doc = await engine.open(
      { kind: 'bytes', id: `appearance:${entry.name}`, bytes },
      { scope: ['*'] },
    );
    try {
      const pages = await doc.pages.list();
      const first = pages.pages[0];
      if (!first) throw new Error(`${entry.name}: the fixture has no pages`);
      const page = doc.page(first.ref);
      const result = await page.annotations.renderAppearances({
        ...(entry.scale !== undefined ? { scale: entry.scale } : {}),
        ...(entry.modes !== undefined ? { modes: entry.modes } : {}),
      });
      const appearances: Placed[] = result.appearances.map((a) => ({
        ref: flattenPageRefs(a.ref),
        mode: a.mode,
        rect: a.rect,
        width: a.raster.width,
        height: a.raster.height,
      }));
      return { ...entry, pageObjectNumber: first.ref.pageObjectNumber, appearances };
    } finally {
      await doc.close();
    }
  } finally {
    await engine.destroy();
  }
}

/** See `generate-annotation-read-vectors.ts`: a session id is minted per open. */
function normalizeSessionIds(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(normalizeSessionIds);
  if (value === null || typeof value !== 'object') return value;
  return Object.fromEntries(
    Object.entries(value as Record<string, unknown>).map(([key, entry]) =>
      key === 'docSessionId' ? [key, '<session>'] : [key, normalizeSessionIds(entry)],
    ),
  );
}

function serialize(cases: readonly unknown[]): string {
  const head = JSON.stringify(
    { schemaVersion: SCHEMA_VERSION, kind: 'annotation.appearances' },
    null,
    2,
  ).replace(/\n}$/, ',');
  const body = cases
    .map((entry) => `  ${JSON.stringify(normalizeSessionIds(entry))}`)
    .join(',\n');
  return `${head}\n  "cases": [\n${body}\n  ]\n}\n`;
}

async function main(): Promise<void> {
  const check = process.argv.includes('--check');
  const cases: unknown[] = [];
  for (const entry of CASES) cases.push(await readCase(entry));
  const text = serialize(cases);
  const absolute = join(vectorsRoot, 'appearances.json');

  let current: string | undefined;
  try {
    current = await readFile(absolute, 'utf8');
  } catch {
    current = undefined;
  }
  if (current === text) {
    process.stdout.write('  ok      annotation/appearances.json\n');
    return;
  }
  if (check) {
    process.stdout.write(
      `  DIFFERS annotation/appearances.json${current === undefined ? ' (missing)' : ''}\n`,
    );
    process.stderr.write(
      '\nAnnotation appearance vectors differ. Regenerate with:\n' +
        '  pnpm --filter @embedpdf/tooling-engine-conformance run annotation-appearance\n' +
        'and commit the diff.\n',
    );
    process.exitCode = 1;
    return;
  }
  await mkdir(dirname(absolute), { recursive: true });
  await writeFile(absolute, text);
  process.stdout.write('  written annotation/appearances.json\n');
}

main().catch((error: unknown) => {
  process.stderr.write(
    `generate-annotation-appearance-vectors: ${error instanceof Error ? (error.stack ?? error.message) : String(error)}\n`,
  );
  process.exitCode = 1;
});
