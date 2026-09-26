/**
 * `packages/plugin/stamp`: the v3 surface the v2-shaped suites do not reach.
 * Library creation and edits, registered (`/Names /Pages`) libraries, marks
 * authored into assets, asset edits, moves and copies, assets from
 * annotations, filtered reads, rendered previews, programmatic placement,
 * the armed-asset read, and the change events. Replayed by
 * `crates/plugins/stamp/tests/conformance.rs`.
 */
import type { FakeDocumentSpec, ResponseSpec } from '../fake-engine';
import type { ScenarioCase } from '../scenario';
import type { Suite } from '../vectors';

import { created, hydrate, objRef, stamp as stampDto } from './annotation-fixtures';
import { list } from './form-fixtures';
import {
  PDF,
  PLACEMENT,
  STAMP,
  WORKSHOP_PAGE,
  arr,
  assetDoc,
  assetPage,

  engineError,

  nm,
  openDoc,
  opens,
  pdf,
  png,
  stamp,
  start,
  str,
  withWorkshop,
} from './stamp-fixtures';

const ONE = [assetPage(0, 100)];
const TWO = [assetPage(0, 100), assetPage(1, 101)];
const fn = (name: string, over: Record<string, unknown> = {}) => ({ $fn: name, ...over });
const page = (pon: number) => ({ kind: 'objectNumber', pageObjectNumber: pon });
const named = (name: string, pon: number) => ({ name, target: { kind: 'page', page: page(pon) } });

const EVENTS = [
  'onLibraryChanged',
  'onLibraryCreated',
  'onLibraryUpdated',
  'onLibraryDeleted',
  'onAssetCreated',
  'onAssetUpdated',
  'onAssetDeleted',
  'onArmChanged',
];
const listenAll = EVENTS.map((e) => stamp(e, [fn(e.slice(2))]));

/** The scratch page a mark is authored on: the blank seed's page 3, whose
 *  create answers an annotation and whose export answers a one-page PDF. */
const scratchDoc = (over: { responses?: ResponseSpec[] } = {}): FakeDocumentSpec =>
  assetDoc([assetPage(0, 3, 612, 792)], {
    responses: [
      ...(over.responses ?? []),
      { path: 'page.annotations.create', pon: 3, value: created(stampDto(40, { pageObjectNumber: 3 })) },
      { path: 'page.annotations.exportAppearance', pon: 3, value: pdf('mark') },
    ],
  });

