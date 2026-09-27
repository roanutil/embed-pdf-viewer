/**
 * `packages/plugin/stamp`: importing a PDF as a canonical stamp library.
 * The asset engine is the kernel's fake engine, which hands out the library
 * documents named by each case's engine-level `open` answers. Replayed by
 * `crates/plugins/stamp/tests/conformance.rs`.
 */
import type { ScenarioCase } from '../scenario';
import type { Suite } from '../vectors';

import {
  GARBAGE,
  PDF,
  STAMP,
  advance,
  arr,
  assetDoc,
  assetPage,
  destroy,
  engineError,
  image,
  jpeg,
  nm,
  numEntry,
  opens,
  png,
  settle,
  stamp,
  start,
  str,
  withWorkshop,
} from './stamp-fixtures';

const TWO = [assetPage(0, 100), assetPage(1, 101)];
const ONE = [assetPage(0, 100)];

const importing: ScenarioCase[] = [
  {
    name: 'imports-every-page-as-a-vector-asset',
    covers:
      'importLibraryPdf opens the bytes on the asset engine, reads catalog and page PieceInfo, writes the normalized metadata back, extracts and renders every page, keeps the saved PDF as the canonical bytes, closes the document, and only then adds the library and its assets; the selectors read them back in order.',
    plugins: STAMP(),
    engine: { documents: { lib: assetDoc(TWO) }, responses: opens('lib') },
    steps: [
      start,
      stamp('importLibraryPdf', [PDF, { name: 'Approvals' }]),
      stamp('libraries'),
      stamp('library', ['stamp-lib-0-1']),
      stamp('library', ['nope']),
      stamp('assets', ['stamp-lib-0-1']),
      stamp('assets'),
      stamp('assets', ['nope']),
      stamp('assets', ['']),
      stamp('asset', ['stamp-lib-0-1:Stamp1']),
      stamp('asset', ['nope']),
      stamp('assetBytes', ['stamp-lib-0-1:Stamp2']),
      stamp('assetBytes', ['nope']),
      stamp('assetPreview', ['stamp-lib-0-1:Stamp1']),
      stamp('assetPreview', ['nope']),
      stamp('libraryBytes', ['stamp-lib-0-1']),
      stamp('libraryBytes', ['nope']),
    ],
  },
  {
    name: 'reads-ids-and-display-metadata-from-pieceinfo',
    covers:
      "Catalog Id, Name and Categories become the library's; page Id, Name, Kind (a name, matched case-insensitively), Subject and Categories become each asset's. An empty string, a Kind that is a string rather than a name, an unknown Kind and a non-array Categories are ignored.",
    plugins: STAMP(),
    engine: {
      documents: {
        lib: assetDoc([assetPage(0, 100), assetPage(1, 101), assetPage(2, 102), assetPage(3, 103)], {
          catalog: {
            Version: numEntry(1),
            Id: str('review-library'),
            Name: str('Review'),
            Categories: arr('Team', 'Legal'),
          },
          page: {
            100: {
              Id: str('review-library:Approved'),
              Name: str('Approved'),
              Kind: nm('Signature'),
              Subject: str('Approval signature'),
              Categories: arr('Review'),
            },
            101: { Id: str(''), Name: str(''), Kind: nm('INITIALS'), Subject: str(''), Categories: str('Review') },
            102: { Kind: str('Signature'), Name: nm('Not a string') },
            103: { Kind: nm('Watermark') },
          },
        }),
      },
      responses: opens('lib'),
    },
    steps: [start, stamp('importLibraryPdf', [PDF]), stamp('libraries'), stamp('assets')],
  },
  {
    name: 'a-name-kind-and-stamp-kinds-read-back',
    covers: "Kind 'stamp' reads as a stamp and an empty name as no kind; the default library name is 'Stamps'.",
    plugins: STAMP(),
    engine: {
      documents: {
        lib: assetDoc([assetPage(0, 100), assetPage(1, 101)], {
          catalog: { Name: str('') },
          page: { 100: { Kind: nm('sTaMp') }, 101: { Kind: nm('') } },
        }),
      },
      responses: opens('lib'),
    },
    steps: [start, stamp('importLibraryPdf', [PDF]), stamp('libraries'), stamp('assets')],
  },
  {
    name: 'import-options-override-the-pdf',
    covers:
      "opts.name, opts.categories and opts.kind win over PieceInfo (an empty name and empty categories included); opts.assetName names each page by index, and a nullish answer falls back to PieceInfo's Name, then to 'Stamp <n>'.",
    plugins: STAMP(),
    engine: {
      documents: {
        lib: assetDoc([assetPage(0, 100), assetPage(1, 101), assetPage(2, 102)], {
          catalog: { Name: str('Embedded'), Categories: arr('Embedded') },
          page: { 101: { Name: str('From PDF'), Kind: nm('Signature') } },
        }),
      },
      responses: opens('lib'),
    },
    steps: [
      start,
      stamp('importLibraryPdf', [
        PDF,
        {
          name: '',
          categories: [],
          kind: 'initials',
          assetName: { $fn: 'assetName', seq: [{ returns: 'First' }, { returns: null }, {}] },
        },
      ]),
      stamp('libraries'),
      stamp('assets'),
    ],
  },
  {
    name: 'a-payload-source-and-its-errors',
    covers:
      "A {data, mimeType} payload is read for its bytes; a non-PDF (PNG, JPEG, unknown bytes) rejects with InvalidArg before any engine call; a source that is not bytes or a payload rejects with the TypeError resolveBinarySource throws.",
    plugins: STAMP(),
    engine: { documents: { lib: assetDoc(ONE) }, responses: opens('lib') },
    steps: [
      start,
      stamp('importLibraryPdf', [png()]),
      stamp('importLibraryPdf', [jpeg()]),
      stamp('importLibraryPdf', [GARBAGE]),
      stamp('importLibraryPdf', [null]),
      stamp('importLibraryPdf', [{ data: 5 }]),
      stamp('importLibraryPdf', [{ data: PDF, mimeType: 'application/pdf', name: 'lib.pdf' }]),
      stamp('libraries'),
    ],
  },
];

