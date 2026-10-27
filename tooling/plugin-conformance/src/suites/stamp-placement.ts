/** Current placement through annotation stamps and the actions-owned detached script realm. */
import type { ScenarioCase, Step } from '../scenario';
import type { Suite } from '../vectors';
import type { PageFlattenResult, PageImageResult } from '@embedpdf/engine-core/runtime';
import type { ScriptOutput } from '../../../../packages/core/acrojs/src/index';
import { canonical } from '../canonical';
import {
  STAMP,
  PDF,
  assetDoc,
  bytes,
  destroy,
  engineError,
  image,
  opens,
  pdf,
  png,
  stamp,
  start,
  settle,
} from './stamp-current-fixtures';
import {
  openDoc,
  namedEngine,
  ONE,
  TWO,
  appendDoc,
  seedDoc,
  fn,
  write,
  created,
} from './stamp-mutation-fixtures';
import { materialize, refused } from './annotation-live-fixtures';
import {
  assetPage,
  PLACEMENT,
  target,
  dynamicDoc,
  scripting,
  DATE,
  annotation,
  observers,
  setValue,
  pr,
} from './stamp-placement-fixtures';
const ID = 'stamp-lib-0-1:Stamp1';
const arm = (assetId = ID, options: { targetWidth?: number; documentId?: string } = {}): Step =>
  stamp('armAsset', [assetId, { documentId: 'a', ...options }]);
const armed = annotation('getArmedStamp');
const read = stamp('getArmedAsset', ['a']);
const boot: Step[] = [start, openDoc(), stamp('importLibrary', [PDF])];
const output = (over: Partial<ScriptOutput> = {}) => ({ output: over });
const budget = (maxEffects: number) => ({
  maxExecutionMs: 50,
  maxMemoryBytes: 16777216,
  maxStackBytes: 524288,
  maxEffects,
  maxOutputBytes: 1048576,
});
const alert = (message: string): ScriptOutput['uiEffects'][number] => ({
  kind: 'alert',
  message,
  icon: 0,
});
const diagnostic = { code: 'unsupported-api' as const, message: 'app.viewerVersion' };
const canonicalEngine = (copy = dynamicDoc()) =>
  namedEngine({ a: target(), lib: assetDoc(ONE), copy }, ['a', 'lib', 'copy']);