const libraries: ScenarioCase[] = [
  {
    name: 'create-update-and-delete-libraries',
    covers:
      "createLibrary writes a blank seed's /Title and catalog PieceInfo and keeps the saved bytes; a preferred id is kept unless taken; kind and categories ride along. updateLibrary rewrites the title only when the name changes and the PieceInfo always; an unknown id rejects NotFound. exportLibrary copies the canonical bytes or rejects not-found. The library events and the change signal fire for each; filtered reads select by one kind or several.",
    plugins: STAMP(),
    steps: [
      start,
      ...listenAll,
      stamp('onLibraryCreated', [fn('LibraryCreated.bad', { throws: { message: 'listener broke' } })]),
      stamp('createLibrary', ['Mine']),
      stamp('createLibrary', ['Sigs', { id: 'sigs', kind: 'signatures', categories: ['Legal'] }]),
      stamp('createLibrary', ['Taken', { id: 'sigs' }]),
      stamp('listLibraries'),
      stamp('listLibraries', [{ kind: 'signatures' }]),
      stamp('listLibraries', [{ kind: ['stamps', 'signatures'] }]),
      stamp('listLibraries', [{ kind: [] }]),
      stamp('getLibrary', ['sigs']),
      stamp('getLibrary', ['nope']),
      stamp('updateLibrary', ['sigs', { name: 'Signatures', categories: ['A'] }]),
      stamp('updateLibrary', ['sigs', { categories: ['B'] }]),
      stamp('updateLibrary', ['sigs', {}]),
      stamp('updateLibrary', ['nope', { name: 'x' }]),
      stamp('exportLibrary', ['sigs']),
      stamp('exportLibrary', ['nope']),
      stamp('deleteLibrary', ['sigs']),
      stamp('deleteLibrary', ['sigs']),
      stamp('listLibraries'),
    ],
  },
  {
    name: 'library-writes-that-fail',
    covers:
      'A library whose save answered nothing has no canonical bytes: updating it or one of its assets rejects Unknown. A raster whose size is not positive rejects InvalidArg. A seed without named pages rejects NotImplemented before any write, and a failed save rejects after the close, adding nothing.',
    plugins: STAMP(),
    engine: {
      documents: {
        lib: assetDoc(ONE, { responses: [{ path: 'download' }] }),
        unnamed: assetDoc([assetPage(0, 3, 612, 792)], { lacks: ['pages.setName'] }),
        broken: assetDoc([assetPage(0, 3, 612, 792)], { responses: [{ path: 'download', error: engineError('Io', 'disk full') }] }),
      },
      responses: opens('lib', 'unnamed', 'broken'),
    },
    steps: [
      start,
      stamp('importLibraryPdf', [PDF]),
      stamp('updateLibrary', ['stamp-lib-0-1', { name: 'Renamed' }]),
      stamp('updateAsset', ['stamp-lib-0-1:Stamp1', { label: 'Renamed' }]),
      stamp('createAsset', [{ libraryId: 'stamp-lib-0-1', name: 'Flat', source: png(), size: { width: 0, height: 5 } }]),
      stamp('createLibrary', ['No names']),
      stamp('createLibrary', ['Unsaved']),
      stamp('listLibraries'),
    ],
  },
  {
    name: 'import-a-registered-library',
    covers:
      "A library whose /Names /Pages registry names its pages imports those keys as identifier and label, in page order whatever the key order; its /Title names it, its catalog kind and locale read back; a registration that is not a page is ignored with a warning; nothing is registered again.",
    plugins: STAMP(),
    engine: {
      documents: {
        lib: assetDoc(TWO, {
          title: 'Goedkeuringen',
          catalog: { Id: str('nl'), Kind: nm('SignatureLibrary'), Locale: str('nl-NL') },
          page: { 100: { Kind: nm('Initials'), SubjectOverride: str('Paraaf'), Categories: arr('x') } },
          namedPages: [named('#LBG=Custom', 101), named('Approved=Goedgekeurd', 100), { name: 'Hidden', target: { kind: 'template' } }],
        }),
      },
      responses: opens('lib'),
    },
    steps: [start, stamp('importLibraryPdf', [PDF, { name: 'Fallback' }]), stamp('listLibraries'), stamp('listAssets')],
  },
  {
    name: 'registry-refusals',
    covers:
      'A registry key with an empty identifier or a repeated one rejects InvalidArg naming it; an engine that reports no registry at all rejects NotImplemented; a library kind the plugin does not define is kept as written.',
    plugins: STAMP(),
    engine: {
      documents: {
        empty: assetDoc(ONE, { namedPages: [named('=Label', 100)] }),
        dup: assetDoc(TWO, { namedPages: [named('A=one', 100), named('A=two', 101)] }),
        old: assetDoc(ONE, { responses: [{ path: 'pages.list', value: { pageCount: 1, pages: ONE } }] }),
        custom: assetDoc(ONE, { catalog: { Kind: nm('toolbar') }, namedPages: [named('Only', 100)] }),
      },
      responses: opens('empty', 'dup', 'old', 'custom'),
    },
    steps: [
      start,
      stamp('importLibraryPdf', [PDF]),
      stamp('importLibraryPdf', [PDF]),
      stamp('importLibraryPdf', [PDF]),
      stamp('importLibraryPdf', [PDF]),
      stamp('listLibraries'),
      stamp('listAssets'),
    ],
  },
];

