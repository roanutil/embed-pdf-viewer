/**
 * `packages/plugin/stamp`: single assets. Loose raster and PDF assets, pages
 * appended to and deleted from a canonical library, library removal, and the
 * per-library mutation queue that orders them. Replayed by
 * `crates/plugins/stamp/tests/conformance.rs`.
 */
import type { ResponseSpec } from '../fake-engine';
import type { ScenarioCase } from '../scenario';
import type { Suite } from '../vectors';

import {
  GARBAGE,
  PDF,
  STAMP,
  arr,
  assetDoc,
  bytes,
  assetPage,
  destroy,
  engineError,
  jpeg,
  nm,
  opens,
  pdf,
  png,
  settle,
  stamp,
  start,
  str,
  withWorkshop,
} from './stamp-fixtures';

const ONE = [assetPage(0, 100)];
const TWO = [assetPage(0, 100), assetPage(1, 101)];
const THREE = [assetPage(0, 100), assetPage(1, 101), assetPage(2, 102)];

/** The canonical library reopened for an append: `pages.insert` adds page
 *  `pon` (300 x 120), which the registry already knows. */
const appendDoc = (pages: typeof ONE, pon = 101, over: { responses?: ResponseSpec[]; services?: string[] } = {}) => {
  const added = assetPage(pages.length, pon, 300, 120);
  const after = [...pages, added];
  return assetDoc(pages, {
    registry: after,
    ...(over.services ? { services: over.services } : {}),
    responses: [
      ...(over.responses ?? []),
      { path: 'pages.insert', value: { insertedPages: [{ kind: 'objectNumber', pageObjectNumber: pon }], layout: { pageCount: after.length, pages: after }, cache: null } },
      { path: 'pages.extract', args: [[{ kind: 'objectNumber', pageObjectNumber: pon }]], value: pdf(`page-${pon}`) },
      { path: 'page.render.image', pon, value: { contentType: 'image/png', source: { kind: 'bytes', bytes: bytes(`png-${pon}`) } } },
    ],
    download: 'canonical-2',
  });
};

/** The canonical library reopened for a delete. */
const deleteDoc = (pages: typeof ONE, over: { responses?: ResponseSpec[] } = {}) =>
  assetDoc(pages, {
    responses: [
      ...(over.responses ?? []),
      { path: 'pages.delete', value: { layout: { pageCount: pages.length - 1, pages: pages.slice(1) }, cache: null } },
    ],
    download: 'canonical-rewritten',
  });

