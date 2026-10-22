import { readFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterEach, describe, expect, test, vi } from 'vitest';
import type { TextLayout } from '@embedpdf/engine-core/runtime';
import { createPdfRuntime } from '@embedpdf/engine-runtime';
import { RustPageGeometryReader } from '@embedpdf/engine-services';
import { LocalEngine } from '../src/index';
import { InlineTransport } from '../src/transport/InlineTransport';

/**
 * The Phase 0 exit gate for G2: `pages.geometry` at differential parity with
 * `PageGeometryReader`, measured through the service boundary.
 *
 * Not the same test as `crates/epdf-engine/tests/geometry_corpus.rs`, which
 * compares the Rust reader against frozen vectors, or
 * `crates/epdf-wasm/tests/borrowed_page.rs`, which compares the C ABI against
 * the same. This one runs both readers against a live session through
 * `doc.page().geometry.read()`, so everything between the glyph loop and the
 * caller is in scope: the borrow, the envelope, the worker wire, the decode.
 *
 * Upstream removed `page.geometry.read()`. `page.text.layout()` is now the
 * lowest public call that sends `pages.geometry`, so both readers are compared
 * through it: the runs it carries are the snapshot the reader returned, moved
 * into page space.
 *
 * Both engines open their own document from identical bytes, which is the
 * differential-replay rule from `multiplatform-plan.md` Section 9: never two
 * backends on one live document.
 *
 * Skipped unless the runtime was built with the Rust core linked in, which no
 * shipping payload is yet. Build one with the recipe in
 * `tooling/engine-conformance/README.md` and point `EMBEDPDF_WASM_URL` at it.
 */
const here = dirname(fileURLToPath(import.meta.url));
const fixtures = [
  {
    name: 'report',
    path: resolve(here, '../../../../pdfium-multiplatform-prototype/fixtures/report.pdf'),
  },
  {
    name: 'embedded_orientations',
    path: resolve(
      here,
      '../../../../tooling/engine-conformance/fixtures/embedded_orientations.pdf',
    ),
  },
];

async function makeEngine(rustReads: boolean) {
  const runtime = await createPdfRuntime({ prefer: 'wasm' });
  if (rustReads && !runtime.rust) {
    await runtime.destroy();
    return undefined;
  }
  return {
    engine: LocalEngine.fromTransport({ transport: new InlineTransport(runtime, { rustReads }) }),
    runtime,
  };
}

/** What a layout is built from: everything else it answers is computed from these. */
function geometryOf(layout: TextLayout): Pick<TextLayout, 'charCount' | 'runs'> {
  return { charCount: layout.charCount, runs: layout.runs };
}

async function readAllPages(
  rustReads: boolean,
  path: string,
): Promise<Pick<TextLayout, 'charCount' | 'runs'>[]> {
  const made = await makeEngine(rustReads);
  if (!made) throw new Error('no Rust core');
  const bytes = new Uint8Array(await readFile(path));
  const doc = await made.engine.open({ kind: 'bytes', id: `parity-${rustReads}`, bytes });
  try {
    const { pages } = await doc.pages.list();
    const layouts: Pick<TextLayout, 'charCount' | 'runs'>[] = [];
    for (const page of pages) {
      layouts.push(geometryOf(await doc.page(page.ref).text.layout()));
    }
    return layouts;
  } finally {
    await doc.close();
    await made.engine.destroy();
  }
}

const hasRustCore = await (async () => {
  const runtime = await createPdfRuntime({ prefer: 'wasm' });
  const present = runtime.rust !== undefined;
  await runtime.destroy();
  return present;
})();

describe.skipIf(!hasRustCore)('pages.geometry: Rust and TypeScript readers agree', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  for (const fixture of fixtures) {
    test(`${fixture.name}, every page, through the service boundary`, async () => {
      // Counted, so a routing change that quietly sends both engines through
      // the TypeScript reader fails here instead of comparing it with itself.
      const rustReads = vi.spyOn(RustPageGeometryReader.prototype, 'read');
      const [typescript, rust] = await Promise.all([
        readAllPages(false, fixture.path),
        readAllPages(true, fixture.path),
      ]);

      expect(rustReads).toHaveBeenCalledTimes(rust.length);
      expect(typescript.some((page) => page.runs.length > 0)).toBe(true);
      expect(rust.length).toBe(typescript.length);
      for (let i = 0; i < typescript.length; i++) {
        // Exact, not tolerant: both readers are reading the same PDFium in the
        // same module, so the only way a float differs is a real difference in
        // what they computed. The quantized comparison in the corpus exists
        // because WASM and native builds differ, which does not apply here.
        expect(rust[i], `page ${i}`).toEqual(typescript[i]);
      }
    });
  }

  test('a page the document does not have fails the same way on both readers', async () => {
    const made = await makeEngine(true);
    expect(made).toBeDefined();
    const bytes = new Uint8Array(await readFile(fixtures[0].path));
    const doc = await made!.engine.open({ kind: 'bytes', id: 'parity-missing', bytes });
    try {
      // Object number 999999 is not in any of these documents. The Rust path
      // must surface the same EngineErrorCode the TypeScript path does, since
      // the plan makes one error vocabulary a requirement rather than a goal.
      await expect(doc.page(999999 as never).text.layout()).rejects.toMatchObject({
        code: 'NotFound',
      });
    } finally {
      await doc.close();
      await made!.engine.destroy();
    }
  });
});
