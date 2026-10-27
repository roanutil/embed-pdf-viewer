/**
 * Emits `vectors/render/geometry.json`: what `deviceSize`,
 * `pdfRectToDisplayRect` and `renderMatrix` (formerly
 * `displayRectToDeviceMatrix`) produce.
 *
 *   pnpm --filter @embedpdf/tooling-engine-conformance run render
 *   pnpm --filter @embedpdf/tooling-engine-conformance run render:check
 *
 * Part of G4's gate. `crates/epdf-engine/tests/render_geometry.rs` replays it.
 * The pixels themselves are checked against a fixture; this is the arithmetic
 * before them, which is where a divergence is invisible rather than obvious.
 */
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { renderMatrix } from '../../../packages/engine/core/src/geometry/pageTransform';
import { deviceSize } from '../../../packages/engine/services/src/features/render/deviceRaster';

import { RENDER_CASES, type RenderCase } from './render-cases';
import { quantize } from './quantize';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const vectorsRoot = join(packageRoot, 'vectors', 'render');

/** Bump when the envelope changes shape. The Rust runner asserts it. */
const SCHEMA_VERSION = 1;

/**
 * `pdfRectToDisplayRect` is not exported, and exporting it to be tested would
 * be changing the subject's shape to suit the test. It is four lines and
 * repeated here; the vectors carry its output, so a divergence between this
 * copy and the original shows up as a wrong display rect in every case at
 * once rather than silently.
 */
function pdfRectToDisplayRect(rect: RenderCase['rect'], pageHeight: number) {
  return {
    left: rect.left,
    right: rect.right,
    bottom: pageHeight - rect.top,
    top: pageHeight - rect.bottom,
  };
}

/**
 * Upstream replaced `displayRectToDeviceMatrix` with engine-core's
 * `renderMatrix` over a page-space box (771342379). The rasterizer converts
 * the display rect with a private `pageBoxOfDisplayRect`; this is that
 * conversion, repeated for the same reason as `pdfRectToDisplayRect` above.
 */
function pageBoxOfDisplayRect(rect: RenderCase['rect']) {
  return {
    x: rect.left,
    y: rect.bottom,
    width: rect.right - rect.left,
    height: rect.top - rect.bottom,
  };
}

function runCase(entry: RenderCase) {
  const displayRect = pdfRectToDisplayRect(entry.rect, entry.pageHeight);
  const size = deviceSize(displayRect, entry.rotation, entry.viewport);
  const matrix = renderMatrix(
    pageBoxOfDisplayRect(displayRect),
    entry.rotation,
    size.width,
    size.height,
  );
  return {
    ...entry,
    displayRect,
    size,
    // Quantized, because these are ratios of doubles and the two languages
    // need not agree on the last bit. The scale is the corpus-wide one, so a
    // matrix cell is pinned to 1e-4 of a device pixel.
    matrix: matrix.map(quantize),
  };
}

function serialize(entries: readonly unknown[]): string {
  const head = JSON.stringify({ schemaVersion: SCHEMA_VERSION, kind: 'render.geometry' }, null, 2)
    .replace(/\n}$/, ',');
  const body = entries.map((entry) => `  ${JSON.stringify(entry)}`).join(',\n');
  return `${head}\n  "cases": [\n${body}\n  ]\n}\n`;
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
    process.stdout.write(`  ok      render/${relative}\n`);
    return false;
  }
  if (check) {
    process.stdout.write(`  DIFFERS render/${relative}${current === undefined ? ' (missing)' : ''}\n`);
    return true;
  }
  await mkdir(dirname(absolute), { recursive: true });
  await writeFile(absolute, text);
  process.stdout.write(`  written render/${relative}\n`);
  return true;
}

async function main(): Promise<void> {
  const check = process.argv.includes('--check');

  const names = new Set<string>();
  for (const entry of RENDER_CASES) {
    if (names.has(entry.name)) throw new Error(`duplicate case name ${entry.name}`);
    names.add(entry.name);
  }

  const differed = await emit('geometry.json', serialize(RENDER_CASES.map(runCase)), check);
  if (check && differed) {
    process.stderr.write(
      '\nRender vectors differ. Regenerate with:\n' +
        '  pnpm --filter @embedpdf/tooling-engine-conformance run render\n' +
        'and commit the diff.\n',
    );
    process.exitCode = 1;
  }
}

main().catch((error: unknown) => {
  process.stderr.write(
    `generate-render-vectors: ${error instanceof Error ? (error.stack ?? error.message) : String(error)}\n`,
  );
  process.exitCode = 1;
});