const failures: ScenarioCase[] = [
  {
    name: 'a-cloud-kernel-engine-names-the-fix',
    covers:
      "With no assetEngine configured, an InvalidArg from the kernel engine's open becomes a NotImplemented naming the assetEngine option, and canImport turns false after it; any other open error rejects as it is.",
    plugins: STAMP(),
    engine: {
      responses: [
        { path: 'open', times: 1, error: engineError('InvalidArg', "cloud engine supports OpenInput kind 'token' or 'id'") },
        { path: 'open', times: 1, error: engineError('Unknown', 'engine offline') },
        { path: 'open', times: 1, error: { name: 'Error', message: 'plain failure' } },
      ],
    },
    steps: [
      start,
      stamp('canImport'),
      stamp('importLibraryPdf', [PDF]),
      stamp('canImport'),
      stamp('importLibraryPdf', [PDF]),
      stamp('importLibraryPdf', [PDF]),
      stamp('libraries'),
    ],
  },
  {
    name: 'a-configured-engine-keeps-its-invalid-arg',
    covers: 'A configured asset engine that refuses bytes rejects with its own InvalidArg: the configuration hint is only for the kernel engine.',
    plugins: STAMP({ assetEngine: 'instance' }),
    engine: { responses: [{ path: 'open', error: engineError('InvalidArg', 'no bytes here') }] },
    steps: [start, stamp('importLibraryPdf', [PDF])],
  },
  {
    name: 'missing-services-refuse-before-any-write',
    covers:
      'An asset document without catalog pieceInfo, or without page pieceInfo, rejects with NotImplemented before listing or before any read; one with no pages rejects with InvalidArg; a listed page the document does not know throws from page(). Every one closes the document and adds nothing.',
    plugins: STAMP(),
    engine: {
      documents: {
        bare: assetDoc(ONE, { services: [] }),
        catalogOnly: assetDoc(ONE, { services: ['pieceInfo'] }),
        empty: assetDoc([]),
        unknownPage: assetDoc(ONE, { registry: [assetPage(0, 7)] }),
      },
      responses: opens('bare', 'catalogOnly', 'empty', 'unknownPage'),
    },
    steps: [
      start,
      stamp('importLibraryPdf', [PDF]),
      stamp('importLibraryPdf', [PDF]),
      stamp('importLibraryPdf', [PDF]),
      stamp('importLibraryPdf', [PDF]),
      stamp('libraries'),
    ],
  },
  {
    name: 'an-engine-failure-mid-import-adds-nothing',
    covers:
      'A failed metadata write, extraction, render or save rejects the import with the engine error, still closes the document, and leaves the store and the binaries untouched.',
    plugins: STAMP(),
    engine: {
      documents: {
        update: assetDoc(ONE, { responses: [{ path: 'pieceInfo.update', error: engineError('Refused', 'catalog is read-only') }] }),
        pageUpdate: assetDoc(ONE, { responses: [{ path: 'page.pieceInfo.update', error: engineError('Refused', 'page is read-only') }] }),
        extract: assetDoc(ONE, { responses: [{ path: 'pages.extract', error: engineError('Unknown', 'extract failed') }] }),
        render: assetDoc(ONE, { responses: [{ path: 'page.render.image', error: engineError('Unknown', 'render failed') }] }),
        save: assetDoc(ONE, { responses: [{ path: 'download', error: { name: 'Error', message: 'save failed' } }] }),
        read: assetDoc(ONE, { responses: [{ path: 'page.pieceInfo.read', error: engineError('Unknown', 'read failed') }] }),
      },
      responses: opens('update', 'pageUpdate', 'extract', 'render', 'save', 'read'),
    },
    steps: [
      start,
      stamp('importLibraryPdf', [PDF]),
      stamp('importLibraryPdf', [PDF]),
      stamp('importLibraryPdf', [PDF]),
      stamp('importLibraryPdf', [PDF]),
      stamp('importLibraryPdf', [PDF]),
      stamp('importLibraryPdf', [PDF]),
      stamp('libraries'),
      stamp('assets'),
    ],
  },
  {
    name: 'a-failed-close-fails-the-import',
    covers: 'A close that rejects in the finally rejects the whole import, after a successful save, and nothing is added.',
    plugins: STAMP(),
    engine: {
      documents: { lib: assetDoc(ONE, { responses: [{ path: 'close', error: engineError('Unknown', 'close failed') }] }) },
      responses: opens('lib'),
    },
    steps: [start, stamp('importLibraryPdf', [PDF]), stamp('libraries')],
  },
  {
    name: 'previews-must-be-inline-bytes',
    covers:
      'A render whose source is a URL rejects with NotImplemented (asset engines must be local); a render without a content type caches a preview without a MIME type; previewWidth sets the render width.',
    plugins: STAMP({ previewWidth: 96 }),
    engine: {
      documents: {
        url: assetDoc(ONE, {
          responses: [{ path: 'page.render.image', value: { contentType: 'image/png', source: { kind: 'url', url: 'https://x/p.png' } } }],
        }),
        untyped: assetDoc(ONE, { responses: [{ path: 'page.render.image', value: image(100, undefined) }] }),
      },
      responses: opens('url', 'untyped'),
    },
    steps: [
      start,
      stamp('importLibraryPdf', [PDF]),
      stamp('importLibraryPdf', [PDF]),
      stamp('assetPreview', ['stamp-0-3']),
    ],
  },
];