const marks: ScenarioCase[] = [
  {
    name: 'marks-become-assets',
    covers:
      "createAsset authors an ink mark (its box padded by the stroke, the colour parsed) or a typed one (a generous box, the default colour) on a scratch page and exports its appearance as the asset's page; an image mark is a raster; a PDF mark is used whole, or one page of it extracted. Without a name the identifier is minted Acrobat-style from Math.random, and the label defaults to it.",
    plugins: STAMP(),
    engine: {
      documents: {
        ink: scratchDoc(),
        text: scratchDoc(),
        source: assetDoc(TWO),
      },
      responses: opens('ink', 'workshop', 'workshop', 'text', 'workshop', 'workshop', 'workshop', 'workshop', 'workshop', 'workshop', 'source', 'workshop', 'workshop'),
    },
    steps: [
      start,
      stamp('createAsset', [{ name: 'Ink', mark: { kind: 'ink', strokes: [[{ x: 10, y: 20 }, { x: 40, y: 5 }], [{ x: 0, y: 30 }]], color: '#ff8000', strokeWidth: 3 } }]),
      stamp('createAsset', [{ label: 'Typed', mark: { kind: 'text', text: 'Ada', fontFamily: 'Helvetica' } }]),
      stamp('createAsset', [{ name: 'Picture', mark: { kind: 'image', source: png(20, 10) } }]),
      stamp('createAsset', [{ name: 'Whole', mark: { kind: 'pdf', source: PDF } }]),
      stamp('createAsset', [{ name: 'Second page', mark: { kind: 'pdf', source: PDF, pageIndex: 1 } }]),
      stamp('listAssets'),
    ],
  },
  {
    name: 'marks-that-are-refused',
    covers:
      "Both a source and a mark, or neither, reject InvalidArg; a colour that is not #rrggbb and an ink mark with no points reject InvalidArg after the scratch document closes; a PDF mark's missing page rejects InvalidArg naming the page count. An identifier that is empty or holds '=' rejects InvalidArg.",
    plugins: STAMP(),
    engine: {
      documents: { a: scratchDoc(), b: scratchDoc(), c: assetDoc(ONE) },
      responses: opens('a', 'b', 'c'),
    },
    steps: [
      start,
      stamp('createAsset', [{ name: 'X', source: png(), mark: { kind: 'image', source: png() } }]),
      stamp('createAsset', [{ name: 'X' }]),
      stamp('createAsset', [{ name: 'X', mark: { kind: 'text', text: 'x', fontFamily: 'Helvetica', color: 'red' } }]),
      stamp('createAsset', [{ name: 'X', mark: { kind: 'ink', strokes: [[]] } }]),
      stamp('createAsset', [{ name: 'X', mark: { kind: 'pdf', source: PDF, pageIndex: 4 } }]),
      stamp('createAsset', [{ name: '', source: png() }]),
      stamp('createAsset', [{ name: 'a=b', source: png() }]),
      stamp('listLibraries'),
    ],
  },
];

const extraMarks: ScenarioCase[] = [
  {
    name: 'an-ink-mark-with-defaults',
    covers: "An ink mark with no colour or stroke width takes the defaults (#1d2b53, 2); a scratch document with no page rejects Unknown after closing.",
    plugins: STAMP(),
    engine: {
      documents: {
        ink: scratchDoc(),
        blank: { pages: [], responses: [{ path: 'pages.list', value: { pageCount: 0, pages: [] } }] },
      },
      responses: opens('ink', 'workshop', 'workshop', 'blank'),
    },
    steps: [
      start,
      stamp('createAsset', [{ name: 'Plain ink', mark: { kind: 'ink', strokes: [[{ x: 1, y: 2 }]] } }]),
      stamp('createAsset', [{ name: 'Nowhere', mark: { kind: 'text', text: 'x', fontFamily: 'Helvetica' } }]),
      stamp('listAssets'),
    ],
  },
];

