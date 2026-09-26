/**
 * `packages/plugin/stamp`: placement. armAsset delegates to the target
 * document's annotation plugin (the real interaction, annotation and form
 * plugins run beside stamp); with scripting enabled, a form-backed PDF asset
 * is first evaluated on a detached copy through the form plugin's standalone
 * scripting controller, over the scripted sandbox, then flattened. Replayed
 * by `crates/plugins/stamp/tests/conformance.rs`.
 */
import type { FakeDocumentSpec, ResponseSpec } from '../fake-engine';
import type { ScenarioCase } from '../scenario';
import type { Suite } from '../vectors';

import { hydrate } from './annotation-fixtures';
import { effectsOk, js, list, ref, text, tree } from './form-fixtures';
import {
  PDF,
  PLACEMENT,
  STAMP,
  assetDoc,
  assetPage,
  bytes,
  destroy,
  engineError,
  openDoc,
  opens,
  pdf,
  png,
  settle,
  stamp,
  start,
  str,
  type StampScriptingSpec,
  type StampSpec,
  withWorkshop,
  WORKSHOP_PAGE,
} from './stamp-fixtures';

const ONE = [assetPage(0, 100)];
const TWO = [assetPage(0, 100), assetPage(1, 101)];

const IDENTITY = { user_id: 'alex', group_id: 'EmbedPDF', display_name: 'Alex Morgan' };

/** The target document the kernel opens: annotation hydration and an empty
 *  form snapshot for the plugins beside stamp. */
const target = (over: Partial<FakeDocumentSpec> = {}): FakeDocumentSpec => ({
  pages: [assetPage(0, 1, 600, 800)],
  security: { allows: '*', identity: IDENTITY },
  responses: [hydrate(), list([])],
  ...over,
});

const ev = { rc: true, value: null, change: '', selStart: 0, selEnd: 0 };
const out = (over: Record<string, unknown> = {}) => ({ output: over });
const setValue = (n: number, value: string) => ({ kind: 'setValue', ref: ref(n), value: { type: 'text', value } });

/** One field with a widget on `pon`, a boot script that sets it, and an
 *  engine that applies the effect. */
const DATE = (pon = 100) => text(1, 'date', '', {}, [[201, pon]]);
const BOOT_SCRIPTS: ResponseSpec = {
  path: 'actions.read',
  value: { nameTreeScripts: [{ name: 'stamp', action: tree(js('this.getField("date").value = util.printd(...);')) }], openAction: null, openDestination: null },
};

interface DynamicOptions {
  readonly pages?: typeof ONE;
  readonly fields?: unknown[];
  readonly responses?: ResponseSpec[];
  readonly lacks?: string[];
  readonly flatten?: unknown;
}

/** The detached copy armAsset evaluates: the library's pages, a form
 *  snapshot, the boot scripts, and answers for the apply, flatten, extract
 *  and render that follow. */
const dynamicDoc = (o: DynamicOptions = {}): FakeDocumentSpec => {
  const pages = o.pages ?? ONE;
  return {
    services: ['actions', 'pieceInfo', 'page.pieceInfo'],
    ...(o.lacks ? { lacks: o.lacks } : {}),
    pages,
    responses: [
      ...(o.responses ?? []),
      { path: 'pages.list', value: { pageCount: pages.length, pages } },
      list(o.fields ?? [DATE()]),
      BOOT_SCRIPTS,
      { path: 'forms.applyEffects', value: effectsOk(1) },
      {
        path: 'pages.flatten',
        value: o.flatten ?? { results: pages.map((p) => ({ page: p.ref, status: 'flattened' })) },
      },
      ...pages.map((p) => ({ path: 'pages.extract', args: [[{ kind: 'objectNumber', pageObjectNumber: p.ref.pageObjectNumber }]], value: pdf(`flat-${p.ref.pageObjectNumber}`) })),
      { path: 'page.render.image', value: { contentType: 'image/png', source: { kind: 'bytes', bytes: bytes('flat-preview') } } },
    ],
  };
};

const BOOT_SETS = { builds: [{ boot: [out({ formEffects: [setValue(1, '2026-07-15')] })] }] };
const scripting = (over: Partial<StampScriptingSpec> = {}): StampSpec => ({
  scripting: { enabled: true, sandbox: BOOT_SETS, ...over },
});

const arm = (assetId: string, opts?: unknown, doc = 'a') => stamp('armAsset', opts === undefined ? [doc, assetId] : [doc, assetId, opts]);

