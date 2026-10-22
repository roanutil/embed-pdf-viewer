/**
 * `packages/plugin/stamp`: importing a PDF as a canonical stamp library.
 * A typed local asset port forwards current engine calls to static answers. Replayed by
 * `crates/plugins/stamp/tests/conformance.rs`.
 */
import type { ScenarioCase } from '../scenario';
import type { Suite } from '../vectors';

import {
  GARBAGE,
  PDF,
  bytes,
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
  malformedStamp,
  importWithAssetName,
} from './stamp-current-fixtures';

const TWO = [assetPage(0, 100), assetPage(1, 101)];
const ONE = [assetPage(0, 100)];

const importing: ScenarioCase[] = [
  {
    name: 'imports-every-page-as-a-vector-asset',
    covers:
      'importLibrary opens source bytes on the configured local asset engine with wildcard scope, reads current catalog/page PieceInfo and metadata, registers plain PDF pages, rewrites normalized metadata, extracts source PDFs and inline previews, saves canonical bytes and closes before committing current library/asset descriptors. Current selectors return ordered descriptors, independent binary copies and null for absent cached reads; exporting an absent library rejects.',
    plugins: STAMP(),
    engine: {
      documents: { lib: assetDoc(TWO, { titleAfterWrite: 'Approvals' }) },
      responses: opens('lib'),
    },
    steps: [
      start,
      stamp('importLibrary', [PDF, { name: 'Approvals' }]),
      stamp('listLibraries'),
      stamp('getLibrary', ['stamp-lib-0-1']),
      stamp('getLibrary', ['nope']),
      stamp('listAssets', [{ libraryId: 'stamp-lib-0-1' }]),
      stamp('listAssets'),
      stamp('listAssets', [{ libraryId: 'nope' }]),
      stamp('listAssets', [{ libraryId: '' }]),
      stamp('getAsset', ['stamp-lib-0-1:Stamp1']),
      stamp('getAsset', ['nope']),
      stamp('readAssetBytes', ['stamp-lib-0-1:Stamp2']),
      stamp('readAssetBytes', ['nope']),
      stamp('getAssetPreview', ['stamp-lib-0-1:Stamp1']),
      stamp('getAssetPreview', ['nope']),
      stamp('exportLibrary', ['stamp-lib-0-1']),
      stamp('exportLibrary', ['nope']),
    ],
  },
  {
    name: 'reads-ids-and-display-metadata-from-pieceinfo',
    covers:
      'Catalog Id, fallback Name and Categories survive. A plain PDF uses page Name as identifier and Subject as label; current kind uses a case-insensitive PDF name, and Categories requires string-array. Empty/unknown/wrong-tag values fall back. Page PieceInfo Id no longer owns asset identity; ids derive from library and identifier.',
    plugins: STAMP(),
    engine: {
      documents: {
        lib: assetDoc(
          [assetPage(0, 100), assetPage(1, 101), assetPage(2, 102), assetPage(3, 103)],
          {
            titleAfterWrite: 'Review',
            pageNames: [
              'Approved=Approval signature',
              'Stamp2=Stamp2',
              'Stamp3=Stamp3',
              'Stamp4=Stamp4',
            ],
            catalogWrites: [{ id: 'review-library', categories: ['Team', 'Legal'] }],
            pageWrites: {
              100: { kind: 'signature', categories: ['Review'] },
              101: { kind: 'initials' },
            },
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
              101: {
                Id: str(''),
                Name: str(''),
                Kind: nm('INITIALS'),
                Subject: str(''),
                Categories: str('Review'),
              },
              102: { Kind: str('Signature'), Name: nm('Not a string') },
              103: { Kind: nm('Watermark') },
            },
          },
        ),
      },
      responses: opens('lib'),
    },
    steps: [start, stamp('importLibrary', [PDF]), stamp('listLibraries'), stamp('listAssets')],
  },
  {
    name: 'a-name-kind-and-stamp-kinds-read-back',
    covers:
      'Mixed-case Stamp maps to stamp; empty kind falls back to stamp; an empty embedded library name falls back to Stamps.',
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
    steps: [start, stamp('importLibrary', [PDF]), stamp('listLibraries'), stamp('listAssets')],
  },
  {
    name: 'import-options-override-the-pdf',
    covers:
      'Caller categories and asset kind override PDF metadata, including empty categories; the existing PDF name wins over the caller name fallback. Current assetName(index) supplies a label, with typed undefined responses falling back to the identifier.',
    plugins: STAMP(),
    engine: {
      documents: {
        lib: assetDoc([assetPage(0, 100), assetPage(1, 101), assetPage(2, 102)], {
          titleAfterWrite: 'Embedded',
          pageNames: ['Stamp1=First', 'From PDF=From PDF', 'Stamp3=Stamp3'],
          catalogWrites: [{ id: 'stamp-lib-0-1', categories: [] }],
          pageWrites: {
            100: { kind: 'initials' },
            101: { kind: 'initials' },
            102: { kind: 'initials' },
          },
          catalog: { Name: str('Embedded'), Categories: arr('Embedded') },
          page: { 101: { Name: str('From PDF'), Kind: nm('Signature') } },
        }),
      },
      responses: opens('lib'),
    },
    steps: [
      start,
      importWithAssetName(PDF, {
        name: '',
        categories: [],
        kind: 'initials',
        assetName: { $fn: 'assetName', seq: [{ returns: 'First' }, {}, {}] },
      }),
      stamp('listLibraries'),
      stamp('listAssets'),
    ],
  },
  {
    name: 'a-payload-source-and-its-errors',
    covers:
      'Current BinarySource payload bytes import successfully. PNG, JPEG and unknown bytes reject invalid-input before any engine call. Explicit malformed null source and numeric payload data reject exact operation-failed PluginErrors with source-owned TypeError causes, then a valid payload recovers.',
    plugins: STAMP(),
    engine: { documents: { lib: assetDoc(ONE) }, responses: opens('lib') },
    steps: [
      start,
      stamp('importLibrary', [png()]),
      stamp('importLibrary', [jpeg()]),
      stamp('importLibrary', [GARBAGE]),
      malformedStamp('importLibrary', [null]),
      malformedStamp('importLibrary', [{ data: 5 }]),
      stamp('importLibrary', [{ data: PDF, mimeType: 'application/pdf', name: 'lib.pdf' }]),
      stamp('listLibraries'),
    ],
  },
];