const assets: ScenarioCase[] = [
  {
    name: 'edit-move-and-copy-assets',
    covers:
      "updateAsset renames the registry entry only when the label changes, rewrites the page PieceInfo, and clears the subject for null; an unknown asset rejects NotFound. duplicateAsset appends a copy of the page with a minted identifier and a ' copy' label (or the one given); moveAsset copies into another library and removes the original, returns the id itself for the same library, and rejects not-found for an unknown asset, as duplicateAsset does. A second asset of the same name rejects InvalidArg.",
    plugins: STAMP(),
    engine: {
      documents: {
        lib: assetDoc(ONE, { page: { 100: { SubjectOverride: str('Keep'), Categories: arr('c') } } }),
        edit: assetDoc(ONE, { download: 'edited' }),
        remove: assetDoc(ONE, { responses: [{ path: 'pages.delete', value: { layout: null, cache: null } }], download: 'removed' }),
      },
      responses: opens('lib', 'edit', 'edit', 'workshop', 'workshop', 'workshop', 'workshop', 'remove'),
    },
    steps: [
      start,
      ...listenAll,
      stamp('importLibraryPdf', [PDF]),
      stamp('updateAsset', ['stamp-lib-0-1:Stamp1', { label: 'Relabelled', subject: 'New', categories: ['d'] }]),
      stamp('updateAsset', ['stamp-lib-0-1:Stamp1', { subject: null }]),
      stamp('updateAsset', ['nope', { label: 'x' }]),
      stamp('duplicateAsset', ['stamp-lib-0-1:Stamp1']),
      stamp('duplicateAsset', ['stamp-lib-0-1:Stamp1', { label: 'Named copy' }]),
      stamp('duplicateAsset', ['nope']),
      stamp('createLibrary', ['Other', { id: 'other' }]),
      stamp('moveAsset', ['stamp-lib-0-1:Stamp1', { libraryId: 'stamp-lib-0-1' }]),
      stamp('moveAsset', ['stamp-lib-0-1:Stamp1', { libraryId: 'other' }]),
      stamp('moveAsset', ['nope', { libraryId: 'other' }]),
      stamp('createAsset', [{ libraryId: 'other', name: 'Dup', source: PDF }]),
      stamp('createAsset', [{ libraryId: 'other', name: 'Dup', source: PDF }]),
      stamp('listAssets'),
      stamp('listAssets', [{ kind: 'stamp' }]),
      stamp('listAssets', [{ kind: 'signature' }]),
      stamp('listAssets', [{ category: 'd' }]),
      stamp('listAssets', [{ libraryId: 'other' }]),
    ],
  },
  {
    name: 'edits-queued-behind-removals',
    covers:
      'An asset edit queued behind the removal of that asset, and a library edit queued behind the removal of that library, find their target gone when their turn comes and reject NotFound. A move and a copy of an asset carry its subject, categories and preview.',
    plugins: STAMP(),
    engine: {
      documents: {
        lib: assetDoc(TWO, { page: { 101: { SubjectOverride: str('Subj'), Categories: arr('k') } } }),
        remove: assetDoc(TWO, { responses: [{ path: 'pages.delete', value: { layout: null, cache: null } }] }),
      },
      responses: [...opens('lib'), { path: 'open', times: 1, defer: 'reopen' }, ...opens('workshop', 'workshop', 'workshop', 'remove')],
    },
    steps: [
      start,
      stamp('importLibraryPdf', [PDF]),
      stamp('deleteAsset', ['stamp-lib-0-1:Stamp1']),
      stamp('updateAsset', ['stamp-lib-0-1:Stamp1', { label: 'Late' }]),
      stamp('deleteLibrary', ['stamp-lib-0-1']),
      stamp('updateLibrary', ['stamp-lib-0-1', { name: 'Late' }]),
      { op: 'settle', label: 'reopen', value: { $document: 'remove' } },
      stamp('listLibraries'),
    ],
  },
  {
    name: 'copies-carry-their-metadata',
    covers: "moveAsset and duplicateAsset pass the asset's subject, categories and preview to the copy.",
    plugins: STAMP(),
    engine: {
      documents: {
        lib: assetDoc(TWO, { page: { 101: { SubjectOverride: str('Subj'), Categories: arr('k') } } }),
        remove: assetDoc(TWO, { responses: [{ path: 'pages.delete', value: { layout: null, cache: null } }] }),
      },
      responses: opens('lib', 'workshop', 'workshop', 'workshop', 'workshop', 'remove'),
    },
    steps: [
      start,
      stamp('importLibraryPdf', [PDF]),
      stamp('duplicateAsset', ['stamp-lib-0-1:Stamp2']),
      stamp('duplicateAsset', ['stamp-lib-0-1:Stamp1']),
      stamp('createLibrary', ['Other', { id: 'other' }]),
      stamp('moveAsset', ['stamp-lib-0-1:Stamp2', { libraryId: 'other' }]),
      stamp('listAssets'),
    ],
  },
  {
    name: 'raster-pages-that-fail',
    covers: 'A raster page whose flatten does not apply rejects Unknown, and an asset engine without pages.flatten rejects NotImplemented; both close the library copy and add nothing.',
    plugins: STAMP(),
    engine: {
      documents: {
        flat: assetDoc([assetPage(0, 3, 612, 792), assetPage(1, WORKSHOP_PAGE, 300, 120)], {
          responses: [
            { path: 'pages.insertBlank', value: { insertedPages: [page(WORKSHOP_PAGE)], layout: null, cache: null } },
            { path: 'page.annotations.create', value: { created: null } },
            { path: 'pages.flatten', value: { results: [{ page: page(WORKSHOP_PAGE), status: 'failed' }], pages: [], cache: null } },
          ],
        }),
        noFlatten: assetDoc([assetPage(0, 3, 612, 792)], { lacks: ['pages.flatten'] }),
      },
      responses: opens('workshop', 'flat', 'noFlatten'),
    },
    steps: [
      start,
      stamp('createLibrary', ['Raster', { id: 'r' }]),
      stamp('createAsset', [{ libraryId: 'r', name: 'A', source: png() }]),
      stamp('createAsset', [{ libraryId: 'r', name: 'B', source: png() }]),
      stamp('listAssets'),
    ],
  },
  {
    name: 'an-asset-from-annotations',
    covers:
      "createAssetFromAnnotations exports the selected annotations' appearance from the open document as the source of a new asset; a document that is not open rejects NotFound.",
    plugins: PLACEMENT(),
    engine: {
      documents: {
        a: {
          pages: [assetPage(0, 1, 600, 800)],
          security: { allows: '*' },
          responses: [hydrate(), list([]), { path: 'page.annotations.exportAppearance', pon: 1, value: pdf('selection') }],
        },
      },
      responses: opens('a'),
    },
    steps: [
      start,
      openDoc('a'),
      stamp('createAssetFromAnnotations', ['a', page(1), [objRef(7)], { name: 'Selection', kind: 'signature' }]),
      stamp('createAssetFromAnnotations', ['closed', page(1), [objRef(7)], { name: 'Nowhere' }]),
      stamp('listAssets'),
    ],
  },
];