const staticCases: ScenarioCase[] = [
  {
    name: 'arm-delegates-to-the-annotation-plugin',
    covers:
      'Current armAsset forwards bytes, intrinsic aspect, target width and identifier; disarm drops both public reads and the interaction tool arm.',
    plugins: PLACEMENT(),
    engine: namedEngine({ a: target(), lib: assetDoc(ONE) }, ['a', 'lib']),
    steps: [
      ...boot,
      { op: 'call', target: { token: 'stamp' }, method: 'onArmChanged', args: [fn('armChanged')] },
      arm(ID, { targetWidth: 120 }),
      armed,
      read,
      stamp('disarm', ['a']),
      armed,
      read,
      arm(),
      armed,
      read,
    ],
  },
  {
    name: 'the-target-width-sizes-the-ghost-and-a-template-ghost-is-cached',
    covers:
      'Current hover image ghost follows page-space point and intrinsic aspect; two arms share one source-byte render in a width bucket.',
    plugins: PLACEMENT(),
    engine: namedEngine({ a: target(), lib: assetDoc(ONE), ghost: assetDoc(ONE) }, [
      'a',
      'lib',
      'ghost',
    ]),
    steps: [
      ...boot,
      arm(ID, { targetWidth: 120 }),
      annotation('hoverGhostAt', ['stamp', pr(1), { x: 300, y: 400 }]),
      annotation('getImageGhost', [pr(1)]),
      annotation('renderArmedStampPreview', [200]),
      stamp('disarm', ['a']),
      arm(),
      annotation('renderArmedStampPreview', [200]),
    ],
  },
  {
    name: 'a-raster-asset-arms-with-its-own-preview',
    covers:
      'A raster asset becomes a canonical PDF page with a rendered thumbnail; its placement aspect and rendered ghost survive. A PDF import whose preview render refuses leaves the raster arm and existing resources intact.',
    plugins: PLACEMENT(),
    engine: namedEngine(
      {
        a: target(),
        seed: seedDoc('stamp-lib-0-0', 'Logo'),
        append: appendDoc([assetPage(0, 3, 612, 792)], 'Logo', {
          raster: true,
          size: { width: 100, height: 50 },
        }),
        ghost: assetDoc(ONE),
        pdf: assetDoc(ONE, {
          catalog: { Id: { type: 'string', value: 'pdf' } },
          catalogWrites: [{ id: 'pdf' }],
          responses: [{ path: 'page.render.image', error: engineError('Io', 'no thumbnail') }],
        }),
      },
      ['a', 'seed', 'append', 'ghost', 'pdf'],
    ),
    steps: [
      start,
      openDoc(),
      stamp('createAsset', [{ name: 'Logo', source: png(100, 50) }]),
      arm('stamp-lib-0-0:Logo', { targetWidth: 50 }),
      armed,
      annotation('renderArmedStampPreview', [50]),
      stamp('getAssetPreview', ['stamp-lib-0-0:Logo']),
      stamp('importLibrary', [PDF]),
      arm('pdf:Stamp1'),
      armed,
      stamp('getAssetPreview', ['pdf:Stamp1']),
    ],
  },
  {
    name: 'arming-what-is-not-there',
    covers:
      'Unknown assets reject not-found; missing annotation capability and closed document reject permission-denied. Disarm is safely inert.',
    plugins: STAMP(),
    engine: namedEngine({ a: target({ responses: [] }), lib: assetDoc(ONE) }, ['a', 'lib']),
    steps: [
      ...boot,
      arm('nope'),
      arm(),
      arm(ID, { documentId: 'elsewhere' }),
      stamp('disarm', ['a']),
      stamp('disarm', ['elsewhere']),
      read,
    ],
  },
];
const skipped: ScenarioCase[] = [
  {
    name: 'scripting-off-arms-the-template',
    covers:
      'The dynamic setting false prevents detached copies even under an enabled actions scripting host.',
    plugins: PLACEMENT({ assetEngine: 'instance', dynamic: false }, scripting()),
    engine: namedEngine({ a: target(), lib: assetDoc(ONE) }, ['a', 'lib']),
    steps: [...boot, arm(), armed],
  },
  {
    name: 'no-sandbox-host',
    covers:
      'Without an actions host the template arms unchanged. Enabled actions with no injected sandbox uses current default QuickJS and is separately deferred to D.',
    plugins: PLACEMENT(),
    engine: namedEngine({ a: target(), lib: assetDoc(ONE) }, ['a', 'lib']),
    steps: [...boot, arm(), armed],
  },
  {
    name: 'a-raster-asset-is-never-evaluated',
    covers:
      'Canonical raster assets can carry forms now; a raster-authored page with no selected-page field opens/closes a copy and arms without creating a script realm.',
    plugins: PLACEMENT(undefined, scripting()),
    engine: namedEngine(
      {
        a: target(),
        seed: seedDoc('stamp-lib-0-0', 'Logo'),
        append: appendDoc([assetPage(0, 3, 612, 792)], 'Logo', {
          raster: true,
          size: { width: 100, height: 50 },
        }),
        copy: dynamicDoc({
          pages: [assetPage(0, 3, 612, 792), assetPage(1, 900, 100, 50)],
          fields: [],
          selected: 900,
        }),
      },
      ['a', 'seed', 'append', 'copy'],
    ),
    steps: [
      start,
      openDoc(),
      stamp('createAsset', [{ name: 'Logo', source: png(100, 50) }]),
      arm('stamp-lib-0-0:Logo'),
      armed,
    ],
  },
  {
    name: 'a-stamp-without-fields-on-its-page-arms-the-template',
    covers:
      'A field on another page does not count: close the copy and arm unchanged reusable bytes.',
    plugins: PLACEMENT(undefined, scripting()),
    engine: namedEngine(
      { a: target(), lib: assetDoc(TWO), copy: dynamicDoc({ pages: TWO, fields: [DATE(101)] }) },
      ['a', 'lib', 'copy'],
    ),
    steps: [...boot, arm(), armed],
  },
];
const dynamic: ScenarioCase[] = [
  {
    name: 'materializes-a-dynamic-stamp',
    covers:
      'The detached copy uses target identity/time/seed, current native form ops, flatten and extraction; the canonical library and page cache remain unchanged.',
    plugins: PLACEMENT(
      undefined,
      scripting({
        now: { $fn: 'now', returns: 1784107800000 },
        utcOffsetMinutes: { $fn: 'offset', returns: 180 },
        randomSeed: { $fn: 'seed', returns: 7 },
      }),
    ),
    engine: namedEngine(
      { a: target(), lib: assetDoc(ONE), copy: dynamicDoc(), ghost: assetDoc(ONE) },
      ['a', 'lib', 'copy', 'ghost'],
    ),
    steps: [
      ...boot,
      ...observers(),
      arm(ID, { targetWidth: 80 }),
      armed,
      annotation('renderArmedStampPreview', [64]),
      stamp('exportLibrary', ['stamp-lib-0-1']),
      stamp('readAssetBytes', [ID]),
    ],
  },
  {
    name: 'a-loose-dynamic-stamp',
    covers:
      'A PDF createAsset produces a canonical single-asset library, then evaluates the selected appended page; identityless target uses default identity.',
    plugins: PLACEMENT(undefined, scripting()),
    engine: namedEngine(
      {
        a: target({ security: { allows: '*' } }),
        seed: seedDoc('stamp-lib-0-0', 'Dynamic'),
        append: appendDoc([assetPage(0, 3, 612, 792)], 'Dynamic', { number: 900 }),
        copy: dynamicDoc({
          pages: [assetPage(0, 3, 612, 792), assetPage(1, 900, 300, 120)],
          fields: [DATE(900)],
          selected: 900,
        }),
      },
      ['a', 'seed', 'append', 'copy'],
    ),
    steps: [
      start,
      openDoc(),
      stamp('createAsset', [{ name: 'Dynamic', source: PDF, size: { width: 200, height: 80 } }]),
      arm('stamp-lib-0-0:Dynamic'),
      armed,
      stamp('readAssetBytes', ['stamp-lib-0-0:Dynamic']),
    ],
  },
  {
    name: 'dynamic-stamp-refusals',
    covers:
      'Closed target permission, absent canonical page, required flatten Unsupported and extract Io each refuse precisely, close the detached copy and leave no arm.',
    plugins: PLACEMENT(undefined, scripting()),
    engine: namedEngine(
      {
        a: target(),
        lib: assetDoc(ONE),
        moved: dynamicDoc({ pages: [{ ...ONE[0], ref: pr(300) }] }),
        noFlatten: dynamicDoc({
          responses: [
            { path: 'pages.flatten', error: engineError('Unsupported', 'flatten unavailable') },
          ],
        }),
        noExtract: dynamicDoc({
          responses: [{ path: 'pages.extract', error: engineError('Io', 'extract unavailable') }],
        }),
      },
      ['a', 'lib', 'moved', 'noFlatten', 'noExtract'],
    ),
    steps: [...boot, arm(ID, { documentId: 'closed' }), arm(), arm(), arm(), armed, read],
  },
  {
    name: 'a-failed-boot-script-is-a-diagnostic',
    covers:
      'A boot exception is surfaced as a continuing diagnostic without a script-failed event, while accepted recalculate still arms.',
    plugins: PLACEMENT(
      undefined,
      scripting({
        sandbox: {
          builds: [
            {
              boot: [
                output({
                  uiEffects: [alert('Stamp expired')],
                  diagnostics: [diagnostic],
                  error: { kind: 'exception', message: 'boom' },
                }),
              ],
            },
          ],
        },
      }),
    ),
    engine: canonicalEngine(),
    steps: [...boot, ...observers(), arm(), armed],
  },
  {
    name: 'a-failed-recalculation-fails-the-arm',
    covers:
      'Aggregate effect budget failure surfaces exact diagnostics/UI/error, rejects operation-failed, disposes realm and closes copy without arming.',
    plugins: PLACEMENT(
      undefined,
      scripting({
        sandbox: {
          builds: [
            {
              boot: [
                output({
                  formEffects: [setValue('x')],
                  uiEffects: [alert('Stamp expired')],
                  diagnostics: [diagnostic],
                }),
              ],
            },
          ],
        },
        budget: budget(1),
      }),
    ),
    engine: canonicalEngine(),
    steps: [...boot, ...observers(), arm(), armed, read],
  },
  {
    name: 'an-error-observer-that-throws',
    covers:
      'A throwing script-failed listener is isolated by the kernel report channel; the current arm still rejects its own budget failure.',
    plugins: PLACEMENT(undefined, scripting({ budget: budget(0) })),
    engine: canonicalEngine(),
    steps: [...boot, ...observers('error'), arm(), armed],
  },
  {
    name: 'observers-see-a-successful-run',
    covers:
      'Current actions UI adapter and event hooks surface accepted script outputs. A throwing UI adapter rejects the arm and finally closes the copy.',
    plugins: PLACEMENT(
      undefined,
      scripting({
        sandbox: {
          builds: [
            {
              boot: [
                output({
                  formEffects: [setValue('x')],
                  uiEffects: [alert('one'), alert('two')],
                  diagnostics: [diagnostic],
                }),
              ],
            },
          ],
        },
      }),
    ),
    engine: canonicalEngine(dynamicDoc({ formValue: 'x' })),
    steps: [...boot, ...observers('ui'), arm(), armed],
  },
  {
    name: 'a-flatten-that-fails-or-skips',
    covers:
      'Current flatten is all-or-nothing: engine refusal rejects the arm; typed unchanged is a successful no-op and still materializes.',
    plugins: PLACEMENT(undefined, scripting()),
    engine: namedEngine(
      {
        a: target(),
        lib: assetDoc(ONE),
        failed: dynamicDoc({
          responses: [{ path: 'pages.flatten', error: engineError('Refused', 'flatten failed') }],
        }),
        unchanged: dynamicDoc({
          flatten: {
            pages: [pr(100)],
            usage: 'display',
            results: [{ page: pr(100), status: 'unchanged' }],
            meta: {
              affectedPages: [pr(100)],
              cacheDelta: null,
              opId: 'flatten-unchanged',
              undoable: false,
            },
          } satisfies PageFlattenResult,
        }),
      },
      ['a', 'lib', 'failed', 'unchanged'],
    ),
    steps: [...boot, arm(), armed, arm(), armed],
  },
  {
    name: 'a-failed-copy-close-fails-the-arm',
    covers:
      'Copy close Io overrides a successful materialization. A well-typed URL-only image has no source bytes and rejects unsupported after close.',
    plugins: PLACEMENT(undefined, scripting()),
    engine: namedEngine(
      {
        a: target(),
        lib: assetDoc(ONE),
        copy: dynamicDoc({
          responses: [{ path: 'close', error: engineError('Io', 'close failed') }],
        }),
        url: dynamicDoc({
          responses: [
            {
              path: 'page.render.image',
              value: {
                ...image(100),
                source: { kind: 'url', url: 'https://x' },
              } satisfies PageImageResult,
            },
          ],
        }),
      },
      ['a', 'lib', 'copy', 'url'],
    ),
    steps: [...boot, arm(), arm(), armed],
  },
  {
    name: 'a-library-without-canonical-bytes',
    covers:
      'Explicit malformed download/extract replies exercise current fallback: evaluate extracted page when canonical bytes absent; invalid missing placement bytes reject by the current binary resolver.',
    plugins: PLACEMENT(undefined, scripting()),
    engine: namedEngine(
      {
        a: target(),
        lib: assetDoc(TWO, {
          responses: [{ path: 'download' }, { path: 'pages.extract', args: [[pr(101)]] }],
        }),
        copy: dynamicDoc({ pages: TWO }),
      },
      ['a', 'lib', 'copy', 'copy'],
    ),
    steps: [...boot, arm(), armed, stamp('disarm', ['a']), arm('stamp-lib-0-1:Stamp2'), armed],
  },
  {
    name: 'teardown-destroys-a-factory-engine-once',
    covers:
      'The lazily-created local engine is memoized across library and detached-copy opens and destroyed exactly once.',
    plugins: PLACEMENT({ assetEngine: 'factory' }, scripting()),
    engine: canonicalEngine(),
    steps: [...boot, arm(), armed, destroy],
  },
];