const failures: ScenarioCase[] = [
  {
    name: 'a-cloud-kernel-engine-names-the-fix',
    covers:
      'An unconfigured remote kernel engine refuses before open with an actionable unsupported PluginError naming the local assetEngine option. canImport changes from optimistic true to false; repeated calls retain the same exact refusal.',
    plugins: STAMP({ assetEngine: null }),

    steps: [
      start,
      stamp('canImport'),
      stamp('importLibrary', [PDF]),
      stamp('canImport'),
      stamp('importLibrary', [PDF]),
      stamp('importLibrary', [PDF]),
      stamp('listLibraries'),
    ],
  },
  {
    name: 'a-configured-engine-keeps-its-invalid-arg',
    covers:
      'A configured local asset engine InvalidArg is wrapped in the current invalid-input PluginError with the exact EngineError cause; it does not become the remote-engine configuration hint.',
    plugins: STAMP({ assetEngine: 'instance' }),
    engine: { responses: [{ path: 'open', error: engineError('InvalidArg', 'no bytes here') }] },
    steps: [start, stamp('importLibrary', [PDF])],
  },
  {
    name: 'missing-services-refuse-before-any-write',
    covers:
      'Required local catalog/page PieceInfo services reject with explicit engine Unsupported answers; these are service refusals, not absent-service coverage. Empty layouts and a listed page absent from the page registry reject their current exact errors. Every opened document closes and no state commits.',
    plugins: STAMP(),
    engine: {
      documents: {
        bare: assetDoc(ONE, {
          responses: [
            {
              path: 'pieceInfo.get',
              error: engineError('Unsupported', 'catalog piece info unsupported'),
            },
          ],
        }),
        catalogOnly: assetDoc(ONE, {
          responses: [
            {
              path: 'page.pieceInfo.get',
              error: engineError('Unsupported', 'page piece info unsupported'),
            },
          ],
        }),
        empty: assetDoc([]),
        unknownPage: assetDoc(ONE, { registry: [assetPage(0, 7)] }),
      },
      responses: opens('bare', 'catalogOnly', 'empty', 'unknownPage'),
    },
    steps: [
      start,
      stamp('importLibrary', [PDF]),
      stamp('importLibrary', [PDF]),
      stamp('importLibrary', [PDF]),
      stamp('importLibrary', [PDF]),
      stamp('listLibraries'),
    ],
  },
  {
    name: 'an-engine-failure-mid-import-adds-nothing',
    covers:
      'Actual catalog/page metadata write, extraction, render, save and page PieceInfo read failures are wrapped as exact PluginErrors, close their document and commit no library, asset or bytes.',
    plugins: STAMP(),
    engine: {
      documents: {
        update: assetDoc(ONE, {
          responses: [
            { path: 'pieceInfo.update', error: engineError('Refused', 'catalog is read-only') },
          ],
        }),
        pageUpdate: assetDoc(ONE, {
          catalogWrites: [{ id: 'stamp-lib-0-3' }],
          responses: [
            { path: 'page.pieceInfo.update', error: engineError('Refused', 'page is read-only') },
          ],
        }),
        extract: assetDoc(ONE, {
          catalogWrites: [{ id: 'stamp-lib-0-5' }],
          responses: [{ path: 'pages.extract', error: engineError('Unknown', 'extract failed') }],
        }),
        render: assetDoc(ONE, {
          catalogWrites: [{ id: 'stamp-lib-0-7' }],
          responses: [
            { path: 'page.render.image', error: engineError('Unknown', 'render failed') },
          ],
        }),
        save: assetDoc(ONE, {
          catalogWrites: [{ id: 'stamp-lib-0-9' }],
          responses: [{ path: 'download', error: { name: 'Error', message: 'save failed' } }],
        }),
        read: assetDoc(ONE, {
          responses: [{ path: 'page.pieceInfo.get', error: engineError('Unknown', 'read failed') }],
        }),
      },
      responses: opens('update', 'pageUpdate', 'extract', 'render', 'save', 'read'),
    },
    steps: [
      start,
      stamp('importLibrary', [PDF]),
      stamp('importLibrary', [PDF]),
      stamp('importLibrary', [PDF]),
      stamp('importLibrary', [PDF]),
      stamp('importLibrary', [PDF]),
      stamp('importLibrary', [PDF]),
      stamp('listLibraries'),
      stamp('listAssets'),
    ],
  },
  {
    name: 'a-failed-close-fails-the-import',
    covers:
      'A finally close failure rejects after successful download and prevents every descriptor/binary commit.',
    plugins: STAMP(),
    engine: {
      documents: {
        lib: assetDoc(ONE, {
          responses: [{ path: 'close', error: engineError('Unknown', 'close failed') }],
        }),
      },
      responses: opens('lib'),
    },
    steps: [start, stamp('importLibrary', [PDF]), stamp('listLibraries')],
  },
  {
    name: 'previews-must-be-inline-bytes',
    covers:
      'A valid URL-sourced image is unsupported on the local asset port. An explicitly malformed image missing contentType still caches its byte source with absent MIME. previewWidth sets the actual render request width.',
    plugins: STAMP({ assetEngine: 'instance', previewWidth: 96 }),
    engine: {
      documents: {
        url: assetDoc(ONE, {
          responses: [
            {
              path: 'page.render.image',
              value: { ...image(100), source: { kind: 'url', url: 'https://x/p.png' } },
            },
          ],
        }),
        untyped: assetDoc(ONE, {
          catalogWrites: [{ id: 'stamp-lib-0-3' }],
          responses: [
            {
              path: 'page.render.image',
              value: {
                ...image(100),
                contentType: undefined,
                source: {
                  kind: 'bytes',
                  bytes: { $bytes: Buffer.from(bytes('png-100')).toString('base64') },
                },
              },
            },
          ],
        }),
      },
      responses: opens('url', 'untyped'),
    },
    steps: [
      start,
      stamp('importLibrary', [PDF]),
      stamp('importLibrary', [PDF]),
      stamp('getAssetPreview', ['stamp-lib-0-3:Stamp1']),
    ],
  },
];