const previews: ScenarioCase[] = [
  {
    name: 'rendered-previews',
    covers:
      "renderAssetPreview opens the asset's page bytes, renders its first page at the width asked and closes, caching the render per width; a render that fails is reported and falls back to the cached thumbnail; a copy with no pages answers null; a render with no content type reads as image/png; an unknown asset rejects not-found. Removing the asset drops its renders.",
    plugins: STAMP(),
    engine: {
      documents: {
        lib: assetDoc(TWO),
        ghost: assetDoc([assetPage(0, 5, 50, 20)]),
        broken: assetDoc([assetPage(0, 5, 50, 20)], { responses: [{ path: 'page.render.image', error: engineError('Io', 'render failed') }] }),
        empty: { pages: [], responses: [{ path: 'pages.list', value: { pageCount: 0, pages: [] } }] },
        nomime: assetDoc([assetPage(0, 6, 50, 20)], { responses: [{ path: 'page.render.image', value: { source: { kind: 'bytes', bytes: pdf('no-type') } } }] }),
        remove: assetDoc(TWO, { responses: [{ path: 'pages.delete', value: { layout: null, cache: null } }] }),
      },
      responses: opens('lib', 'ghost', 'broken', 'empty', 'nomime', 'remove'),
    },
    steps: [
      start,
      stamp('importLibraryPdf', [PDF]),
      stamp('renderAssetPreview', ['stamp-lib-0-1:Stamp1', { width: 512 }]),
      stamp('renderAssetPreview', ['stamp-lib-0-1:Stamp1', { width: 512 }]),
      stamp('renderAssetPreview', ['stamp-lib-0-1:Stamp1', { width: 128 }]),
      stamp('renderAssetPreview', ['stamp-lib-0-1:Stamp2', { width: 512 }]),
      stamp('renderAssetPreview', ['nope', { width: 512 }]),
      stamp('renderAssetPreview', ['stamp-lib-0-1:Stamp2', { width: 64 }]),
      stamp('deleteAsset', ['stamp-lib-0-1:Stamp1']),
      stamp('renderAssetPreview', ['stamp-lib-0-1:Stamp1', { width: 512 }]),
    ],
  },
];