const loose: ScenarioCase[] = [
  {
    name: 'a-raster-asset-is-sniffed-and-previews-itself',
    covers:
      "addAsset without a library sniffs a PNG's or JPEG's size, keeps its bytes as its own preview, and adds a loose single-asset library named after it; kind, subject and categories ride along.",
    plugins: STAMP(),
    steps: [
      start,
      stamp('addAsset', [{ name: 'Logo', source: png(100, 50) }]),
      stamp('addAsset', [{ name: 'Photo', kind: 'signature', subject: 'Scan', categories: ['Ink'], source: jpeg(64, 32) }]),
      stamp('libraries'),
      stamp('assets'),
      stamp('assetPreview', ['stamp-lib-0-0:Logo']),
      stamp('assetBytes', ['stamp-lib-0-3:Photo']),
      stamp('libraryBytes', ['stamp-lib-0-0']),
    ],
  },
  {
    name: 'supplied-previews-and-sizes',
    covers:
      "An explicit size wins over the sniffed one; a supplied preview wins over the raster's own bytes and keeps its payload MIME type, which defaults to image/png when absent or empty.",
    plugins: STAMP(),
    steps: [
      start,
      stamp('addAsset', [{ name: 'Sized', source: png(10, 10), size: { width: 40, height: 20 }, preview: { data: jpeg(), mimeType: 'image/jpeg' } }]),
      stamp('addAsset', [{ name: 'Bare preview', source: png(), preview: png(8, 8) }]),
      stamp('addAsset', [{ name: 'Empty type', source: png(), preview: { data: png(8, 8), mimeType: '' } }]),
      stamp('assets'),
      stamp('assetPreview', ['stamp-lib-0-0:Sized']),
      stamp('assetPreview', ['stamp-lib-0-3:Bare preview']),
      stamp('assetPreview', ['stamp-lib-0-6:Empty type']),
    ],
  },
  {
    name: 'a-loose-pdf-needs-a-size',
    covers:
      'A PDF added without a library needs size (PDF bytes carry none): without it the call rejects and nothing is added; with it the asset is a vector with no preview unless one is supplied.',
    plugins: STAMP(),
    steps: [
      start,
      stamp('addAsset', [{ name: 'Sig', source: PDF }]),
      stamp('addAsset', [{ name: 'Sig', source: PDF, size: { width: 150, height: 60 } }]),
      stamp('addAsset', [{ name: 'Sig preview', source: PDF, size: { width: 150, height: 60 }, preview: png() }]),
      stamp('assets'),
      stamp('assetPreview', ['stamp-lib-0-0:Sig']),
      stamp('assetPreview', ['stamp-lib-0-3:Sig']),
    ],
  },
  {
    name: 'asset-sources-that-are-refused',
    covers:
      "An unknown libraryId rejects with NotFound before reading the source; bytes that are not PNG, JPEG or PDF reject with InvalidArg; a non-source rejects with resolveBinarySource's TypeError, and a bad preview with its own.",
    plugins: STAMP(),
    steps: [
      start,
      stamp('addAsset', [{ libraryId: 'missing', name: 'X', source: png() }]),
      stamp('addAsset', [{ name: 'X', source: GARBAGE }]),
      stamp('addAsset', [{ name: 'X', source: bytesOf(3) }]),
      stamp('addAsset', [{ name: 'X', source: null }]),
      stamp('addAsset', [{ name: 'X', source: png(), preview: { data: null } }]),
      stamp('addAsset', [{ name: 'X', source: png(0, 5) }]),
      stamp('libraries'),
    ],
  },
  {
    name: 'into-an-existing-loose-library',
    covers:
      "addAsset with a loose library's id queues on that library and appends the asset; an empty libraryId is no library at all.",
    plugins: STAMP(),
    steps: [
      start,
      stamp('addAsset', [{ name: 'First', source: png() }]),
      stamp('addAsset', [{ libraryId: 'stamp-lib-0-0', name: 'Second', source: PDF, size: { width: 1, height: 2 } }]),
      stamp('addAsset', [{ libraryId: '', name: 'Own library', source: png() }]),
      stamp('libraries'),
      stamp('assets', ['stamp-lib-0-0']),
    ],
  },
];

function bytesOf(n: number) {
  return { $bytes: Buffer.alloc(n, 0x25).toString('base64') };
}