const blankRenders: ScenarioCase[] = [
  {
    name: 'a-render-with-no-image-throws',
    covers: "A render that answers null, or an image whose source is null, rejects the import with the TypeError reading 'source' or 'kind' off null.",
    plugins: STAMP(),
    engine: {
      documents: {
        noImage: assetDoc(ONE, { responses: [{ path: 'page.render.image', value: null }] }),
        noSource: assetDoc(ONE, { responses: [{ path: 'page.render.image', value: { contentType: 'image/png', source: null } }] }),
      },
      responses: opens('noImage', 'noSource'),
    },
    steps: [start, stamp('importLibraryPdf', [PDF]), stamp('importLibraryPdf', [PDF]), stamp('libraries')],
  },
];

const ids: ScenarioCase[] = [
  {
    name: 'a-second-import-allocates-fresh-ids',
    covers:
      "Importing the same canonical library twice keeps the embedded ids the first time and allocates session ids the second, since both are taken; an embedded id equal to the next session id makes the allocator draw again.",
    plugins: STAMP(),
    engine: {
      documents: {
        lib: assetDoc(ONE, { catalog: { Id: str('shared-library') }, page: { 100: { Id: str('shared-library:Stamp1') } } }),
        clash: assetDoc(ONE, { catalog: { Id: str('stamp-lib-0-4') }, page: { 100: { Id: str('stamp-lib-0-4:Stamp1') } } }),
      },
      responses: opens('lib', 'lib', 'clash', 'lib'),
    },
    steps: [
      start,
      stamp('importLibraryPdf', [PDF]),
      stamp('importLibraryPdf', [PDF]),
      stamp('importLibraryPdf', [PDF]),
      stamp('importLibraryPdf', [PDF]),
      stamp('libraries'),
      stamp('assets'),
    ],
  },
  {
    name: 'ids-carry-the-clock',
    covers: 'Session ids are the prefix, Date.now() in base 36, and a counter in base 36.',
    plugins: STAMP(),
    engine: { documents: { lib: assetDoc(ONE) }, responses: opens('lib') },
    snapshot: ['stamp'],
    steps: [start, advance(123456789), stamp('importLibraryPdf', [PDF])],
  },
  {
    name: 'concurrent-imports-read-the-store-before-either-lands',
    covers:
      'Two imports whose reads both precede either dispatch allocate the same embedded ids: the second LIBRARY_ADDED and ASSET_ADDED overwrite the first. With different library ids and one shared asset id, removing the asset leaves the first library listing an id with no asset, which the selectors skip.',
    plugins: STAMP(),
    engine: {
      documents: {
        first: assetDoc(ONE, {
          catalog: { Id: str('lib-a') },
          page: { 100: { Id: str('shared') } },
          responses: [{ path: 'download', defer: 'save-first' }],
        }),
        second: assetDoc(ONE, { catalog: { Id: str('lib-b') }, page: { 100: { Id: str('shared') } }, download: 'second' }),
        again: assetDoc(ONE, {
          catalog: { Id: str('lib-c') },
          page: { 100: { Id: str('other') } },
          responses: [{ path: 'download', defer: 'save-again' }],
        }),
        same: assetDoc(ONE, { catalog: { Id: str('lib-c') }, page: { 100: { Id: str('other') } } }),
      },
      responses: opens('first', 'second', 'again', 'same'),
    },
    steps: [
      start,
      stamp('importLibraryPdf', [PDF]),
      stamp('importLibraryPdf', [PDF]),
      settle('save-first', PDF),
      stamp('assets', ['lib-a']),
      stamp('removeAsset', ['shared']),
      stamp('assets', ['lib-a']),
      stamp('assets', ['lib-b']),
      stamp('assets'),
      stamp('library', ['lib-a']),
      stamp('importLibraryPdf', [PDF]),
      stamp('importLibraryPdf', [PDF]),
      settle('save-again', PDF),
      stamp('libraries'),
      stamp('removeLibrary', ['lib-c']),
      stamp('libraries'),
      stamp('assets'),
    ],
  },
];