const TARGET: FakeDocumentSpec = {
  pages: [assetPage(0, 1, 600, 800), assetPage(1, 2, 600, 800)],
  security: { allows: '*' },
  responses: [
    hydrate(),
    list([]),
    { path: 'page.annotations.create', pon: 1, value: created(stampDto(31)) },
    { path: 'page.annotations.create', pon: 2, times: 1, error: engineError('Refused', 'page locked') },
    { path: 'page.annotations.create', pon: 2, value: created(stampDto(32, { pageObjectNumber: 2 })) },
  ],
};

const placement: ScenarioCase[] = [
  {
    name: 'place-without-the-pointer',
    covers:
      "placeAsset hands the target's annotation plugin the asset's payload and the placement and answers the placed annotation's ref; an unknown asset rejects NotFound. placeAssetOnPages places on every page or the ones listed, reporting each failure by its page as a stamp PluginError.",
    plugins: PLACEMENT(),
    engine: { documents: { a: TARGET, lib: assetDoc(ONE) }, responses: opens('a', 'lib') },
    steps: [
      start,
      openDoc('a'),
      stamp('importLibraryPdf', [PDF]),
      stamp('placeAsset', ['a', 'stamp-lib-0-1:Stamp1', { page: page(1), at: { x: 100, y: 100 }, targetWidth: 50 }]),
      stamp('placeAsset', ['a', 'nope', { page: page(1), at: { x: 1, y: 1 } }]),
      stamp('placeAssetOnPages', ['a', 'stamp-lib-0-1:Stamp1', 'all', { at: { x: 10, y: 10 } }]),
      stamp('placeAssetOnPages', ['a', 'stamp-lib-0-1:Stamp1', [page(2)], { at: { x: 20, y: 20 }, rotation: 90 }]),
      stamp('placeAssetOnPages', ['closed', 'stamp-lib-0-1:Stamp1', 'all', { at: { x: 1, y: 1 } }]),
    ],
  },
  {
    name: 'the-armed-asset',
    covers:
      "getArmedAsset answers the asset this plugin armed while the annotation plugin still holds a stamp, and null once a tool change dropped it or nothing was armed; disarm announces a change only when something was armed. canPlace follows the target's annotation create grant, the active document when none is named; canImport is optimistic.",
    plugins: PLACEMENT(),
    engine: { documents: { a: TARGET, lib: assetDoc(ONE) }, responses: opens('a', 'lib') },
    steps: [
      start,
      openDoc('a'),
      stamp('onArmChanged', [fn('armChanged')]),
      stamp('importLibraryPdf', [PDF]),
      stamp('getArmedAsset', ['a']),
      stamp('armAsset', ['a', 'stamp-lib-0-1:Stamp1']),
      stamp('getArmedAsset', ['a']),
      { op: 'call', target: { token: 'interaction', doc: 'a' }, method: 'activateTool', args: ['pan'] },
      stamp('getArmedAsset', ['a']),
      stamp('disarm', ['a']),
      stamp('disarm', ['a']),
      stamp('canPlace', ['a']),
      stamp('canPlace'),
      stamp('canPlace', ['closed']),
      stamp('canImport'),
    ],
  },
  {
    name: 'can-place-without-a-document',
    covers: 'canPlace with no document named and none active answers false.',
    plugins: STAMP(),
    steps: [start, stamp('canPlace')],
  },
];

