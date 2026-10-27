/** Current stamp public API: library edits, marks, mutations, extraction and placement. */
import type { AnnotationDraft, PageFlattenResult } from '@embedpdf/engine-core/runtime';
import type { ScenarioCase, Step } from '../scenario';
import type { Suite } from '../vectors';
import { canonical } from '../canonical';
import {
  STAMP,
  PDF,
  assetDoc,
  assetPage,
  engineError,
  opens,
  pdf,
  png,
  stamp,
  malformedStamp,
  start,
  str,
  nm,
  arr,
  settle,
} from './stamp-current-fixtures';
import {
  ONE,
  TWO,
  SEED,
  WITH_ANNOTATION,
  appendDoc,
  deleteDoc,
  seedDoc,
  mutationDoc,
  scratchDoc,
  namedEngine,
  named,
  pr,
  objRef,
  fn,
  listenAll,
  openDoc,
  write,
  created,
} from './stamp-mutation-fixtures';
import { hydrate, doc, materialize, refused } from './annotation-live-fixtures';
import { list, snapshot } from './form-current-fixtures';
import {
  stampPieceInfo,
  stampLibraryPieceInfo,
} from '../../../../packages/plugin/stamp/src/convention';
const CUSTOM = [
  '#ABCDEFGHIJKLMNOPQRSTUV',
  '#WXYZabcdefghijklmnopqr',
  '#stuvwxyz0123456789ABCD',
] as const;
const INK: AnnotationDraft = {
  subtype: 'ink',
  inkList: [
    [
      { x: 10, y: 20 },
      { x: 40, y: 5 },
    ],
    [{ x: 0, y: 30 }],
  ],
  color: '#ff8000',
  strokeWidth: 3,
};
const TEXT: AnnotationDraft = {
  subtype: 'free-text',
  intent: 'free-text',
  contents: 'Ada',
  fontFamily: 'Helvetica',
  fontSize: 36,
  textAlign: 'center',
  verticalAlign: 'middle',
  box: { x: 0, y: 0, width: 117, height: 57.6 },
  fontColor: '#1d2b53',
  strokeWidth: 0,
};
const PLACEMENT = () => STAMP({ assetEngine: 'instance', dynamic: false }, WITH_ANNOTATION);
const libraries: ScenarioCase[] = [
  {
    name: 'create-update-and-delete-libraries',
    covers:
      'Create/update/export/delete canonical libraries, filter kinds, preserve categories, emit facts and isolate a throwing listener.',
    plugins: STAMP(),
    engine: namedEngine(
      {
        a: seedDoc('stamp-lib-0-0', 'Mine'),
        s: seedDoc('sigs', 'Sigs', 'signatures', ['Legal']),
        t: seedDoc('stamp-lib-0-3', 'Taken'),
        edit: mutationDoc(SEED, {
          catalogPatches: [
            stampLibraryPieceInfo('sigs', { kind: 'signatures', categories: ['A'] }),
            stampLibraryPieceInfo('sigs', { kind: 'signatures', categories: ['B'] }),
          ],
          metadataTitles: ['Signatures'],
          download: 'edited',
        }),
      },
      ['a', 's', 't', 'edit', 'edit', 'edit'],
    ),
    steps: [
      start,
      ...listenAll,
      {
        op: 'call',
        target: { token: 'stamp' },
        method: 'onLibraryCreated',
        args: [fn('LibraryCreated.bad', { throws: { message: 'listener broke' } })],
      },
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
      'Missing canonical bytes refuse updates; invalid raster size rejects before an open; required metadata Unsupported and failed saves close with no new library.',
    plugins: STAMP(),
    engine: namedEngine(
      {
        lib: assetDoc(ONE, { responses: [{ path: 'download' }] }),
        unsupported: mutationDoc(SEED, {
          responses: [
            { path: 'metadata.update', error: engineError('Unsupported', 'metadata unsupported') },
          ],
        }),
        broken: mutationDoc(SEED, {
          metadataTitles: ['Unsaved'],
          catalogPatches: [stampLibraryPieceInfo('stamp-lib-0-4')],
          responses: [{ path: 'download', error: engineError('Io', 'disk full') }],
        }),
      },
      ['lib', 'unsupported', 'broken'],
    ),
    steps: [
      start,
      stamp('importLibrary', [PDF]),
      stamp('updateLibrary', ['stamp-lib-0-1', { name: 'Renamed' }]),
      stamp('updateAsset', ['stamp-lib-0-1:Stamp1', { label: 'Renamed' }]),
      stamp('createAsset', [
        { libraryId: 'stamp-lib-0-1', name: 'Flat', source: png(), size: { width: 0, height: 5 } },
      ]),
      stamp('createLibrary', ['No names']),
      stamp('createLibrary', ['Unsaved']),
      stamp('listLibraries'),
    ],
  },
  {
    name: 'import-a-registered-library',
    covers:
      'Registered page keys determine identifiers/labels in page order; title/kind/locale and page metadata survive, hidden registration warns once.',
    plugins: STAMP(),
    engine: namedEngine(
      {
        lib: assetDoc(TWO, {
          title: 'Goedkeuringen',
          catalog: { Id: str('nl'), Kind: nm('SignatureLibrary'), Locale: str('nl-NL') },
          catalogWrites: [{ id: 'nl', kind: 'signatures', locale: 'nl-NL' }],
          page: {
            100: { Kind: nm('Initials'), SubjectOverride: str('Paraaf'), Categories: arr('x') },
          },
          pageWrites: { 100: { kind: 'initials', subject: 'Paraaf', categories: ['x'] } },
          namedPages: [
            named('#LBG=Custom', 101),
            named('Approved=Goedgekeurd', 100),
            { name: 'Hidden', target: { kind: 'template', objectNumber: 333 } },
          ],
        }),
      },
      ['lib'],
    ),
    steps: [
      start,
      stamp('importLibrary', [PDF, { name: 'Fallback' }]),
      stamp('listLibraries'),
      stamp('listAssets'),
    ],
  },
  {
    name: 'registry-refusals',
    covers:
      'Empty/duplicate identifiers and a deliberately malformed absent registry refuse exactly; custom library kind survives successful recovery.',
    plugins: STAMP(),
    engine: namedEngine(
      {
        empty: assetDoc(ONE, { namedPages: [named('=Label', 100)] }),
        dup: assetDoc(TWO, { namedPages: [named('A=one', 100), named('A=two', 101)] }),
        old: assetDoc(ONE, {
          responses: [{ path: 'pages.list', value: { pageCount: 1, pages: ONE } }],
        }),
        custom: assetDoc(ONE, {
          catalog: { Kind: nm('toolbar') },
          catalogWrites: [{ id: 'stamp-lib-0-7', kind: 'toolbar' }],
          namedPages: [named('Only', 100)],
        }),
      },
      ['empty', 'dup', 'old', 'custom'],
    ),
    steps: [
      start,
      stamp('importLibrary', [PDF]),
      stamp('importLibrary', [PDF]),
      stamp('importLibrary', [PDF]),
      stamp('importLibrary', [PDF]),
      stamp('listLibraries'),
      stamp('listAssets'),
    ],
  },
];
const marks: ScenarioCase[] = [
  {
    name: 'marks-become-assets',
    covers:
      'Ink/free-text marks create typed annotations on scratch pages, export their appearance, then append PDF pages; image and PDF marks use current source bytes.',
    plugins: STAMP(),
    engine: namedEngine(
      {
        ink: scratchDoc(INK),
        s1: seedDoc('stamp-lib-0-1', 'Ink'),
        a1: appendDoc(SEED, 'Ink'),
        text: scratchDoc(TEXT),
        s2: seedDoc('stamp-lib-0-5', 'Typed'),
        a2: appendDoc(SEED, CUSTOM[0], { label: 'Typed' }),
        s3: seedDoc('stamp-lib-0-8', 'Picture'),
        a3: appendDoc(SEED, 'Picture', { raster: true, size: { width: 20, height: 10 } }),
        s4: seedDoc('stamp-lib-0-b', 'Whole'),
        a4: appendDoc(SEED, 'Whole'),
        source: assetDoc(TWO),
        s5: seedDoc('stamp-lib-0-f', 'Second page'),
        a5: appendDoc(SEED, 'Second page'),
      },
      ['ink', 's1', 'a1', 'text', 's2', 'a2', 's3', 'a3', 's4', 'a4', 'source', 's5', 'a5'],
    ),
    steps: [
      start,
      stamp('createAsset', [
        {
          name: 'Ink',
          mark: {
            kind: 'ink',
            strokes: [
              [
                { x: 10, y: 20 },
                { x: 40, y: 5 },
              ],
              [{ x: 0, y: 30 }],
            ],
            color: '#ff8000',
            strokeWidth: 3,
          },
        },
      ]),
      stamp('createAsset', [
        { label: 'Typed', mark: { kind: 'text', text: 'Ada', fontFamily: 'Helvetica' } },
      ]),
      stamp('createAsset', [{ name: 'Picture', mark: { kind: 'image', source: png(20, 10) } }]),
      stamp('createAsset', [{ name: 'Whole', mark: { kind: 'pdf', source: PDF } }]),
      stamp('createAsset', [
        { name: 'Second page', mark: { kind: 'pdf', source: PDF, pageIndex: 1 } },
      ]),
      stamp('listAssets'),
    ],
  },
  {
    name: 'marks-that-are-refused',
    covers:
      'Conflicting/missing source, malformed colors, empty ink, missing PDF pages and invalid identifiers refuse without adding assets; every scratch/source document closes.',
    plugins: STAMP(),
    engine: namedEngine(
      {
        a: mutationDoc(SEED),
        b: mutationDoc(SEED),
        c: assetDoc(ONE),
        d: mutationDoc(SEED),
        e: mutationDoc(SEED),
        f: assetDoc(ONE),
      },
      ['a', 'b', 'c', 'd', 'e', 'f'],
    ),
    steps: [
      start,
      stamp('createAsset', [{ name: 'X', source: png(), mark: { kind: 'image', source: png() } }]),
      stamp('createAsset', [{ name: 'X' }]),
      stamp('createAsset', [
        { name: 'X', mark: { kind: 'text', text: 'x', fontFamily: 'Helvetica', color: 'red' } },
      ]),
      stamp('createAsset', [{ name: 'X', mark: { kind: 'ink', strokes: [[]] } }]),
      stamp('createAsset', [{ name: 'X', mark: { kind: 'pdf', source: PDF, pageIndex: 4 } }]),
      stamp('createAsset', [{ name: '', source: png() }]),
      stamp('createAsset', [{ name: 'a=b', source: png() }]),
      stamp('createAsset', [
        { name: 'X', mark: { kind: 'text', text: 'x', fontFamily: 'Helvetica', color: '#12345' } },
      ]),
      stamp('createAsset', [
        { name: 'X', mark: { kind: 'text', text: 'x', fontFamily: 'Helvetica', color: '#zzzzzz' } },
      ]),
      stamp('createAsset', [{ name: 'X', mark: { kind: 'pdf', source: PDF, pageIndex: -1 } }]),
      stamp('listLibraries'),
    ],
  },
  {
    name: 'an-ink-mark-with-defaults',
    covers:
      'Ink defaults reach the real engine draft; an empty scratch document closes and rejects without creating a library.',
    plugins: STAMP(),
    engine: namedEngine(
      {
        ink: scratchDoc({
          subtype: 'ink',
          inkList: [[{ x: 1, y: 2 }]],
          color: '#1d2b53',
          strokeWidth: 2,
        }),
        seed: seedDoc('stamp-lib-0-1', 'Plain ink'),
        append: appendDoc(SEED, 'Plain ink'),
        blank: {
          local: true,
          pages: [],
          responses: [{ path: 'pages.list', value: { pageCount: 0, pages: [], namedPages: [] } }],
        },
      },
      ['ink', 'seed', 'append', 'blank'],
    ),
    steps: [
      start,
      stamp('createAsset', [
        { name: 'Plain ink', mark: { kind: 'ink', strokes: [[{ x: 1, y: 2 }]] } },
      ]),
      stamp('createAsset', [
        { name: 'Nowhere', mark: { kind: 'text', text: 'x', fontFamily: 'Helvetica' } },
      ]),
      stamp('listAssets'),
    ],
  },
];
const assets: ScenarioCase[] = [
  {
    name: 'edit-move-and-copy-assets',
    covers:
      'Asset updates preserve identity and merge metadata; duplicate/move use derived new ids and preserve bytes, previews and metadata; same-library move is an identity read.',
    plugins: STAMP(),
    engine: namedEngine(
      {
        lib: assetDoc(ONE, {
          page: { 100: { SubjectOverride: str('Keep'), Categories: arr('c') } },
          pageWrites: { 100: { kind: 'stamp', subject: 'Keep', categories: ['c'] } },
        }),
        e1: mutationDoc(ONE, {
          page: {
            100: {
              Version: { type: 'number', value: 2 },
              Kind: nm('Stamp'),
              SubjectOverride: str('Keep'),
              Categories: arr('c'),
            },
          },
          namedPages: [named('Stamp1=Stamp1', 100)],
          names: [{ name: 'Stamp1=Relabelled', page: pr(100), replace: 'Stamp1=Stamp1' }],
          pagePatches: [
            { page: 100, patch: stampPieceInfo('stamp', { subject: 'New', categories: ['d'] }) },
          ],
          download: 'edited',
        }),
        e2: mutationDoc(ONE, {
          namedPages: [named('Stamp1=Relabelled', 100)],
          page: {
            100: {
              Version: { type: 'number', value: 2 },
              Kind: nm('Stamp'),
              SubjectOverride: str('New'),
              Categories: arr('d'),
            },
          },
          pagePatches: [{ page: 100, patch: stampPieceInfo('stamp', { categories: ['d'] }) }],
          download: 'edited',
        }),
        d1: appendDoc(ONE, CUSTOM[0], {
          namedPages: [named('Stamp1=Relabelled', 100)],
          label: 'Relabelled copy',
          categories: ['d'],
        }),
        d2: appendDoc([...ONE, assetPage(1, 900, 300, 120)], CUSTOM[1], {
          number: 901,
          namedPages: [named(CUSTOM[0] + '=Relabelled copy', 900), named('Stamp1=Relabelled', 100)],
          label: 'Named copy',
          categories: ['d'],
        }),
        other: seedDoc('other', 'Other'),
        move: appendDoc(SEED, CUSTOM[2], { label: 'Relabelled', categories: ['d'] }),
        remove: deleteDoc(
          [...ONE, assetPage(1, 900, 300, 120), assetPage(2, 901, 300, 120)],
          [100],
          [],
          [
            named(CUSTOM[0] + '=Relabelled copy', 900),
            named(CUSTOM[1] + '=Named copy', 901),
            named('Stamp1=Relabelled', 100),
          ],
        ),
        dup: appendDoc([...SEED, assetPage(1, 900, 300, 120)], 'Dup', {
          number: 901,
          namedPages: [named(CUSTOM[2] + '=Relabelled', 900)],
        }),
      },
      ['lib', 'e1', 'e2', 'd1', 'd2', 'other', 'move', 'remove', 'dup'],
    ),
    steps: [
      start,
      ...listenAll,
      stamp('importLibrary', [PDF]),
      stamp('updateAsset', [
        'stamp-lib-0-1:Stamp1',
        { label: 'Relabelled', subject: 'New', categories: ['d'] },
      ]),
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
      'Asset and library edits queued after their deletions reject not-found when their queue turn arrives.',
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
      stamp('updateAsset', ['stamp-lib-0-1:Stamp1', { label: 'Late' }]),
      stamp('deleteLibrary', ['stamp-lib-0-1']),
      stamp('updateLibrary', ['stamp-lib-0-1', { name: 'Late' }]),
      settle('reopen', { $document: 'remove' }),
      stamp('listLibraries'),
    ],
  },
  {
    name: 'copies-carry-their-metadata',
    covers:
      'Copies and cross-library moves retain subject/categories/preview and remove the original only after the new asset commits.',
    plugins: STAMP(),
    engine: namedEngine(
      {
        lib: assetDoc(TWO, {
          page: { 101: { SubjectOverride: str('Subj'), Categories: arr('k') } },
          pageWrites: { 101: { kind: 'stamp', subject: 'Subj', categories: ['k'] } },
        }),
        d1: appendDoc(TWO, CUSTOM[0], {
          namedPages: [named('Stamp1=Stamp1', 100), named('Stamp2=Stamp2', 101)],
          label: 'Stamp2 copy',
          subject: 'Subj',
          categories: ['k'],
        }),
        d2: appendDoc([...TWO, assetPage(2, 900, 300, 120)], CUSTOM[1], {
          number: 901,
          namedPages: [
            named(CUSTOM[0] + '=Stamp2 copy', 900),
            named('Stamp1=Stamp1', 100),
            named('Stamp2=Stamp2', 101),
          ],
          label: 'Stamp1 copy',
        }),
        other: seedDoc('other', 'Other'),
        move: appendDoc(SEED, CUSTOM[2], { label: 'Stamp2', subject: 'Subj', categories: ['k'] }),
        remove: deleteDoc(
          [...TWO, assetPage(2, 900, 300, 120), assetPage(3, 901, 300, 120)],
          [101],
          [],
          [
            named(CUSTOM[0] + '=Stamp2 copy', 900),
            named(CUSTOM[1] + '=Stamp1 copy', 901),
            named('Stamp1=Stamp1', 100),
            named('Stamp2=Stamp2', 101),
          ],
        ),
      },
      ['lib', 'd1', 'd2', 'other', 'move', 'remove'],
    ),
    steps: [
      start,
      stamp('importLibrary', [PDF]),
      stamp('duplicateAsset', ['stamp-lib-0-1:Stamp2']),
      stamp('duplicateAsset', ['stamp-lib-0-1:Stamp1']),
      stamp('createLibrary', ['Other', { id: 'other' }]),
      stamp('moveAsset', ['stamp-lib-0-1:Stamp2', { libraryId: 'other' }]),
      stamp('listAssets'),
    ],
  },
  {
    name: 'raster-pages-that-fail',
    covers:
      'A valid current unchanged flatten status is refused by stamp; required engine flatten Unsupported also closes with no asset, and a later successful raster append recovers.',
    plugins: STAMP(),
    engine: namedEngine(
      {
        seed: seedDoc('r', 'Raster'),
        flat: appendDoc(SEED, 'A', {
          raster: true,
          size: { width: 100, height: 50 },
          responses: [
            {
              path: 'pages.flatten',
              value: {
                pages: [pr(900)],
                usage: 'display',
                results: [{ page: pr(900), status: 'unchanged' }],
                meta: { affectedPages: [pr(900)], cacheDelta: null, opId: 'flat', undoable: true },
              } satisfies PageFlattenResult,
            },
          ],
        }),
        noFlatten: appendDoc(SEED, 'B', {
          raster: true,
          size: { width: 100, height: 50 },
          responses: [
            { path: 'pages.flatten', error: engineError('Unsupported', 'flatten unsupported') },
          ],
        }),
        ok: appendDoc(SEED, 'C', { raster: true, size: { width: 100, height: 50 } }),
      },
      ['seed', 'flat', 'noFlatten', 'ok'],
    ),
    steps: [
      start,
      stamp('createLibrary', ['Raster', { id: 'r' }]),
      stamp('createAsset', [{ libraryId: 'r', name: 'A', source: png() }]),
      stamp('createAsset', [{ libraryId: 'r', name: 'B', source: png() }]),
      stamp('listAssets'),
      stamp('createAsset', [{ libraryId: 'r', name: 'C', source: png() }]),
      stamp('listAssets'),
    ],
  },
  {
    name: 'an-asset-from-annotations',
    covers:
      'Current extraction resolves the real page and download grant before exporting selected appearances; invalid source/mark, closed document and refused egress cannot add assets.',
    plugins: PLACEMENT(),
    engine: namedEngine(
      {
        a: doc(
          [
            hydrate([
              materialize(7, { subtype: 'stamp', box: { x: 0, y: 0, width: 30, height: 20 } }),
            ]),
            list(snapshot([])),
            {
              path: 'page.annotations.exportAppearance',
              pon: 1,
              args: [[objRef(7)]],
              value: canonical(pdf('selection')),
            },
          ],
          { pages: [assetPage(0, 1, 600, 800)] },
        ),
        denied: doc([], { security: { allows: [] } }),
        seed: seedDoc('stamp-lib-0-0', 'Selection'),
        append: appendDoc(SEED, 'Selection', { kind: 'signature' }),
      },
      ['a', 'seed', 'append', 'denied'],
    ),
    steps: [
      start,
      openDoc(),
      stamp('createAssetFromAnnotations', [
        pr(1),
        [objRef(7)],
        { name: 'Selection', kind: 'signature' },
        { documentId: 'a' },
      ]),
      malformedStamp('createAssetFromAnnotations', [
        pr(1),
        [objRef(7)],
        { name: 'Both', mark: { kind: 'image', source: canonical(png()) } },
        { documentId: 'a' },
      ]),
      stamp('createAssetFromAnnotations', [
        pr(1),
        [objRef(7)],
        { name: 'Nowhere' },
        { documentId: 'closed' },
      ]),
      stamp('canCreateFromAnnotations', ['a']),
      stamp('createAssetFromAnnotations', [
        pr(999),
        [objRef(7)],
        { name: 'Wrong page' },
        { documentId: 'a' },
      ]),
      openDoc('denied'),
      stamp('canCreateFromAnnotations', ['denied']),
      stamp('createAssetFromAnnotations', [
        pr(1),
        [objRef(7)],
        { name: 'Denied' },
        { documentId: 'denied' },
      ]),
      stamp('listAssets'),
    ],
  },
];
const previews: ScenarioCase[] = [
  {
    name: 'rendered-previews',
    covers:
      'Preview widths cache independently, failure reports and falls back, empty source page returns null; deleting the asset clears its renders.',
    plugins: STAMP(),
    engine: namedEngine(
      {
        lib: assetDoc(TWO),
        ghost: assetDoc([assetPage(0, 5, 50, 20)]),
        broken: assetDoc([assetPage(0, 5, 50, 20)], {
          responses: [{ path: 'page.render.image', error: engineError('Io', 'render failed') }],
        }),
        empty: {
          local: true,
          pages: [],
          responses: [{ path: 'pages.list', value: { pageCount: 0, pages: [], namedPages: [] } }],
        },
        nomime: assetDoc([assetPage(0, 6, 50, 20)], {
          responses: [
            {
              path: 'page.render.image',
              value: canonical({
                width: 50,
                height: 20,
                format: 'png',
                source: { kind: 'bytes', bytes: pdf('no-type') },
              }),
            },
          ],
        }),
        remove: deleteDoc(
          TWO,
          [100],
          [],
          [named('Stamp1=Stamp1', 100), named('Stamp2=Stamp2', 101)],
        ),
      },
      ['lib', 'ghost', 'broken', 'empty', 'nomime', 'remove'],
    ),
    steps: [
      start,
      stamp('importLibrary', [PDF]),
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
const placed = materialize(10001, {
  subtype: 'stamp',
  box: { x: 75, y: 87.5, width: 50, height: 25 },
  name: 'Stamp1',
  subject: 'Stamp1',
  fit: 'contain',
});
const placed2 = materialize(10002, {
  subtype: 'stamp',
  box: { x: 0, y: 0, width: 200, height: 100 },
  name: 'Stamp1',
  subject: 'Stamp1',
  fit: 'contain',
});
const placed3 = materialize(
  10004,
  {
    subtype: 'stamp',
    box: { x: -50, y: 50, width: 200, height: 100 },
    rotation: 90,
    name: 'Stamp1',
    subject: 'Stamp1',
    fit: 'contain',
  },
  2,
);
const placement: ScenarioCase[] = [
  {
    name: 'place-without-the-pointer',
    covers:
      'Programmatic placement uses current annotation operations/resources; per-page failure appears in the batch and successful later placement recovers.',
    plugins: PLACEMENT(),
    engine: namedEngine(
      {
        a: doc(
          [
            hydrate(),
            list(snapshot([])),
            write(1, [{ item: created(placed) }]),
            write(2, [{ item: created(placed2) }]),
            refused('page locked'),
            write(4, [{ item: created(placed3) }]),
          ],
          { pages: [assetPage(0, 1, 600, 800), assetPage(1, 2, 600, 800)] },
        ),
        lib: assetDoc(ONE),
      },
      ['a', 'lib'],
    ),
    steps: [
      start,
      openDoc(),
      {
        op: 'call',
        target: { token: 'annotation', doc: 'a' },
        method: 'onCreated',
        args: [fn('annotationCreated')],
      },
      {
        op: 'call',
        target: { token: 'annotation', doc: 'a' },
        method: 'onWriteFailed',
        args: [fn('annotationWriteFailed')],
      },
      stamp('importLibrary', [PDF]),
      stamp('placeAsset', [
        'stamp-lib-0-1:Stamp1',
        { page: pr(1), center: { x: 100, y: 100 }, targetWidth: 50 },
        { documentId: 'a' },
      ]),
      stamp('placeAsset', ['nope', { page: pr(1), center: { x: 1, y: 1 } }, { documentId: 'a' }]),
      stamp('placeAssetOnPages', [
        'stamp-lib-0-1:Stamp1',
        'all',
        { center: { x: 10, y: 10 } },
        { documentId: 'a' },
      ]),
      stamp('placeAssetOnPages', [
        'stamp-lib-0-1:Stamp1',
        [pr(2)],
        { center: { x: 20, y: 20 }, rotation: 90 },
        { documentId: 'a' },
      ]),
      stamp('placeAssetOnPages', [
        'stamp-lib-0-1:Stamp1',
        'all',
        { center: { x: 1, y: 1 } },
        { documentId: 'closed' },
      ]),
      { op: 'call', target: { token: 'annotation', doc: 'a' }, method: 'list', args: [] },
      stamp('placeAssetOnPages', [
        'stamp-lib-0-1:Stamp1',
        [pr(1), pr(999)],
        { center: { x: 10, y: 10 } },
        { documentId: 'a' },
      ]),
    ],
  },
  {
    name: 'the-armed-asset',
    covers:
      'Armed read follows the shipping annotation stamp tool; tool changes disarm once and permission getters resolve explicit/active/closed documents.',
    plugins: PLACEMENT(),
    engine: namedEngine({ a: doc([hydrate(), list(snapshot([]))]), lib: assetDoc(ONE) }, [
      'a',
      'lib',
    ]),
    steps: [
      start,
      openDoc(),
      { op: 'call', target: { token: 'stamp' }, method: 'onArmChanged', args: [fn('armChanged')] },
      stamp('importLibrary', [PDF]),
      stamp('getArmedAsset', ['a']),
      stamp('armAsset', ['stamp-lib-0-1:Stamp1', { documentId: 'a' }]),
      stamp('getArmedAsset', ['a']),
      {
        op: 'call',
        target: { token: 'interaction', doc: 'a' },
        method: 'activateTool',
        args: ['pan'],
      },
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
    covers: 'No active document means placement and annotation extraction grants are false.',
    plugins: STAMP(),
    steps: [start, stamp('canPlace'), stamp('canCreateFromAnnotations')],
  },
];
const persist = (method: string, args: unknown[] = []): Step => ({
  op: 'call',
  target: { token: 'stamp-persistence' },
  method,
  args: args.map((arg) => canonical(arg)),
});
const PERSIST = (config?: unknown) => [
  ...STAMP(),
  { factory: 'stamp-current-persistence', ...(config ? { config } : {}) },
];
const persistence: ScenarioCase[] = [
  {
    name: 'restore-stored-libraries',
    covers: 'Stored libraries import file-owned ids; invalid stored bytes warn and are skipped.',
    plugins: PERSIST(),
    engine: namedEngine({ lib: assetDoc(ONE, { title: 'Kept' }) }, ['lib']),
    steps: [
      start,
      persist('seed', [
        [
          { id: 'kept', bytes: PDF },
          { id: 'junk', bytes: png() },
        ],
      ]),
      persist('restore'),
      stamp('listLibraries'),
    ],
  },
  {
    name: 'a-store-that-cannot-list',
    covers: 'A store list rejection rejects restore unchanged.',
    plugins: PERSIST({ fail: { list: 'offline' } }),
    steps: [start, persist('restore')],
  },
  {
    name: 'persist-library-changes',
    covers:
      'Current persistence saves after each burst, deletes removed libraries, honors exclusions and stops observing after unsubscribe; debounce timers were removed.',
    plugins: PERSIST(),
    engine: namedEngine(
      {
        a: seedDoc('a', 'A'),
        skip: seedDoc('skip', 'Skip'),
        edit: mutationDoc(SEED, {
          metadataTitles: ['A2'],
          catalogPatches: [stampLibraryPieceInfo('a')],
          download: 'A2',
        }),
        b: seedDoc('b', 'B'),
        c: seedDoc('c', 'C'),
      },
      ['a', 'skip', 'edit', 'b', 'c'],
    ),
    steps: [
      start,
      persist('persist', [{ except: ['skip'] }]),
      stamp('createLibrary', ['A', { id: 'a' }]),
      stamp('createLibrary', ['Skip', { id: 'skip' }]),
      stamp('updateLibrary', ['a', { name: 'A2' }]),
      persist('rows'),
      stamp('createLibrary', ['B', { id: 'b' }]),
      stamp('deleteLibrary', ['a']),
      persist('rows'),
      persist('stop'),
      stamp('createLibrary', ['C', { id: 'c' }]),
      persist('rows'),
    ],
  },
  {
    name: 'stop-flushes-a-write-still-pending-behind-another',
    covers:
      'Current unsubscribe lets already requested work finish; it prevents later library changes from being persisted.',
    plugins: [...STAMP(), { factory: 'stamp-mutation-persistence' }],
    engine: namedEngine({ a: seedDoc('a', 'A'), b: seedDoc('b', 'B'), c: seedDoc('c', 'C') }, [
      'a',
      'b',
      'c',
    ]),
    steps: [
      start,
      persist('persist'),
      stamp('createLibrary', ['A', { id: 'a' }]),
      stamp('createLibrary', ['B', { id: 'b' }]),
      persist('rows'),
      persist('stop'),
      settle('put:a'),
      persist('rows'),
      stamp('createLibrary', ['C', { id: 'c' }]),
      persist('rows'),
    ],
  },
  {
    name: 'persistence-failures-are-reported',
    covers: 'Failed store put/delete warn with exact library id; subsequent work remains live.',
    plugins: PERSIST({ fail: { put: 'quota exceeded', delete: 'locked' } }),
    engine: namedEngine({ a: seedDoc('a', 'A'), b: seedDoc('b', 'B') }, ['a', 'b']),
    steps: [
      start,
      persist('persist'),
      stamp('createLibrary', ['A', { id: 'a' }]),
      stamp('deleteLibrary', ['a']),
      stamp('createLibrary', ['B', { id: 'b' }]),
      persist('rows'),
    ],
  },
];
export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/stamp-api.json',
    describes: 'packages/plugin/stamp: current mutation APIs, marks, placement and persistence.',
    cases: [...libraries, ...marks, ...assets, ...previews, ...placement, ...persistence],
  },
];