const dynamicPlaced = materialize(10001, {
  subtype: 'stamp',
  box: { x: 60, y: 80, width: 80, height: 40 },
  name: 'Stamp1',
  subject: 'Stamp1',
  fit: 'contain',
});
const batchPlaced = materialize(
  10003,
  {
    subtype: 'stamp',
    box: { x: 170, y: 185, width: 60, height: 30 },
    name: 'Stamp1',
    subject: 'Stamp1',
    fit: 'contain',
  },
  2,
);
const pickerPlaced = materialize(10001, {
  subtype: 'stamp',
  box: { x: 50, y: 75, width: 100, height: 50 },
  fit: 'contain',
});
const added: ScenarioCase[] = [
  {
    name: 'dynamic-placement-writes-materialized-bytes-and-batch-facts',
    covers:
      'Direct and batch placement use each fresh detached materialization as annotation appearance resources; one refused page does not suppress the later successful page or its fact and public readback.',
    plugins: PLACEMENT(undefined, scripting()),
    engine: namedEngine(
      {
        a: target({
          pages: [assetPage(0, 1, 600, 800), assetPage(1, 2, 500, 700)],
          responses: [
            ...target().responses!,
            write(1, [{ item: created(dynamicPlaced) }]),
            refused('page locked'),
            write(3, [{ item: created(batchPlaced) }]),
          ],
        }),
        lib: assetDoc(ONE),
        copy1: dynamicDoc(),
        copy2: dynamicDoc(),
        copy3: dynamicDoc(),
      },
      ['a', 'lib', 'copy1', 'copy2', 'copy3'],
    ),
    steps: [
      ...boot,
      annotation('onCreated', [fn('annotationCreated')]),
      annotation('onWriteFailed', [fn('annotationWriteFailed')]),
      stamp('placeAsset', [
        ID,
        { page: pr(1), center: { x: 100, y: 100 }, targetWidth: 80 },
        { documentId: 'a' },
      ]),
      stamp('placeAssetOnPages', [
        ID,
        'all',
        { center: { x: 200, y: 200 }, targetWidth: 60 },
        { documentId: 'a' },
      ]),
      stamp('placeAssetOnPages', [
        ID,
        [pr(1), pr(999)],
        { center: { x: 20, y: 20 } },
        { documentId: 'a' },
      ]),
      annotation('list'),
      stamp('readAssetBytes', [ID]),
      stamp('exportLibrary', ['stamp-lib-0-1']),
    ],
  },
  {
    name: 'file-picker-places-cancels-and-expires-the-click',
    covers:
      'The current prompt provider receives the typed page-space request; picked PNG bytes create one stamp, cancel creates none, and a delayed result after a tool change expires without a write.',
    plugins: PLACEMENT(),
    engine: namedEngine(
      {
        a: target({
          responses: [...target().responses!, write(1, [{ item: created(pickerPlaced) }])],
        }),
      },
      ['a'],
    ),
    steps: [
      start,
      openDoc(),
      {
        op: 'call',
        target: { token: 'interaction', doc: 'a' },
        method: 'activateTool',
        args: ['stamp'],
      },
      annotation('setFilePickerProvider', [
        {
          $fn: 'filePicker',
          seq: [
            { resolves: { data: png(100, 50), name: 'picked.png' } },
            { resolves: null },
            { defer: 'picked' },
          ],
        },
      ]),
      annotation('requestStampAt', [pr(1), { x: 100, y: 100 }]),
      annotation('requestStampAt', [pr(1), { x: 200, y: 200 }]),
      annotation('requestStampAt', [pr(1), { x: 300, y: 300 }]),
      {
        op: 'call',
        target: { token: 'interaction', doc: 'a' },
        method: 'activateTool',
        args: ['pan'],
      },
      settle('picked', { data: png(100, 50), name: 'late.png' }),
      annotation('list'),
    ],
  },
  {
    name: 'permission-refusal-recovers-on-an-authorized-document',
    covers:
      'An open read-only document refuses arm/place before asset copies; an independently authorized document can subsequently arm and disarm the same asset.',
    plugins: PLACEMENT(undefined, scripting()),
    engine: namedEngine(
      {
        a: target({ security: { allows: ['doc.annotate.read', 'doc.forms.read'] } }),
        b: target(),
        lib: assetDoc(ONE),
        copy: dynamicDoc(),
      },
      ['a', 'lib', 'b', 'copy'],
    ),
    steps: [
      ...boot,
      stamp('canPlace', ['a']),
      arm(),
      stamp('placeAsset', [ID, { page: pr(1), center: { x: 100, y: 100 } }, { documentId: 'a' }]),
      openDoc('b'),
      stamp('canPlace', ['b']),
      arm(ID, { documentId: 'b' }),
      annotation('getArmedStamp', [], 'b'),
      stamp('getArmedAsset', ['b']),
      stamp('disarm', ['b']),
      annotation('getArmedStamp', [], 'b'),
    ],
  },
];

export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/stamp-placement.json',
    describes: 'Current annotation placement, ghosts and detached actions/form scripting.',
    cases: [...staticCases, ...skipped, ...dynamic, ...added],
  },
];