const canonical: ScenarioCase[] = [
  {
    name: 'appends-a-page-to-a-canonical-library',
    covers:
      'addAsset on a canonical library reopens its canonical bytes, inserts the PDF, writes the page PieceInfo, extracts and renders the new page, saves the rewritten library and closes; the asset takes the inserted page size.',
    plugins: STAMP(),
    engine: { documents: { lib: assetDoc(ONE), append: appendDoc(ONE) }, responses: opens('lib', 'append') },
    steps: [
      start,
      stamp('importLibraryPdf', [PDF]),
      stamp('addAsset', [
        { libraryId: 'stamp-lib-0-1', name: 'Signed', kind: 'signature', subject: 'Customer sign-off', categories: ['Signature'], source: PDF, size: { width: 9, height: 9 } },
      ]),
      stamp('assets', ['stamp-lib-0-1']),
      stamp('libraryBytes', ['stamp-lib-0-1']),
      stamp('assetBytes', ['stamp-0-3']),
      stamp('assetPreview', ['stamp-0-3']),
    ],
  },
  {
    name: 'a-supplied-preview-skips-the-render',
    covers: 'An append with a supplied preview keeps it and renders nothing; the kind defaults to stamp.',
    plugins: STAMP(),
    engine: { documents: { lib: assetDoc(ONE), append: appendDoc(ONE) }, responses: opens('lib', 'append') },
    steps: [
      start,
      stamp('importLibraryPdf', [PDF]),
      stamp('addAsset', [{ libraryId: 'stamp-lib-0-1', name: 'Plain', source: PDF, preview: { data: png(), mimeType: 'image/webp' } }]),
      stamp('assetPreview', ['stamp-0-3']),
      stamp('asset', ['stamp-0-3']),
    ],
  },
  {
    name: 'appends-that-are-refused',
    covers:
      'A raster joins a canonical library as a page of its own (v2 refused it before any engine call; here the workshop document takes that append). An insert of more than one page, an inserted page missing from the layout, a document without page pieceInfo and an inserted page the handle does not know each reject after closing the document, adding nothing.',
    plugins: STAMP(),
    engine: {
      documents: {
        lib: assetDoc(ONE),
        multi: assetDoc(ONE, {
          responses: [{ path: 'pages.insert', value: { insertedPages: [{ kind: 'objectNumber', pageObjectNumber: 101 }, { kind: 'objectNumber', pageObjectNumber: 102 }], layout: { pageCount: 3, pages: THREE }, cache: null } }],
        }),
        unlisted: assetDoc(ONE, {
          registry: TWO,
          responses: [{ path: 'pages.insert', value: { insertedPages: [{ kind: 'objectNumber', pageObjectNumber: 101 }], layout: { pageCount: 1, pages: ONE }, cache: null } }],
        }),
        noPageInfo: appendDoc(ONE, 101, { services: ['pieceInfo'] }),
        unknown: assetDoc(ONE, {
          responses: [{ path: 'pages.insert', value: { insertedPages: [{ kind: 'objectNumber', pageObjectNumber: 555 }], layout: { pageCount: 2, pages: TWO }, cache: null } }],
        }),
      },
      responses: opens('lib', 'workshop', 'multi', 'unlisted', 'noPageInfo', 'unknown'),
    },
    steps: [
      start,
      stamp('importLibraryPdf', [PDF]),
      stamp('addAsset', [{ libraryId: 'stamp-lib-0-1', name: 'Logo', source: png() }]),
      stamp('addAsset', [{ libraryId: 'stamp-lib-0-1', name: 'Two pages', source: PDF }]),
      stamp('addAsset', [{ libraryId: 'stamp-lib-0-1', name: 'Unlisted', source: PDF }]),
      stamp('addAsset', [{ libraryId: 'stamp-lib-0-1', name: 'No page info', source: PDF }]),
      stamp('addAsset', [{ libraryId: 'stamp-lib-0-1', name: 'Unknown', source: PDF }]),
      stamp('assets', ['stamp-lib-0-1']),
      stamp('libraryBytes', ['stamp-lib-0-1']),
    ],
  },
  {
    name: 'an-unsaved-append-changes-nothing',
    covers: 'A save that fails rejects the append after closing; the store and the canonical bytes are as they were, and the next append starts from them.',
    plugins: STAMP(),
    engine: {
      documents: {
        lib: assetDoc(ONE),
        broken: appendDoc(ONE, 101, { responses: [{ path: 'download', error: { name: 'Error', message: 'save failed' } }] }),
        append: appendDoc(ONE),
      },
      responses: opens('lib', 'broken', 'append'),
    },
    steps: [
      start,
      stamp('importLibraryPdf', [PDF]),
      stamp('addAsset', [{ libraryId: 'stamp-lib-0-1', name: 'Not saved', source: PDF }]),
      stamp('assets', ['stamp-lib-0-1']),
      stamp('libraryBytes', ['stamp-lib-0-1']),
      stamp('addAsset', [{ libraryId: 'stamp-lib-0-1', name: 'Saved', source: PDF }]),
      stamp('libraryBytes', ['stamp-lib-0-1']),
    ],
  },
  {
    name: 'a-save-that-answers-nothing',
    covers:
      "An engine whose save and extract answer nothing leaves the library with no canonical bytes and the asset with none: the selectors read null, and an append or a removal on that library rejects with Unknown before opening anything.",
    plugins: STAMP(),
    engine: {
      documents: {
        lib: assetDoc(TWO, { responses: [{ path: 'download' }, { path: 'pages.extract', args: [[100]] }] }),
      },
      responses: opens('lib'),
    },
    steps: [
      start,
      stamp('importLibraryPdf', [PDF]),
      stamp('libraryBytes', ['stamp-lib-0-1']),
      stamp('assetBytes', ['stamp-lib-0-1:Stamp1']),
      stamp('addAsset', [{ libraryId: 'stamp-lib-0-1', name: 'Late', source: PDF }]),
      stamp('removeAsset', ['stamp-lib-0-1:Stamp1']),
      stamp('assets'),
    ],
  },
];