const staticCases: ScenarioCase[] = [
  {
    name: 'arm-delegates-to-the-annotation-plugin',
    covers:
      "armAsset hands the target document's annotation plugin the asset's placement bytes, its preview as a payload, its intrinsic size and the target width; disarm disarms that document's stamp.",
    plugins: PLACEMENT(),
    engine: { documents: { a: target(), lib: assetDoc(ONE) }, responses: opens('a', 'lib') },
    snapshot: ['stamp', 'annotation::a', 'interaction::a'],
    steps: [
      start,
      openDoc('a'),
      stamp('importLibraryPdf', [PDF]),
      arm('stamp-lib-0-1:Stamp1', { targetWidth: 120 }),
      stamp('disarm', ['a']),
      arm('stamp-lib-0-1:Stamp1'),
    ],
  },
  {
    name: 'a-raster-asset-arms-with-its-own-preview',
    covers: 'A loose raster asset arms with its bytes and itself as the preview; a PDF asset with no preview arms without one.',
    plugins: PLACEMENT(),
    engine: { documents: { a: target() }, responses: opens('a') },
    snapshot: ['annotation::a'],
    steps: [
      start,
      openDoc('a'),
      stamp('addAsset', [{ name: 'Logo', source: png(100, 50) }]),
      stamp('addAsset', [{ name: 'Sig', source: PDF, size: { width: 150, height: 60 } }]),
      arm('stamp-lib-0-0:Logo', { targetWidth: 50 }),
      arm('stamp-lib-0-3:Sig', {}),
    ],
  },
  {
    name: 'arming-what-is-not-there',
    covers:
      "An unknown asset rejects with NotFound before anything else; a document with no annotation plugin, or one that is not open, rejects (arm) or throws (disarm) with the kernel's missing-capability error.",
    plugins: STAMP(),
    engine: { documents: { a: target({ responses: [] }) }, responses: opens('a') },
    snapshot: ['stamp'],
    steps: [
      start,
      openDoc('a'),
      arm('nope'),
      stamp('addAsset', [{ name: 'Logo', source: png() }]),
      arm('stamp-lib-0-0:Logo'),
      arm('stamp-lib-0-0:Logo', undefined, 'elsewhere'),
      stamp('disarm', ['a']),
      stamp('disarm', ['elsewhere']),
    ],
  },
];

const skipped: ScenarioCase[] = [
  {
    name: 'scripting-off-arms-the-template',
    covers: 'With scripting disabled, or enabled on a host with no sandbox, a PDF asset arms its template bytes and no copy is opened.',
    plugins: PLACEMENT({ scripting: { enabled: false, sandbox: BOOT_SETS } }),
    engine: { documents: { a: target(), lib: assetDoc(ONE) }, responses: opens('a', 'lib') },
    snapshot: 'none',
    steps: [start, openDoc('a'), stamp('importLibraryPdf', [PDF]), arm('stamp-lib-0-1:Stamp1')],
  },
  {
    name: 'no-sandbox-host',
    covers: 'A host with scripting enabled but no sandbox evaluates nothing: the Rust port has no scripting options to build, and the TypeScript runs as with scripting disabled.',
    plugins: PLACEMENT({ scripting: { enabled: true, sandbox: null } }),
    engine: { documents: { a: target(), lib: assetDoc(ONE) }, responses: opens('a', 'lib') },
    snapshot: 'none',
    steps: [start, openDoc('a'), stamp('importLibraryPdf', [PDF]), arm('stamp-lib-0-1:Stamp1')],
  },
  {
    name: 'a-raster-asset-is-never-evaluated',
    covers: 'With scripting enabled, a raster asset still arms as is, without a target check.',
    plugins: PLACEMENT(scripting()),
    engine: { documents: { a: target() }, responses: opens('a') },
    snapshot: 'none',
    steps: [start, openDoc('a'), stamp('addAsset', [{ name: 'Logo', source: png() }]), arm('stamp-lib-0-0:Logo')],
  },
  {
    name: 'a-stamp-without-fields-on-its-page-arms-the-template',
    covers:
      'A copy whose form has no widget on the selected page is closed again and the template bytes arm; a field on another page does not count.',
    plugins: PLACEMENT(scripting()),
    engine: {
      documents: {
        a: target(),
        lib: assetDoc(TWO),
        copy: dynamicDoc({ pages: TWO, fields: [DATE(101)] }),
      },
      responses: opens('a', 'lib', 'copy'),
    },
    snapshot: 'none',
    steps: [start, openDoc('a'), stamp('importLibraryPdf', [PDF]), arm('stamp-lib-0-1:Stamp1')],
  },
];