const engines: ScenarioCase[] = [
  {
    name: 'an-asset-engine-factory-is-called-once',
    covers:
      'A factory is called on the first import, not at start, and memoized for the second; kernel destroy destroys the engine it produced, after the documents close.',
    plugins: STAMP({ assetEngine: 'factory' }),
    engine: { documents: { lib: assetDoc(ONE) }, responses: opens('lib', 'lib') },
    snapshot: ['stamp'],
    steps: [start, stamp('importLibraryPdf', [PDF]), stamp('importLibraryPdf', [PDF]), destroy],
  },
  {
    name: 'an-async-asset-engine-factory',
    covers: 'A factory may answer a promise of an engine; it is awaited per open and destroyed on teardown.',
    plugins: STAMP({ assetEngine: 'async-factory' }),
    engine: { documents: { lib: assetDoc(ONE) }, responses: opens('lib') },
    snapshot: ['stamp'],
    steps: [start, stamp('importLibraryPdf', [PDF]), destroy],
  },
  {
    name: 'a-throwing-asset-engine-factory-is-retried',
    covers: 'A factory that throws rejects the import with its error and is called again next time; teardown has no engine to destroy.',
    plugins: STAMP({ assetEngine: { throws: { name: 'Error', message: 'wasm failed to load' } } }),
    snapshot: ['stamp'],
    steps: [start, stamp('importLibraryPdf', [PDF]), stamp('importLibraryPdf', [PDF]), destroy],
  },
  {
    name: 'a-rejecting-asset-engine-factory-is-memoized',
    covers: 'A factory whose promise rejects is memoized as that rejection: the second import rejects without calling it, and teardown swallows it.',
    plugins: STAMP({ assetEngine: { rejects: { name: 'Error', message: 'chunk load failed' } } }),
    snapshot: ['stamp'],
    steps: [start, stamp('importLibraryPdf', [PDF]), stamp('importLibraryPdf', [PDF]), destroy],
  },
  {
    name: 'an-engine-instance-is-not-destroyed',
    covers: 'A configured engine instance is used as is and left alone at teardown, as is the kernel engine; teardown drops every binary.',
    plugins: STAMP({ assetEngine: 'instance' }),
    engine: { documents: { lib: assetDoc(ONE) }, responses: opens('lib') },
    snapshot: ['stamp'],
    steps: [start, stamp('importLibraryPdf', [PDF]), destroy],
  },
  {
    name: 'the-plugin-factory-defaults',
    covers: "stampPlugin() with no config: a workspace plugin named 'stamp' over the kernel engine with 256px previews.",
    plugins: [{ factory: 'stamp-plugin' }],
    engine: { documents: { lib: assetDoc(ONE) }, responses: opens('lib') },
    steps: [start, stamp('importLibraryPdf', [PDF]), stamp('libraries')],
  },
];

/** Every case can create and extend libraries: the workshop document
 *  answers the opens a case does not name (see stamp-fixtures.ts). */
const workshop = (cases: ScenarioCase[]): ScenarioCase[] =>
  cases.map((c) => ({ ...c, engine: withWorkshop(c.engine as never) }));

export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/stamp-library.json',
    describes: 'packages/plugin/stamp: importing canonical PDF libraries, PieceInfo metadata, ids, the asset engine port and its failures.',
    cases: workshop([...importing, ...failures, ...blankRenders, ...ids, ...engines]),
  },
];