const removal: ScenarioCase[] = [
  {
    name: 'removes-a-canonical-page-first',
    covers:
      'removeAsset on a canonical library with other assets deletes its page from the canonical bytes and saves them before the descriptor goes; an unknown id resolves and does nothing.',
    plugins: STAMP(),
    engine: { documents: { lib: assetDoc(TWO), remove: deleteDoc(TWO) }, responses: opens('lib', 'remove') },
    steps: [
      start,
      stamp('importLibraryPdf', [PDF]),
      stamp('removeAsset', ['nope']),
      stamp('removeAsset', ['stamp-lib-0-1:Stamp1']),
      stamp('asset', ['stamp-lib-0-1:Stamp1']),
      stamp('assetBytes', ['stamp-lib-0-1:Stamp1']),
      stamp('assets', ['stamp-lib-0-1']),
      stamp('libraryBytes', ['stamp-lib-0-1']),
    ],
  },
  {
    name: 'removing-the-last-page-drops-the-library',
    covers: "Removing a canonical library's only asset drops the whole library instead of saving a zero-page PDF; a loose asset goes without any engine call and leaves its library.",
    plugins: STAMP(),
    engine: { documents: { lib: assetDoc(ONE) }, responses: opens('lib') },
    steps: [
      start,
      stamp('importLibraryPdf', [PDF]),
      stamp('removeAsset', ['stamp-lib-0-1:Stamp1']),
      stamp('libraries'),
      stamp('libraryBytes', ['stamp-lib-0-1']),
      stamp('addAsset', [{ name: 'Logo', source: png() }]),
      stamp('removeAsset', ['stamp-lib-0-2:Logo']),
      stamp('libraries'),
      stamp('assetBytes', ['stamp-lib-0-2:Logo']),
    ],
  },
  {
    name: 'a-failed-page-delete-keeps-the-asset',
    covers: 'A delete or save that fails rejects removeAsset after closing, and the asset, its binaries and the canonical bytes stay.',
    plugins: STAMP(),
    engine: {
      documents: {
        lib: assetDoc(TWO),
        refuse: deleteDoc(TWO, { responses: [{ path: 'pages.delete', error: engineError('Refused', 'locked') }] }),
      },
      responses: opens('lib', 'refuse'),
    },
    steps: [
      start,
      stamp('importLibraryPdf', [PDF]),
      stamp('removeAsset', ['stamp-lib-0-1:Stamp1']),
      stamp('asset', ['stamp-lib-0-1:Stamp1']),
      stamp('assetBytes', ['stamp-lib-0-1:Stamp1']),
      stamp('libraryBytes', ['stamp-lib-0-1']),
    ],
  },
  {
    name: 'removals-are-serialized-per-library',
    covers:
      'Removals on one library run one after the other: the first rewrite leaves one page, so the queued second drops the library rather than saving an empty PDF, and a third for the same asset finds it gone and does nothing. A removal on another library does not wait.',
    plugins: STAMP(),
    engine: {
      documents: {
        lib: assetDoc(TWO),
        other: assetDoc(TWO, { catalog: { Id: str('other') }, page: { 100: { Id: str('other:Stamp1') }, 101: { Id: str('o2') } } }),
        remove: deleteDoc(TWO),
        removeOther: deleteDoc(TWO),
      },
      responses: [
        ...opens('lib', 'other'),
        { path: 'open', times: 1, defer: 'reopen' },
        ...opens('removeOther'),
      ],
    },
    steps: [
      start,
      stamp('importLibraryPdf', [PDF]),
      stamp('importLibraryPdf', [PDF]),
      stamp('removeAsset', ['stamp-lib-0-1:Stamp1']),
      stamp('removeAsset', ['stamp-lib-0-1:Stamp2']),
      stamp('removeAsset', ['stamp-lib-0-1:Stamp2']),
      stamp('removeAsset', ['other:Stamp1']),
      settle('reopen', { $document: 'remove' }),
      stamp('libraries'),
      stamp('libraryBytes', ['stamp-lib-0-1']),
      stamp('assets'),
    ],
  },
  {
    name: 'remove-library',
    covers:
      'removeLibrary drops the library, its assets, their binaries and its canonical bytes; an unknown id still dispatches LIBRARY_REMOVED, which changes nothing.',
    plugins: STAMP(),
    engine: { documents: { lib: assetDoc(TWO) }, responses: opens('lib') },
    steps: [
      start,
      stamp('importLibraryPdf', [PDF]),
      stamp('addAsset', [{ name: 'Logo', source: png() }]),
      stamp('removeLibrary', ['nope']),
      stamp('removeLibrary', ['stamp-lib-0-1']),
      stamp('libraries'),
      stamp('assets'),
      stamp('assetBytes', ['stamp-lib-0-1:Stamp1']),
      stamp('libraryBytes', ['stamp-lib-0-1']),
      stamp('removeLibrary', ['stamp-lib-0-2']),
      stamp('assetBytes', ['stamp-lib-0-2:Logo']),
      stamp('libraries'),
    ],
  },
];