const dynamic: ScenarioCase[] = [
  {
    name: 'materializes-a-dynamic-stamp',
    covers:
      "With scripting enabled, a canonical asset's whole library PDF opens as a detached copy under the target's identity; the standalone controller boots and recalculates it against the target document with the configured clock, offset and seed; the page is flattened for display, extracted and rendered without annotations, and that page and preview arm. The library and template bytes stay as they were.",
    plugins: PLACEMENT(
      scripting({
        now: { $fn: 'now', returns: 1784107800000 },
        utcOffsetMinutes: { $fn: 'offset', returns: 180 },
        randomSeed: { $fn: 'seed', returns: 7 },
      }),
    ),
    engine: { documents: { a: target(), lib: assetDoc(ONE), copy: dynamicDoc() }, responses: opens('a', 'lib', 'copy') },
    snapshot: ['stamp', 'annotation::a'],
    steps: [
      start,
      openDoc('a'),
      stamp('importLibraryPdf', [PDF]),
      arm('stamp-lib-0-1:Stamp1', { targetWidth: 80 }),
      stamp('libraryBytes', ['stamp-lib-0-1']),
      stamp('assetBytes', ['stamp-lib-0-1:Stamp1']),
    ],
  },
  {
    name: 'a-loose-dynamic-stamp',
    covers:
      'A PDF asset added without a library gets a library of its own (a blank seed, then the page appended, both on the workshop document) and evaluates from that library\'s canonical bytes, selecting its page. A target without an identity opens the copy without one. (v2 kept such an asset loose and evaluated its own bytes, refusing a two-page PDF; v3 has no loose assets.)',
    plugins: PLACEMENT(scripting()),
    engine: {
      documents: {
        a: target({ security: { allows: '*' } }),
        copy: dynamicDoc({ pages: [assetPage(0, 3, 612, 792), assetPage(1, WORKSHOP_PAGE)], fields: [DATE(WORKSHOP_PAGE)] }),
      },
      responses: opens('a', 'workshop', 'workshop', 'copy'),
    },
    snapshot: 'none',
    steps: [
      start,
      openDoc('a'),
      stamp('addAsset', [{ name: 'Dynamic', source: PDF, size: { width: 200, height: 80 } }]),
      arm('stamp-lib-0-0:Dynamic'),
    ],
  },
  {
    name: 'dynamic-stamp-refusals',
    covers:
      "A target that is not open rejects with NotFound before any copy; a canonical page missing from the copy rejects with InvalidArg naming it; an engine without pages.flatten or pages.extract rejects with NotImplemented. Each copy is closed.",
    plugins: PLACEMENT(scripting()),
    engine: {
      documents: {
        a: target(),
        lib: assetDoc(ONE),
        moved: dynamicDoc({ pages: [assetPage(0, 300)] }),
        noFlatten: dynamicDoc({ lacks: ['pages.flatten'] }),
        noExtract: dynamicDoc({ lacks: ['pages.extract'] }),
      },
      responses: opens('a', 'lib', 'moved', 'noFlatten', 'noExtract'),
    },
    snapshot: 'none',
    steps: [
      start,
      openDoc('a'),
      stamp('importLibraryPdf', [PDF]),
      arm('stamp-lib-0-1:Stamp1', undefined, 'closed'),
      arm('stamp-lib-0-1:Stamp1'),
      arm('stamp-lib-0-1:Stamp1'),
      arm('stamp-lib-0-1:Stamp1'),
    ],
  },
  {
    name: 'a-failed-boot-script-is-a-diagnostic',
    covers:
      "A boot script that throws does not fail the recalculation: its UI effects and diagnostics (the failure among them) reach the observers, onError is not called, and the stamp still materializes.",
    plugins: PLACEMENT(
      scripting({
        sandbox: {
          builds: [
            {
              boot: [
                out({
                  uiEffects: [{ kind: 'alert', message: 'Stamp expired', icon: 0 }],
                  diagnostics: [{ code: 'unsupported-api', message: 'app.viewerVersion' }],
                  error: { kind: 'exception', message: 'boom' },
                }),
              ],
            },
          ],
        },
        onUiEffect: { $fn: 'onUiEffect' },
        onDiagnostic: { $fn: 'onDiagnostic' },
        onError: { $fn: 'onError' },
      }),
    ),
    engine: { documents: { a: target(), lib: assetDoc(ONE), copy: dynamicDoc() }, responses: opens('a', 'lib', 'copy') },
    snapshot: 'none',
    steps: [start, openDoc('a'), stamp('importLibraryPdf', [PDF]), arm('stamp-lib-0-1:Stamp1')],
  },
  {
    name: 'a-failed-recalculation-fails-the-arm',
    covers:
      "A recalculation that fails (here over its effect budget) surfaces its effects, diagnostics and error to the observers, then rejects with Unknown naming the script error; the controller is disposed, the copy closed, and nothing arms.",
    plugins: PLACEMENT(
      scripting({
        sandbox: {
          builds: [
            {
              boot: [
                out({
                  formEffects: [setValue(1, 'x')],
                  uiEffects: [{ kind: 'alert', message: 'Stamp expired', icon: 0 }],
                  diagnostics: [{ code: 'unsupported-api', message: 'app.viewerVersion' }],
                }),
              ],
            },
          ],
        },
        budget: { maxExecutionMs: 50, maxMemoryBytes: 16777216, maxStackBytes: 524288, maxEffects: 1, maxOutputBytes: 1048576 },
        onUiEffect: { $fn: 'onUiEffect' },
        onDiagnostic: { $fn: 'onDiagnostic' },
        onError: { $fn: 'onError' },
      }),
    ),
    engine: { documents: { a: target(), lib: assetDoc(ONE), copy: dynamicDoc() }, responses: opens('a', 'lib', 'copy') },
    snapshot: ['annotation::a'],
    steps: [start, openDoc('a'), stamp('importLibraryPdf', [PDF]), arm('stamp-lib-0-1:Stamp1')],
  },
  {
    name: 'an-error-observer-that-throws',
    covers: 'An onError observer that throws is reported to console.error; the failed recalculation still rejects the arm.',
    plugins: PLACEMENT(
      scripting({
        budget: { maxExecutionMs: 50, maxMemoryBytes: 16777216, maxStackBytes: 524288, maxEffects: 0, maxOutputBytes: 1048576 },
        onError: { $fn: 'onError', throws: { name: 'TypeError', message: 'observer is not ready' } },
      }),
    ),
    engine: { documents: { a: target(), lib: assetDoc(ONE), copy: dynamicDoc() }, responses: opens('a', 'lib', 'copy') },
    snapshot: 'none',
    steps: [start, openDoc('a'), stamp('importLibraryPdf', [PDF]), arm('stamp-lib-0-1:Stamp1')],
  },
  {
    name: 'observers-see-a-successful-run',
    covers:
      'A successful run surfaces its UI effects and diagnostics too; an observer that throws is reported to console.error and stops the rest, and the arm still goes ahead.',
    plugins: PLACEMENT(
      scripting({
        sandbox: {
          builds: [
            {
              boot: [
                out({
                  formEffects: [setValue(1, 'x')],
                  uiEffects: [
                    { kind: 'alert', message: 'one', icon: 0 },
                    { kind: 'alert', message: 'two', icon: 0 },
                  ],
                  diagnostics: [{ code: 'unsupported-api', message: 'app.viewerVersion' }],
                }),
              ],
            },
          ],
        },
        onUiEffect: { $fn: 'onUiEffect', seq: [{}, { throws: { name: 'Error', message: 'observer broke' } }] },
        onDiagnostic: { $fn: 'onDiagnostic' },
      }),
    ),
    engine: { documents: { a: target(), lib: assetDoc(ONE), copy: dynamicDoc() }, responses: opens('a', 'lib', 'copy') },
    snapshot: 'none',
    steps: [start, openDoc('a'), stamp('importLibraryPdf', [PDF]), arm('stamp-lib-0-1:Stamp1')],
  },
  {
    name: 'a-flatten-that-fails-or-skips',
    covers: 'A flatten result that failed or was skipped rejects with Unknown naming its page as the template literal prints it, whatever the engine put there; the copy is closed.',
    plugins: PLACEMENT(scripting()),
    engine: {
      documents: {
        a: target(),
        lib: assetDoc(ONE),
        failed: dynamicDoc({ flatten: { results: [{ page: { kind: 'objectNumber', pageObjectNumber: 100 }, status: 'failed' }] } }),
        skipped: dynamicDoc({ flatten: { results: [{ page: { kind: 'objectNumber', pageObjectNumber: 100 }, status: 'skipped' }] } }),
        text: dynamicDoc({ flatten: { results: [{ page: { kind: 'objectNumber', pageObjectNumber: 'p100' }, status: 'failed' }] } }),
        nil: dynamicDoc({ flatten: { results: [{ page: { kind: 'objectNumber', pageObjectNumber: null }, status: 'failed' }] } }),
        flag: dynamicDoc({ flatten: { results: [{ page: { kind: 'objectNumber', pageObjectNumber: true }, status: 'failed' }] } }),
        none: dynamicDoc({ flatten: { results: [{ page: {}, status: 'failed' }] } }),
      },
      responses: opens('a', 'lib', 'failed', 'skipped', 'text', 'nil', 'flag', 'none'),
    },
    snapshot: 'none',
    steps: [
      start,
      openDoc('a'),
      stamp('importLibraryPdf', [PDF]),
      arm('stamp-lib-0-1:Stamp1'),
      arm('stamp-lib-0-1:Stamp1'),
      arm('stamp-lib-0-1:Stamp1'),
      arm('stamp-lib-0-1:Stamp1'),
      arm('stamp-lib-0-1:Stamp1'),
      arm('stamp-lib-0-1:Stamp1'),
    ],
  },
  {
    name: 'a-failed-copy-close-fails-the-arm',
    covers: "A copy whose close rejects fails the arm with that error even after a successful evaluation; a URL-sourced render of the flattened page rejects with NotImplemented.",
    plugins: PLACEMENT(scripting()),
    engine: {
      documents: {
        a: target(),
        lib: assetDoc(ONE),
        copy: dynamicDoc({ responses: [{ path: 'close', error: engineError('Unknown', 'close failed') }] }),
        url: dynamicDoc({ responses: [{ path: 'page.render.image', value: { contentType: 'image/png', source: { kind: 'url', url: 'https://x' } } }] }),
      },
      responses: opens('a', 'lib', 'copy', 'url'),
    },
    snapshot: 'none',
    steps: [start, openDoc('a'), stamp('importLibraryPdf', [PDF]), arm('stamp-lib-0-1:Stamp1'), arm('stamp-lib-0-1:Stamp1')],
  },
  {
    name: 'a-library-without-canonical-bytes',
    covers:
      "When the library's save answered nothing, a dynamic asset evaluates from its own extracted page. An asset whose extraction answered nothing opens an empty copy, and when that copy has no field on its page it arms no bytes, which the annotation plugin refuses.",
    plugins: PLACEMENT(scripting()),
    engine: {
      documents: {
        a: target(),
        lib: assetDoc(TWO, { responses: [{ path: 'download' }, { path: 'pages.extract', args: [[101]] }] }),
        copy: dynamicDoc({ pages: TWO }),
      },
      responses: opens('a', 'lib', 'copy', 'copy'),
    },
    snapshot: 'none',
    steps: [start, openDoc('a'), stamp('importLibraryPdf', [PDF]), arm('stamp-lib-0-1:Stamp1'), stamp('disarm', ['a']), arm('stamp-lib-0-1:Stamp2')],
  },
  {
    name: 'teardown-destroys-a-factory-engine-once',
    covers: 'A materialization through a factory engine uses the memoized engine, and teardown destroys it.',
    plugins: PLACEMENT({ ...scripting(), assetEngine: 'factory' }),
    engine: { documents: { a: target(), lib: assetDoc(ONE), copy: dynamicDoc() }, responses: opens('a', 'lib', 'copy') },
    snapshot: 'none',
    steps: [start, openDoc('a'), stamp('importLibraryPdf', [PDF]), arm('stamp-lib-0-1:Stamp1'), destroy],
  },
];

/** Every case can create and extend libraries: the workshop document
 *  answers the opens a case does not name (see stamp-fixtures.ts). */
const workshop = (cases: ScenarioCase[]): ScenarioCase[] =>
  cases.map((c) => ({ ...c, engine: withWorkshop(c.engine as never) }));

export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/stamp-placement.json',
    describes: 'packages/plugin/stamp: armAsset and disarm through the annotation plugin, and dynamic stamps through form scripting.',
    cases: workshop([...staticCases, ...skipped, ...dynamic]),
  },
];