const persist = (method: string, args: unknown[] = []) => ({ op: 'call' as const, target: { token: 'stamp-persistence' }, method, args });
const PERSIST = (config?: unknown) => [...STAMP(), config === undefined ? { factory: 'stamp-persistence' } : { factory: 'stamp-persistence', config }];

const persistence: ScenarioCase[] = [
  {
    name: 'restore-stored-libraries',
    covers:
      'restoreStampLibraries imports every stored library as it is and answers their ids; one that fails to import is reported and skipped. A store whose list rejects rejects the restore.',
    plugins: PERSIST(),
    engine: { documents: { lib: assetDoc(ONE, { title: 'Kept' }) }, responses: opens('lib') },
    steps: [
      start,
      persist('seed', [[{ id: 'kept', bytes: PDF }, { id: 'junk', bytes: png() }]]),
      persist('restore'),
      stamp('listLibraries'),
    ],
  },
  {
    name: 'a-store-that-cannot-list',
    covers: 'A store whose list rejects rejects the restore with that error.',
    plugins: PERSIST({ fail: { list: 'offline' } }),
    steps: [start, persist('restore')],
  },
  {
    name: 'persist-library-changes',
    covers:
      "persistStampLibraries writes a library's exported PDF to the store once its changes stop for the debounce, a burst coalescing into one write; a removal deletes the stored copy at once and drops a pending write; an excepted library is never stored. The stop function unsubscribes and flushes pending writes immediately.",
    plugins: PERSIST(),
    steps: [
      start,
      persist('persist', [{ except: ['skip'], debounceMs: 100 }]),
      stamp('createLibrary', ['A', { id: 'a' }]),
      stamp('createLibrary', ['Skip', { id: 'skip' }]),
      { op: 'advance', ms: 50 },
      stamp('updateLibrary', ['a', { name: 'A2' }]),
      { op: 'advance', ms: 99 },
      persist('rows'),
      { op: 'advance', ms: 1 },
      persist('rows'),
      stamp('createLibrary', ['B', { id: 'b' }]),
      stamp('deleteLibrary', ['a']),
      persist('rows'),
      persist('stop'),
      persist('rows'),
      stamp('createLibrary', ['C', { id: 'c' }]),
      { op: 'advance', ms: 1000 },
      persist('rows'),
    ],
  },
  {
    name: 'persistence-failures-are-reported',
    covers: 'A store write or delete that rejects is reported with the library id and leaves the stream running; the default debounce is 250 ms.',
    plugins: PERSIST({ fail: { put: 'quota exceeded', delete: 'locked' } }),
    steps: [
      start,
      persist('persist'),
      stamp('createLibrary', ['A', { id: 'a' }]),
      { op: 'advance', ms: 250 },
      stamp('deleteLibrary', ['a']),
      persist('rows'),
    ],
  },
];

const workshop = (cases: ScenarioCase[]): ScenarioCase[] =>
  cases.map((c) => ({ ...c, engine: withWorkshop(c.engine as never) }));

export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/stamp-api.json',
    describes: 'packages/plugin/stamp: library and asset edits, marks, registered libraries, previews, placement without the pointer, and the change events.',
    cases: workshop([...libraries, ...marks, ...extraMarks, ...assets, ...previews, ...placement, ...persistence]),
  },
];