const queued: ScenarioCase[] = [
  {
    name: 'an-append-queued-behind-a-removed-library',
    covers:
      "An append queued behind a removal and a removeLibrary of the same library finds it gone when its turn comes and rejects with NotFound; the removal's rewrite still lands first.",
    plugins: STAMP(),
    engine: {
      documents: { lib: assetDoc(TWO), remove: deleteDoc(TWO) },
      responses: [...opens('lib'), { path: 'open', times: 1, defer: 'reopen' }],
    },
    steps: [
      start,
      stamp('importLibraryPdf', [PDF]),
      stamp('removeAsset', ['stamp-lib-0-1:Stamp1']),
      stamp('removeLibrary', ['stamp-lib-0-1']),
      stamp('addAsset', [{ libraryId: 'stamp-lib-0-1', name: 'Late', source: PDF }]),
      settle('reopen', { $document: 'remove' }),
      stamp('libraries'),
    ],
  },
  {
    name: 'a-loose-append-queued-behind-a-removed-library',
    covers:
      "A session id is not checked against the store: a loose library created while a canonical removal on the same id is in flight replaces the canonical one. A loose append queued behind that removal and a removeLibrary finds the library gone and rejects with NotFound.",
    plugins: STAMP(),
    engine: {
      documents: {
        lib: assetDoc(TWO, { catalog: { Id: str('stamp-lib-0-2') }, page: { 100: { Id: str('stamp-lib-0-2:Stamp1') }, 101: { Id: str('stamp-lib-0-2:Stamp2') } } }),
        remove: deleteDoc(TWO),
      },
      responses: [...opens('lib'), { path: 'open', times: 1, defer: 'reopen' }],
    },
    steps: [
      start,
      stamp('importLibraryPdf', [PDF]),
      stamp('removeAsset', ['stamp-lib-0-2:Stamp1']),
      stamp('addAsset', [{ name: 'Logo', source: png() }]),
      stamp('libraries'),
      stamp('removeLibrary', ['stamp-lib-0-2']),
      stamp('addAsset', [{ libraryId: 'stamp-lib-0-2', name: 'Late', source: png() }]),
      settle('reopen', { $document: 'remove' }),
      stamp('libraries'),
      stamp('assets'),
    ],
  },
  {
    name: 'a-loose-append-finds-its-library-canonical',
    covers:
      "A loose append queued on a library that an import has since replaced with a canonical one finds a library that is not loose when its turn comes, and rejects with NotFound.",
    plugins: STAMP(),
    engine: {
      documents: {
        first: assetDoc(TWO, {
          catalog: { Id: str('stamp-lib-0-3') },
          page: { 100: { Id: str('stamp-lib-0-3:Stamp1') }, 101: { Id: str('stamp-lib-0-3:Stamp2') } },
          responses: [{ path: 'download', defer: 'save-first' }],
        }),
        second: assetDoc(TWO, {
          catalog: { Id: str('stamp-lib-0-3') },
          page: { 100: { Id: str('stamp-lib-0-3:Stamp2') }, 101: { Id: str('stamp-lib-0-2') } },
          responses: [{ path: 'download', defer: 'save-second' }],
        }),
        remove: deleteDoc(TWO),
      },
      responses: [...opens('first', 'second'), { path: 'open', times: 1, defer: 'reopen' }],
    },
    steps: [
      start,
      stamp('importLibraryPdf', [PDF]),
      stamp('importLibraryPdf', [PDF]),
      settle('save-first', PDF),
      stamp('removeAsset', ['stamp-lib-0-3:Stamp1']),
      stamp('addAsset', [{ name: 'Logo', source: png() }]),
      stamp('addAsset', [{ libraryId: 'stamp-lib-0-3', name: 'Late', source: png() }]),
      settle('save-second', PDF),
      settle('reopen', { $document: 'remove' }),
      stamp('libraries'),
    ],
  },
  {
    name: 'a-loose-asset-under-a-canonical-id',
    covers:
      "An import that lands after a loose library took its embedded id replaces the library with the canonical one; removing the loose asset then finds a canonical library it has no page in and rejects with Unknown.",
    plugins: STAMP(),
    engine: {
      documents: {
        lib: assetDoc(TWO, {
          catalog: { Id: str('stamp-lib-0-1') },
          page: { 100: { Id: str('stamp-lib-0-1:Stamp1'), Kind: nm('Stamp') }, 101: { Id: str('stamp-lib-0-1:Stamp2'), Categories: arr('x') } },
          responses: [{ path: 'download', defer: 'save' }],
        }),
      },
      responses: opens('lib'),
    },
    steps: [
      start,
      stamp('importLibraryPdf', [PDF]),
      stamp('addAsset', [{ name: 'Logo', source: png() }]),
      settle('save', PDF),
      stamp('libraries'),
      stamp('removeAsset', ['stamp-lib-0-1:Logo']),
      stamp('asset', ['stamp-lib-0-1:Logo']),
    ],
  },
  {
    name: 'a-queued-mutation-after-destroy',
    covers:
      "A mutation still queued when the kernel is destroyed runs against a destroyed store: the in-flight removal's dispatch lands nowhere, and the queued append's read of the state throws a TypeError.",
    plugins: STAMP(),
    engine: {
      documents: { lib: assetDoc(THREE), remove: deleteDoc(THREE) },
      responses: [...opens('lib'), { path: 'open', times: 1, defer: 'reopen' }],
    },
    snapshot: ['stamp'],
    steps: [
      start,
      stamp('importLibraryPdf', [PDF]),
      stamp('removeAsset', ['stamp-lib-0-1:Stamp1']),
      stamp('addAsset', [{ libraryId: 'stamp-lib-0-1', name: 'Late', source: PDF }]),
      destroy,
      settle('reopen', { $document: 'remove' }),
    ],
  },
];

/** Every case can create and extend libraries: the workshop document
 *  answers the opens a case does not name (see stamp-fixtures.ts). */
const workshop = (cases: ScenarioCase[]): ScenarioCase[] =>
  cases.map((c) => ({ ...c, engine: withWorkshop(c.engine as never) }));

export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/stamp-assets.json',
    describes: 'packages/plugin/stamp: loose and canonical assets, removal, and the per-library mutation queue.',
    cases: workshop([...loose, ...canonical, ...removal, ...queued]),
  },
];
