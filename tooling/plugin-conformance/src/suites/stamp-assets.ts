/** Current stamp asset mutations: real PDF pages, resources, queues and rollback. */
import type { PageInsertResult } from '@embedpdf/engine-core/runtime';
import type { ScenarioCase } from '../scenario';
import type { Suite } from '../vectors';
import { canonical } from '../canonical';
import {
  STAMP,
  PDF,
  GARBAGE,
  assetDoc,
  assetPage,
  bytes,
  destroy,
  engineError,
  jpeg,
  opens,
  pdf,
  png,
  settle,
  stamp,
  malformedStamp,
  start,
  str,
} from './stamp-current-fixtures';
import {
  ONE,
  TWO,
  THREE,
  SEED,
  appendDoc,
  deleteDoc,
  seedDoc,
  namedEngine,
  named,
  listenAll,
} from './stamp-mutation-fixtures';

const loose: ScenarioCase[] = [
  {
    name: 'a-raster-asset-is-sniffed-and-previews-itself',
    covers:
      'Raster dimensions size the flattened page; PNG/JPEG resources become canonical PDF assets with derived previews and metadata.',
    plugins: STAMP(),
    engine: namedEngine(
      {
        s1: seedDoc('stamp-lib-0-0', 'Logo'),
        a1: appendDoc(SEED, 'Logo', { raster: true, size: { width: 100, height: 50 } }),
        s2: seedDoc('stamp-lib-0-3', 'Photo'),
        a2: appendDoc(SEED, 'Photo', {
          raster: true,
          size: { width: 64, height: 32 },
          kind: 'signature',
          subject: 'Scan',
          categories: ['Ink'],
        }),
      },
      ['s1', 'a1', 's2', 'a2'],
    ),
    steps: [
      start,
      ...listenAll,
      stamp('createAsset', [{ name: 'Logo', source: png(100, 50) }]),
      stamp('createAsset', [
        { name: 'Photo', kind: 'signature', subject: 'Scan', categories: ['Ink'], source: jpeg() },
      ]),
      stamp('listLibraries'),
      stamp('listAssets'),
      stamp('getAssetPreview', ['stamp-lib-0-0:Logo']),
      stamp('readAssetBytes', ['stamp-lib-0-3:Photo']),
      stamp('exportLibrary', ['stamp-lib-0-0']),
    ],
  },
  {
    name: 'supplied-previews-and-sizes',
    covers:
      'An explicit raster size controls its page; supplied preview bytes and MIME type survive while rendering is skipped.',
    plugins: STAMP(),
    engine: namedEngine(
      {
        s1: seedDoc('stamp-lib-0-0', 'Sized'),
        a1: appendDoc(SEED, 'Sized', { raster: true, size: { width: 40, height: 20 } }),
        s2: seedDoc('stamp-lib-0-3', 'Bare preview'),
        a2: appendDoc(SEED, 'Bare preview', { raster: true, size: { width: 100, height: 50 } }),
        s3: seedDoc('stamp-lib-0-6', 'Empty type'),
        a3: appendDoc(SEED, 'Empty type', { raster: true, size: { width: 100, height: 50 } }),
      },
      ['s1', 'a1', 's2', 'a2', 's3', 'a3'],
    ),
    steps: [
      start,
      stamp('createAsset', [
        {
          name: 'Sized',
          source: png(10, 10),
          size: { width: 40, height: 20 },
          preview: { data: jpeg(), mimeType: 'image/jpeg' },
        },
      ]),
      stamp('createAsset', [{ name: 'Bare preview', source: png(), preview: png(8, 8) }]),
      stamp('createAsset', [
        { name: 'Empty type', source: png(), preview: { data: png(8, 8), mimeType: '' } },
      ]),
      stamp('listAssets'),
      stamp('getAssetPreview', ['stamp-lib-0-0:Sized']),
      stamp('getAssetPreview', ['stamp-lib-0-3:Bare preview']),
      stamp('getAssetPreview', ['stamp-lib-0-6:Empty type']),
    ],
  },
  {
    name: 'a-loose-pdf-needs-a-size',
    covers:
      'Current PDFs take their inserted page size without a size hint; a hint cannot override the PDF geometry. Every library is canonical.',
    plugins: STAMP(),
    engine: namedEngine(
      {
        s1: seedDoc('stamp-lib-0-0', 'Sig'),
        a1: appendDoc(SEED, 'Sig'),
        s2: seedDoc('stamp-lib-0-3', 'Sig'),
        a2: appendDoc(SEED, 'Sig'),
        s3: seedDoc('stamp-lib-0-6', 'Sig preview'),
        a3: appendDoc(SEED, 'Sig preview'),
      },
      ['s1', 'a1', 's2', 'a2', 's3', 'a3'],
    ),
    steps: [
      start,
      stamp('createAsset', [{ name: 'Sig', source: PDF }]),
      stamp('createAsset', [{ name: 'Sig', source: PDF, size: { width: 150, height: 60 } }]),
      stamp('createAsset', [
        { name: 'Sig preview', source: PDF, size: { width: 150, height: 60 }, preview: png() },
      ]),
      stamp('listAssets'),
      stamp('getAssetPreview', ['stamp-lib-0-0:Sig']),
      stamp('getAssetPreview', ['stamp-lib-0-6:Sig preview']),
    ],
  },
  {
    name: 'asset-sources-that-are-refused',
    covers:
      'Unknown library, unsupported bytes, malformed source/preview and zero-size raster reject on their exact current channels before opening a document.',
    plugins: STAMP(),
    steps: [
      start,
      stamp('createAsset', [{ libraryId: 'missing', name: 'X', source: png() }]),
      stamp('createAsset', [{ name: 'X', source: GARBAGE }]),
      stamp('createAsset', [{ name: 'X', source: new Uint8Array(3).fill(0x25) }]),
      malformedStamp('createAsset', [{ name: 'X', source: null }]),
      malformedStamp('createAsset', [
        { name: 'X', source: canonical(png()), preview: { data: null } },
      ]),
      stamp('createAsset', [{ name: 'X', source: png(0, 5) }]),
      stamp('listLibraries'),
    ],
  },
  {
    name: 'into-an-existing-loose-library',
    covers:
      'Current createAsset queues on an existing canonical library; an empty library id rejects rather than creating an obsolete loose library.',
    plugins: STAMP(),
    engine: namedEngine(
      {
        s: seedDoc('stamp-lib-0-0', 'First'),
        a: appendDoc(SEED, 'First', { raster: true, size: { width: 100, height: 50 } }),
        b: appendDoc([...SEED, assetPage(1, 900, 100, 50)], 'Second', {
          number: 901,
          namedPages: [named('First=First', 900)],
        }),
      },
      ['s', 'a', 'b'],
    ),
    steps: [
      start,
      stamp('createAsset', [{ name: 'First', source: png() }]),
      stamp('createAsset', [
        { libraryId: 'stamp-lib-0-0', name: 'Second', source: PDF, size: { width: 1, height: 2 } },
      ]),
      stamp('createAsset', [{ libraryId: '', name: 'Own library', source: png() }]),
      stamp('listLibraries'),
      stamp('listAssets', [{ libraryId: 'stamp-lib-0-0' }]),
    ],
  },
];
const canonicalCases: ScenarioCase[] = [
  {
    name: 'appends-a-page-to-a-canonical-library',
    covers:
      'Append writes the named page and PieceInfo, extracts/renders it, saves and closes before publishing its descriptor.',
    plugins: STAMP(),
    engine: namedEngine(
      {
        lib: assetDoc(ONE),
        append: appendDoc(ONE, 'Signed', {
          number: 101,
          namedPages: [named('Stamp1=Stamp1', 100)],
          kind: 'signature',
          subject: 'Customer sign-off',
          categories: ['Signature'],
        }),
      },
      ['lib', 'append'],
    ),
    steps: [
      start,
      ...listenAll,
      stamp('importLibrary', [PDF]),
      stamp('createAsset', [
        {
          libraryId: 'stamp-lib-0-1',
          name: 'Signed',
          kind: 'signature',
          subject: 'Customer sign-off',
          categories: ['Signature'],
          source: PDF,
          size: { width: 9, height: 9 },
        },
      ]),
      stamp('listAssets', [{ libraryId: 'stamp-lib-0-1' }]),
      stamp('exportLibrary', ['stamp-lib-0-1']),
      stamp('readAssetBytes', ['stamp-lib-0-1:Signed']),
      stamp('getAssetPreview', ['stamp-lib-0-1:Signed']),
    ],
  },
  {
    name: 'a-supplied-preview-skips-the-render',
    covers: 'A supplied preview skips the append render and preserves MIME type.',
    plugins: STAMP(),
    engine: namedEngine(
      {
        lib: assetDoc(ONE),
        append: appendDoc(ONE, 'Plain', { number: 101, namedPages: [named('Stamp1=Stamp1', 100)] }),
      },
      ['lib', 'append'],
    ),
    steps: [
      start,
      stamp('importLibrary', [PDF]),
      stamp('createAsset', [
        {
          libraryId: 'stamp-lib-0-1',
          name: 'Plain',
          source: PDF,
          preview: { data: png(), mimeType: 'image/webp' },
        },
      ]),
      stamp('getAssetPreview', ['stamp-lib-0-1:Plain']),
      stamp('getAsset', ['stamp-lib-0-1:Plain']),
    ],
  },
  {
    name: 'appends-that-are-refused',
    covers:
      'Multi-page insert, missing layout, PieceInfo refusal and unknown handle close without publishing; raster append is tested independently with a real successful flatten.',
    plugins: STAMP(),
    engine: namedEngine(
      {
        lib: assetDoc(ONE),
        multi: assetDoc(THREE, {
          responses: [
            {
              path: 'pages.insert',
              value: {
                insertedPages: [TWO[1].ref, THREE[2].ref],
                layout: { pageCount: 3, pages: THREE, namedPages: [named('Stamp1=Stamp1', 100)] },
                meta: {
                  affectedPages: [TWO[1].ref, THREE[2].ref],
                  cacheDelta: null,
                  opId: 'multi',
                  undoable: true,
                },
              } satisfies PageInsertResult,
            },
          ],
        }),
        unlisted: assetDoc(ONE, {
          registry: TWO,
          responses: [
            {
              path: 'pages.insert',
              value: {
                insertedPages: [TWO[1].ref],
                layout: { pageCount: 1, pages: ONE, namedPages: [] },
                meta: { affectedPages: [], cacheDelta: null, opId: 'unlisted', undoable: true },
              } satisfies PageInsertResult,
            },
          ],
        }),
        noPageInfo: appendDoc(ONE, 'No page info', {
          number: 101,
          namedPages: [named('Stamp1=Stamp1', 100)],
          responses: [
            {
              path: 'page.pieceInfo.update',
              error: engineError('Unsupported', 'page PieceInfo unsupported'),
            },
          ],
        }),
        unknown: appendDoc(ONE, 'Unknown', {
          number: 555,
          namedPages: [named('Stamp1=Stamp1', 100)],
          responses: [
            {
              path: 'page.pieceInfo.update',
              error: engineError('NotFound', 'page 555 disappeared'),
            },
          ],
        }),
      },
      ['lib', 'multi', 'unlisted', 'noPageInfo', 'unknown'],
    ),
    steps: [
      start,
      stamp('importLibrary', [PDF]),
      ...['Two pages', 'Unlisted', 'No page info', 'Unknown'].map((name) =>
        stamp('createAsset', [{ libraryId: 'stamp-lib-0-1', name, source: PDF }]),
      ),
      stamp('listAssets'),
      stamp('exportLibrary', ['stamp-lib-0-1']),
    ],
  },
  {
    name: 'an-unsaved-append-changes-nothing',
    covers:
      'A failed download closes and rolls back descriptors/resources; recovery reopens the unchanged canonical bytes.',
    plugins: STAMP(),
    engine: namedEngine(
      {
        lib: assetDoc(ONE),
        broken: appendDoc(ONE, 'Not saved', {
          number: 101,
          namedPages: [named('Stamp1=Stamp1', 100)],
          responses: [{ path: 'download', error: { name: 'Error', message: 'save failed' } }],
        }),
        append: appendDoc(ONE, 'Saved', { number: 101, namedPages: [named('Stamp1=Stamp1', 100)] }),
      },
      ['lib', 'broken', 'append'],
    ),
    steps: [
      start,
      ...listenAll,
      stamp('importLibrary', [PDF]),
      stamp('createAsset', [{ libraryId: 'stamp-lib-0-1', name: 'Not saved', source: PDF }]),
      stamp('listAssets'),
      stamp('exportLibrary', ['stamp-lib-0-1']),
      stamp('createAsset', [{ libraryId: 'stamp-lib-0-1', name: 'Saved', source: PDF }]),
      stamp('exportLibrary', ['stamp-lib-0-1']),
    ],
  },
  {
    name: 'a-save-that-answers-nothing',
    covers:
      'Deliberately malformed missing canonical/extracted bytes are kept separate from valid engine DTOs and cause exact later missing-byte refusals.',
    plugins: STAMP(),
    engine: namedEngine(
      {
        lib: assetDoc(TWO, {
          responses: [{ path: 'download' }, { path: 'pages.extract', args: [[ONE[0].ref]] }],
        }),
      },
      ['lib'],
    ),
    steps: [
      start,
      stamp('importLibrary', [PDF]),
      stamp('exportLibrary', ['stamp-lib-0-1']),
      stamp('readAssetBytes', ['stamp-lib-0-1:Stamp1']),
      stamp('createAsset', [{ libraryId: 'stamp-lib-0-1', name: 'Late', source: PDF }]),
      stamp('deleteAsset', ['stamp-lib-0-1:Stamp1']),
      stamp('listAssets'),
    ],
  },
];
const removal: ScenarioCase[] = [
  {
    name: 'removes-a-canonical-page-first',
    covers:
      'Delete rewrites canonical bytes before descriptor/resource removal; unknown asset is a no-op.',
    plugins: STAMP(),
    engine: namedEngine(
      {
        lib: assetDoc(TWO),
        remove: deleteDoc(
          TWO,
          [100],
          [],
          [named('Stamp1=Stamp1', 100), named('Stamp2=Stamp2', 101)],
        ),
      },
      ['lib', 'remove'],
    ),
    steps: [
      start,
      ...listenAll,
      stamp('importLibrary', [PDF]),
      stamp('deleteAsset', ['nope']),
      stamp('deleteAsset', ['stamp-lib-0-1:Stamp1']),
      stamp('getAsset', ['stamp-lib-0-1:Stamp1']),
      stamp('readAssetBytes', ['stamp-lib-0-1:Stamp1']),
      stamp('listAssets'),
      stamp('exportLibrary', ['stamp-lib-0-1']),
    ],
  },
  {
    name: 'removing-the-last-page-drops-the-library',
    covers:
      'Current deletion leaves the canonical library and unregistered seed; removing an imported sole page is refused by a conforming engine, preserving all resources.',
    plugins: STAMP(),
    engine: namedEngine(
      {
        lib: assetDoc(ONE),
        refused: deleteDoc(
          ONE,
          [100],
          [
            {
              path: 'pages.delete',
              error: engineError('InvalidArg', 'a document must keep at least one page'),
            },
          ],
        ),
        seed: seedDoc('stamp-lib-0-3', 'Logo'),
        add: appendDoc(SEED, 'Logo', { raster: true, size: { width: 100, height: 50 } }),
        remove: deleteDoc([...SEED, assetPage(1, 900, 100, 50)], [900]),
      },
      ['lib', 'refused', 'seed', 'add', 'remove'],
    ),
    steps: [
      start,
      stamp('importLibrary', [PDF]),
      stamp('deleteAsset', ['stamp-lib-0-1:Stamp1']),
      stamp('listLibraries'),
      stamp('exportLibrary', ['stamp-lib-0-1']),
      stamp('createAsset', [{ name: 'Logo', source: png() }]),
      stamp('deleteAsset', ['stamp-lib-0-3:Logo']),
      stamp('listLibraries'),
      stamp('readAssetBytes', ['stamp-lib-0-3:Logo']),
    ],
  },
  {
    name: 'a-failed-page-delete-keeps-the-asset',
    covers:
      'Refused delete closes without changing descriptor, source or canonical bytes; retry succeeds.',
    plugins: STAMP(),
    engine: namedEngine(
      {
        lib: assetDoc(TWO),
        refuse: deleteDoc(
          TWO,
          [100],
          [{ path: 'pages.delete', error: engineError('Refused', 'locked') }],
        ),
        retry: deleteDoc(
          TWO,
          [100],
          [],
          [named('Stamp1=Stamp1', 100), named('Stamp2=Stamp2', 101)],
        ),
      },
      ['lib', 'refuse', 'retry'],
    ),
    steps: [
      start,
      ...listenAll,
      stamp('importLibrary', [PDF]),
      stamp('deleteAsset', ['stamp-lib-0-1:Stamp1']),
      stamp('getAsset', ['stamp-lib-0-1:Stamp1']),
      stamp('readAssetBytes', ['stamp-lib-0-1:Stamp1']),
      stamp('exportLibrary', ['stamp-lib-0-1']),
      stamp('deleteAsset', ['stamp-lib-0-1:Stamp1']),
      stamp('getAsset', ['stamp-lib-0-1:Stamp1']),
    ],
  },
  {
    name: 'removals-are-serialized-per-library',
    covers:
      'One library queue waits while another proceeds; queued duplicate removals read current state and do not rewrite twice.',
    plugins: STAMP(),
    engine: {
      documents: {
        lib: assetDoc(THREE),
        other: assetDoc(TWO, { catalog: { Id: str('other') }, catalogWrites: [{ id: 'other' }] }),
        first: deleteDoc(
          THREE,
          [100],
          [],
          [named('Stamp1=Stamp1', 100), named('Stamp2=Stamp2', 101), named('Stamp3=Stamp3', 102)],
        ),
        second: deleteDoc(
          THREE.slice(1).map((page, index) => ({ ...page, index })),
          [101],
          [],
          [named('Stamp2=Stamp2', 101), named('Stamp3=Stamp3', 102)],
        ),
        otherRemove: deleteDoc(
          TWO,
          [100],
          [],
          [named('Stamp1=Stamp1', 100), named('Stamp2=Stamp2', 101)],
        ),
      },
      responses: [
        ...opens('lib', 'other'),
        { path: 'open', times: 1, defer: 'reopen' },
        ...opens('otherRemove', 'second'),
      ],
    },
    steps: [
      start,
      stamp('importLibrary', [PDF]),
      stamp('importLibrary', [PDF]),
      stamp('deleteAsset', ['stamp-lib-0-1:Stamp1']),
      stamp('deleteAsset', ['stamp-lib-0-1:Stamp2']),
      stamp('deleteAsset', ['stamp-lib-0-1:Stamp2']),
      stamp('deleteAsset', ['other:Stamp1']),
      settle('reopen', { $document: 'first' }),
      stamp('listLibraries'),
      stamp('exportLibrary', ['stamp-lib-0-1']),
      stamp('listAssets'),
    ],
  },
  {
    name: 'remove-library',
    covers:
      'Deleting canonical libraries clears all descriptors and resources; unknown and repeated deletes emit no facts.',
    plugins: STAMP(),
    engine: namedEngine({ lib: assetDoc(TWO) }, ['lib']),
    steps: [
      start,
      ...listenAll,
      stamp('importLibrary', [PDF]),
      stamp('deleteLibrary', ['nope']),
      stamp('deleteLibrary', ['stamp-lib-0-1']),
      stamp('listLibraries'),
      stamp('listAssets'),
      stamp('readAssetBytes', ['stamp-lib-0-1:Stamp1']),
      stamp('exportLibrary', ['stamp-lib-0-1']),
      stamp('deleteLibrary', ['stamp-lib-0-1']),
      stamp('listLibraries'),
    ],
  },
];
const queued: ScenarioCase[] = [
  {
    name: 'an-append-queued-behind-a-removed-library',
    covers:
      'Delete library waits for the earlier page deletion; the later append sees the removed target and rejects.',
    plugins: STAMP(),
    engine: {
      documents: {
        lib: assetDoc(TWO),
        remove: deleteDoc(
          TWO,
          [100],
          [],
          [named('Stamp1=Stamp1', 100), named('Stamp2=Stamp2', 101)],
        ),
      },
      responses: [...opens('lib'), { path: 'open', times: 1, defer: 'reopen' }],
    },
    steps: [
      start,
      stamp('importLibrary', [PDF]),
      stamp('deleteAsset', ['stamp-lib-0-1:Stamp1']),
      stamp('deleteLibrary', ['stamp-lib-0-1']),
      stamp('createAsset', [{ libraryId: 'stamp-lib-0-1', name: 'Late', source: PDF }]),
      settle('reopen', { $document: 'remove' }),
      stamp('listLibraries'),
    ],
  },
  {
    name: 'a-loose-append-queued-behind-a-removed-library',
    covers:
      'Current raster append uses the canonical queue and rejects after an earlier library deletion; no loose-library replacement exists.',
    plugins: STAMP(),
    engine: {
      documents: {
        lib: assetDoc(TWO),
        remove: deleteDoc(
          TWO,
          [100],
          [],
          [named('Stamp1=Stamp1', 100), named('Stamp2=Stamp2', 101)],
        ),
      },
      responses: [...opens('lib'), { path: 'open', times: 1, defer: 'reopen' }],
    },
    steps: [
      start,
      stamp('importLibrary', [PDF]),
      stamp('deleteAsset', ['stamp-lib-0-1:Stamp1']),
      stamp('deleteLibrary', ['stamp-lib-0-1']),
      stamp('createAsset', [{ libraryId: 'stamp-lib-0-1', name: 'Late', source: png() }]),
      settle('reopen', { $document: 'remove' }),
      stamp('listLibraries'),
      stamp('listAssets'),
    ],
  },
  {
    name: 'a-loose-append-finds-its-library-canonical',
    covers:
      'A canonical raster append reads the queue state after the first delete, uses its rewritten bytes and adds a new PDF page.',
    plugins: STAMP(),
    engine: {
      documents: {
        lib: assetDoc(TWO),
        remove: deleteDoc(
          TWO,
          [100],
          [],
          [named('Stamp1=Stamp1', 100), named('Stamp2=Stamp2', 101)],
        ),
        append: appendDoc(
          TWO.slice(1).map((page, index) => ({ ...page, index })),
          'Late',
          {
            number: 102,
            namedPages: [named('Stamp2=Stamp2', 101)],
            raster: true,
            size: { width: 100, height: 50 },
          },
        ),
      },
      responses: [...opens('lib'), { path: 'open', times: 1, defer: 'reopen' }, ...opens('append')],
    },
    steps: [
      start,
      stamp('importLibrary', [PDF]),
      stamp('deleteAsset', ['stamp-lib-0-1:Stamp1']),
      stamp('createAsset', [{ libraryId: 'stamp-lib-0-1', name: 'Late', source: png() }]),
      settle('reopen', { $document: 'remove' }),
      stamp('listLibraries'),
      stamp('listAssets'),
    ],
  },
  {
    name: 'a-loose-asset-under-a-canonical-id',
    covers:
      'Preferred library ids cannot replace live canonical ownership: createLibrary allocates a fresh id when the requested id is already taken.',
    plugins: STAMP(),
    engine: namedEngine({ lib: assetDoc(TWO), seed: seedDoc('stamp-lib-0-2', 'Logo') }, [
      'lib',
      'seed',
    ]),
    steps: [
      start,
      stamp('importLibrary', [PDF]),
      stamp('createLibrary', ['Logo', { id: 'stamp-lib-0-1' }]),
      stamp('listLibraries'),
      stamp('deleteAsset', ['stamp-lib-0-1:Logo']),
      stamp('getAsset', ['stamp-lib-0-1:Stamp1']),
    ],
  },
  {
    name: 'a-queued-mutation-after-destroy',
    covers:
      'Current queued writes can finish after destruction; instance-closed diagnostics drop state updates while cleanup clears the resource maps.',
    plugins: STAMP(),
    engine: {
      documents: {
        lib: assetDoc(THREE),
        remove: deleteDoc(
          THREE,
          [100],
          [],
          [named('Stamp1=Stamp1', 100), named('Stamp2=Stamp2', 101), named('Stamp3=Stamp3', 102)],
        ),
        late: appendDoc(
          THREE.slice(1).map((page, index) => ({ ...page, index })),
          'Late',
          {
            namedPages: [named('Stamp2=Stamp2', 101), named('Stamp3=Stamp3', 102)],
          },
        ),
      },
      responses: [...opens('lib'), { path: 'open', times: 1, defer: 'reopen' }, ...opens('late')],
    },
    snapshot: ['stamp'],
    steps: [
      start,
      stamp('importLibrary', [PDF]),
      stamp('deleteAsset', ['stamp-lib-0-1:Stamp1']),
      stamp('createAsset', [{ libraryId: 'stamp-lib-0-1', name: 'Late', source: PDF }]),
      destroy,
      settle('reopen', { $document: 'remove' }),
    ],
  },
];
export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/stamp-assets.json',
    describes:
      'packages/plugin/stamp: current canonical asset mutations, resources, queue isolation and rollback.',
    cases: [...loose, ...canonicalCases, ...removal, ...queued],
  },
];