const blankRenders: ScenarioCase[] = [
  {
    name: 'a-render-with-no-image-throws',
    covers:
      'Explicit malformed null PageImageResult and null source reject with exact source/kind TypeError causes wrapped by the current stamp verb, close and commit nothing.',
    plugins: STAMP(),
    engine: {
      documents: {
        noImage: assetDoc(ONE, { responses: [{ path: 'page.render.image', value: null }] }),
        noSource: assetDoc(ONE, {
          catalogWrites: [{ id: 'stamp-lib-0-3' }],
          responses: [
            { path: 'page.render.image', value: { contentType: 'image/png', source: null } },
          ],
        }),
      },
      responses: opens('noImage', 'noSource'),
    },
    steps: [
      start,
      stamp('importLibrary', [PDF]),
      stamp('importLibrary', [PDF]),
      stamp('listLibraries'),
    ],
  },
];

const ids: ScenarioCase[] = [
  {
    name: 'a-second-import-allocates-fresh-ids',
    covers:
      'Preferred library ids survive once; repeated imports allocate fresh session ids. A preferred library id occupying the next fallback forces an additional uid draw. Asset ids derive independently from each library id and identifier.',
    plugins: STAMP(),
    engine: {
      documents: {
        lib: assetDoc(ONE, {
          catalogWrites: [
            { id: 'shared-library' },
            { id: 'stamp-lib-0-2' },
            { id: 'stamp-lib-0-6' },
          ],
          catalog: { Id: str('shared-library') },
          page: { 100: { Id: str('shared-library:Stamp1') } },
        }),
        clash: assetDoc(ONE, {
          catalogWrites: [{ id: 'stamp-lib-0-5' }],
          catalog: { Id: str('stamp-lib-0-5') },
          page: { 100: { Id: str('stamp-lib-0-5:Stamp1') } },
        }),
      },
      responses: opens('lib', 'lib', 'clash', 'lib'),
    },
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
  {
    name: 'ids-carry-the-clock',
    covers:
      'Fresh per-case shipping uid state uses the prefix, fake Date.now base36 and a base36 counter; the complete typed write answer matches the actual clock-dependent library id.',
    plugins: STAMP(),
    engine: {
      documents: { lib: assetDoc(ONE, { catalogWrites: [{ id: 'stamp-lib-21i3v9-1' }] }) },
      responses: opens('lib'),
    },
    snapshot: ['stamp'],
    steps: [start, advance(123456789), stamp('importLibrary', [PDF])],
  },
  {
    name: 'concurrent-imports-read-the-store-before-either-lands',
    covers:
      'Deferred downloads let both imports read before committing. Different library ids own distinct derived asset ids; deleting one library leaves the other intact. Concurrent equal preferred library ids still produce duplicate display-order entries, then deleteLibrary drops every matching entry and asset.',
    plugins: STAMP(),
    engine: {
      documents: {
        first: assetDoc(ONE, {
          catalogWrites: [{ id: 'lib-a' }],
          catalog: { Id: str('lib-a') },
          page: { 100: { Id: str('shared') } },
          responses: [{ path: 'download', defer: 'save-first' }],
        }),
        second: assetDoc(ONE, {
          catalogWrites: [{ id: 'lib-b' }],
          catalog: { Id: str('lib-b') },
          page: { 100: { Id: str('shared') } },
          download: 'second',
        }),
        again: assetDoc(ONE, {
          catalogWrites: [{ id: 'lib-c' }],
          catalog: { Id: str('lib-c') },
          page: { 100: { Id: str('other') } },
          responses: [{ path: 'download', defer: 'save-again' }],
        }),
        same: assetDoc(ONE, {
          catalogWrites: [{ id: 'lib-c' }],
          catalog: { Id: str('lib-c') },
          page: { 100: { Id: str('other') } },
        }),
      },
      responses: opens('first', 'second', 'again', 'same'),
    },
    steps: [
      start,
      stamp('importLibrary', [PDF]),
      stamp('importLibrary', [PDF]),
      settle('save-first', PDF),
      stamp('listAssets', [{ libraryId: 'lib-a' }]),
      stamp('deleteLibrary', ['lib-a']),
      stamp('listAssets', [{ libraryId: 'lib-a' }]),
      stamp('listAssets', [{ libraryId: 'lib-b' }]),
      stamp('listAssets'),
      stamp('getLibrary', ['lib-a']),
      stamp('importLibrary', [PDF]),
      stamp('importLibrary', [PDF]),
      settle('save-again', PDF),
      stamp('listLibraries'),
      stamp('deleteLibrary', ['lib-c']),
      stamp('listLibraries'),
      stamp('listAssets'),
    ],
  },
];

const engines: ScenarioCase[] = [
  {
    name: 'an-asset-engine-factory-is-called-once',
    covers:
      'The current configured local factory is lazy, memoized across two imports and owned by the plugin; destroy closes its documents first and destroys its produced engine once.',
    plugins: STAMP({ assetEngine: 'factory' }),
    engine: {
      documents: {
        lib: assetDoc(ONE, { catalogWrites: [{ id: 'stamp-lib-0-1' }, { id: 'stamp-lib-0-3' }] }),
      },
      responses: opens('lib', 'lib'),
    },
    snapshot: ['stamp'],
    steps: [start, stamp('importLibrary', [PDF]), stamp('importLibrary', [PDF]), destroy],
  },
  {
    name: 'an-async-asset-engine-factory',
    covers:
      'A current configured factory may resolve a local engine asynchronously; it is awaited and its produced engine is destroyed on teardown.',
    plugins: STAMP({ assetEngine: 'async-factory' }),
    engine: { documents: { lib: assetDoc(ONE) }, responses: opens('lib') },
    snapshot: ['stamp'],
    steps: [start, stamp('importLibrary', [PDF]), destroy],
  },
  {
    name: 'a-throwing-asset-engine-factory-is-retried',
    covers:
      'A synchronous factory throw is wrapped with its exact Error cause and retried next call. No engine is owned or destroyed.',
    plugins: STAMP({ assetEngine: { throws: { name: 'Error', message: 'wasm failed to load' } } }),
    snapshot: ['stamp'],
    steps: [start, stamp('importLibrary', [PDF]), stamp('importLibrary', [PDF]), destroy],
  },
  {
    name: 'a-rejecting-asset-engine-factory-is-memoized',
    covers:
      'An asynchronously rejecting factory is memoized as that rejection; the next call does not invoke it. Cleanup swallows the owned rejection and destroys no engine.',
    plugins: STAMP({ assetEngine: { rejects: { name: 'Error', message: 'chunk load failed' } } }),
    snapshot: ['stamp'],
    steps: [start, stamp('importLibrary', [PDF]), stamp('importLibrary', [PDF]), destroy],
  },
  {
    name: 'an-engine-instance-is-not-destroyed',
    covers:
      'A configured local engine instance is borrowed and never destroyed by the plugin. Teardown clears all binary resources.',
    plugins: STAMP({ assetEngine: 'instance' }),
    engine: { documents: { lib: assetDoc(ONE) }, responses: opens('lib') },
    snapshot: ['stamp'],
    steps: [start, stamp('importLibrary', [PDF]), destroy],
  },
  {
    name: 'the-plugin-factory-defaults',
    covers:
      'The actual shipping definition retains workspace scope, defaults, state and inScope. With no assetEngine configured the current remote kernel engine refuses import before open; a separate explicit local-host check proves the same defaults render at 256 pixels successfully.',
    plugins: [{ factory: 'stamp-current' }],
    engine: { documents: { lib: assetDoc(ONE) }, responses: opens('lib') },
    steps: [start, stamp('importLibrary', [PDF]), stamp('listLibraries')],
  },
];

const current: ScenarioCase[] = [
  {
    name: 'named-registry-order-filters-and-preview-cache',
    covers:
      'Current PDF title and named-page registry own labels and page order; hidden templates warn once. The cached picker thumbnail is distinct from width-bucket ghost previews; repeated widths reuse one render and failures fall back to the thumbnail.',
    plugins: STAMP(),
    engine: {
      documents: {
        lib: assetDoc(TWO, {
          title: 'PDF Title',
          catalogWrites: [{ id: 'registered', kind: 'signatures', locale: 'en-GB' }],
          pageWrites: { 100: { kind: 'signature', subject: 'Override', categories: ['Legal'] } },
          catalog: { Id: str('registered'), Kind: nm('SignatureLibrary'), Locale: str('en-GB') },
          namedPages: [
            { name: 'Later=Second', target: { kind: 'page', page: TWO[1].ref } },
            { name: 'First=Approved', target: { kind: 'page', page: TWO[0].ref } },
            { name: 'Hidden', target: { kind: 'template', objectNumber: 333 } },
          ],
          page: {
            100: {
              Kind: nm('Signature'),
              SubjectOverride: str('Override'),
              Categories: arr('Legal'),
            },
          },
        }),
        ghost: assetDoc(ONE),
        failed: assetDoc(ONE, {
          responses: [{ path: 'page.render.image', error: engineError('Unknown', 'ghost failed') }],
        }),
      },
      responses: opens('lib', 'ghost', 'failed'),
    },
    steps: [
      start,
      stamp('importLibrary', [PDF, { name: 'Caller fallback' }]),
      stamp('listLibraries', [{ kind: ['signatures'] }]),
      stamp('listAssets', [{ category: 'Legal', kind: 'signature' }]),
      stamp('getAssetPreview', ['registered:First']),
      stamp('renderAssetPreview', ['registered:First', { width: 512 }]),
      stamp('renderAssetPreview', ['registered:First', { width: 512 }]),
      stamp('renderAssetPreview', ['registered:First', { width: 1024 }]),
      stamp('getAssetPreview', ['registered:First']),
      stamp('readAssetBytes', ['registered:First']),
      stamp('exportLibrary', ['registered']),
    ],
  },
  {
    name: 'persistence-restores-canonical-bytes-and-unsubscribes',
    covers:
      'The shipping persistence helpers restore file-owned identity, save canonical bytes after changes, delete removed libraries, and stop observing after unsubscribe; memory storage copies caller bytes.',
    plugins: [...STAMP(), { factory: 'stamp-current-persistence' }],
    engine: {
      documents: {
        stored: assetDoc(ONE, {
          catalogWrites: [{ id: 'persisted' }],
          metadataTitles: ['Stamps', 'Saved'],
          catalog: { Id: str('persisted') },
        }),
        fresh: assetDoc(ONE, { catalogWrites: [{ id: 'fresh' }], catalog: { Id: str('fresh') } }),
      },
      responses: opens('stored', 'stored', 'fresh'),
    },
    steps: [
      start,
      {
        op: 'call',
        target: { token: 'stamp-persistence' },
        method: 'seed',
        args: [[{ id: 'hint-only', bytes: { $bytes: Buffer.from(PDF).toString('base64') } }]],
      },
      { op: 'call', target: { token: 'stamp-persistence' }, method: 'restore' },
      { op: 'call', target: { token: 'stamp-persistence' }, method: 'persist' },
      stamp('updateLibrary', ['persisted', { name: 'Saved' }]),
      { op: 'call', target: { token: 'stamp-persistence' }, method: 'rows' },
      stamp('deleteLibrary', ['persisted']),
      { op: 'call', target: { token: 'stamp-persistence' }, method: 'rows' },
      { op: 'call', target: { token: 'stamp-persistence' }, method: 'stop' },
      stamp('importLibrary', [PDF]),
      { op: 'call', target: { token: 'stamp-persistence' }, method: 'rows' },
    ],
  },
];

export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/stamp-library.json',
    describes:
      'packages/plugin/stamp: importing canonical PDF libraries, PieceInfo metadata, ids, the asset engine port and its failures.',
    cases: [...importing, ...failures, ...blankRenders, ...ids, ...engines, ...current],
  },
];
