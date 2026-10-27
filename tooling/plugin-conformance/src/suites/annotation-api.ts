/** Current public nouns and engine-field API, backed by real mirror/pending/apply paths. */
import type { AnnotationDraft, Annotation } from '@embedpdf/engine-core/runtime';
import type { ScenarioCase, Step } from '../scenario';
import type { Suite } from '../vectors';
import { annotationAfter } from '../../../../packages/core/annotation/src/record';
import {
  scenario,
  call,
  square,
  circle,
  line,
  polygon,
  polyline,
  ink,
  markup,
  caret,
  callout,
  freeText,
  link,
  textNote,
  fileAttachment,
  stamp,
  pr,
  objRef,
  hydrate,
  doc,
  write,
  created,
  updated,
  deleted,
  materialize,
  refused,
  advance,
  emit,
  deletedFact,
  updatedFact,
  restoredFact,
  REMOTE,
  METRES,
  quad,
  PLUGINS,
  boot,
} from './annotation-live-fixtures';
const box = { x: 10, y: 20, width: 100, height: 40 };
const select = (...refs: ReturnType<typeof objRef>[]) => call('selection.set', [refs]);
const ptr = (tool: string, phase: string, x: number, y: number, page = 1, finish = false) =>
  call('createPointer', [tool, phase, pr(page), { x, y }, finish]);
const text = freeText(1);
const rich = freeText(1, {
  richText: {
    body: { ...text.richText.body, size: 20 },
    paragraphs: [{ runs: [{ text: 'Hel' }, { text: 'lo', style: { weight: 700 } }] }],
  },
});
const FEET = {
  ...METRES,
  ratio: '1 in = 1 ft',
  x: [{ unit: 'ft', conversion: 1 / 72 }],
  distance: [{ unit: 'ft', conversion: 1 }],
  area: [{ unit: 'ft²', conversion: 1 }],
};
export const dimensionDraft = {
  subtype: 'line',
  linePoints: { start: { x: 100, y: 100 }, end: { x: 244, y: 100 } },
  color: '#e5484d',
  strokeWidth: 1,
  lineEndings: { start: 'closed-arrow', end: 'closed-arrow' },
  intent: 'line-dimension',
  measure: METRES,
  captionEnabled: true,
  captionPosition: 'inline',
  leader: { length: 12, extension: 5, offset: 0 },
  print: true,
} satisfies AnnotationDraft;
export const dimension = (
  n: number,
  over: Partial<Extract<Annotation, { subtype: 'line' }>> = {},
) => ({ ...materialize(n, dimensionDraft), ...over }) as Extract<Annotation, { subtype: 'line' }>;
export const PNG = {
  $bytes: Buffer.from([
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 100, 0, 0, 0, 50,
  ]).toString('base64'),
};
const listenAll = [
  'onCreated',
  'onUpdated',
  'onDeleted',
  'onResynced',
  'onSelectionChanged',
  'onDraftChanged',
  'onEditingChanged',
  'onWriteFailed',
].map((name) => call(name, [{ $fn: name }]));
const createDrafts: AnnotationDraft[] = [
  { subtype: 'square', box, color: '#00ff00', print: true },
  { subtype: 'circle', box, rotation: 45, locked: true },
  { subtype: 'line', linePoints: { start: { x: 1, y: 2 }, end: { x: 50, y: 60 } } },
  {
    subtype: 'polygon',
    vertices: [
      { x: 0, y: 0 },
      { x: 50, y: 0 },
      { x: 25, y: 40 },
    ],
  },
  {
    subtype: 'polyline',
    vertices: [
      { x: 0, y: 0 },
      { x: 50, y: 10 },
    ],
  },
  {
    subtype: 'ink',
    inkList: [
      [
        { x: 0, y: 0 },
        { x: 5, y: 5 },
      ],
    ],
    intent: 'ink-highlight',
  },
  { subtype: 'highlight', quadPoints: [quad(10, 10, 50, 12)] },
  { subtype: 'redact', rect: box },
  {
    subtype: 'free-text',
    box,
    intent: 'free-text-callout',
    calloutLine: [
      { x: 0, y: 0 },
      { x: 10, y: 20 },
    ],
    lineEnding: 'open-arrow',
    contents: 'Callout',
  },
];
const cases: ScenarioCase[] = [
  scenario(
    'draft-changed-per-draft',
    'Current state-derived draft events track an actual move gesture; unchanged/foreign-page samples keep the gesture safe.',
    [
      call('onDraftChanged', [{ $fn: 'draft' }]),
      select(objRef(1)),
      call('editPointer', ['down', pr(1), { x: 100, y: 150 }, false]),
      call('editPointer', ['move', pr(1), { x: 110, y: 150 }, false]),
      call('editPointer', ['move', pr(1), { x: 110, y: 150 }, false]),
      call('editPointer', ['move', pr(2), { x: 120, y: 150 }, false]),
      call('editPointer', ['move', pr(1), { x: 100, y: 150 }, false]),
      call('editPointer', ['up', pr(1), { x: 100, y: 150 }, false]),
    ],
    [hydrate([square(1)])],
  ),
  scenario(
    'page-space-reads',
    'get/list return complete current engine records, filtered by pages/subtype; getAt and current nested selection/tools read actual public nouns.',
    [
      call('getStatus'),
      call('list'),
      call('get', [objRef(1)]),
      call('get', [objRef(99)]),
      call('list', [{ pages: [pr(2)] }]),
      call('list', [{ pages: [pr(1)], subtype: 'square' }]),
      call('getAt', [pr(1), { x: 100, y: 150 }]),
      call('getAt', [pr(1), { x: 5, y: 5 }]),
      select(objRef(1)),
      call('selection.list'),
      call('tools.list'),
      call('tools.get', ['square']),
      call('tools.register', [
        { id: 'arrow', extends: 'line', defaults: { lineEndings: { end: 'open-arrow' } } },
      ]),
      call('tools.get', ['arrow']),
      { op: 'call', target: { ref: 0 } },
      call('tools.get', ['arrow']),
    ],
    [
      hydrate([
        square(1),
        line(2),
        polygon(3),
        ink(4),
        markup('highlight', 5),
        caret(6),
        callout(7),
        link(8, { reply: { to: objRef(7), type: 'group' } }),
        textNote(9, { reply: { to: objRef(1), type: 'reply' }, contents: 'reply' }),
        circle(20, { page: pr(2), ref: objRef(20, 2) }),
      ]),
    ],
  ),
  scenario(
    'create-from-page-space',
    'Each kind is created with its actual engine draft; named tool defaults and flattened flags survive, invalid geometry/unknown tools/unsupported kinds refuse.',
    [
      ...listenAll,
      ...createDrafts.map((draft, i) =>
        call('create', [pr(1), draft, undefined, { select: i === 0 }]),
      ),
      call('create', [pr(1), { subtype: 'square', box }]),
      call('create', [pr(3), { subtype: 'square', box }]),
      call('create', [pr(1), { subtype: 'square', box: { x: 0, y: 0, width: -1, height: 1 } }]),
      call('create', [pr(1), { subtype: 'square', box }, undefined, { tool: 'nope' }]),
      call('create', [pr(1), { subtype: 'unsupported', rect: box }]),
      call('tools.register', [
        { id: 'flagged-box', extends: 'square', flags: { print: false, locked: true } },
      ]),
      call('create', [
        pr(1),
        { subtype: 'square', box, locked: false, hidden: true },
        undefined,
        { tool: 'flagged-box' },
      ]),
      call('list'),
    ],
    [
      hydrate(),
      ...createDrafts.map((draft, i) =>
        write(i + 1, [{ item: created(materialize(10001 + i, draft)) }]),
      ),
      refused('full'),
      refused('intended invalid geometry', 'InvalidArg'),
      write(12, [
        {
          item: created(
            materialize(10012, {
              subtype: 'square',
              box,
              locked: false,
              hidden: true,
              print: false,
              color: '#e5484d',
              strokeWidth: 6,
            }),
          ),
        },
      ]),
    ],
  ),
  scenario(
    'create-without-authority',
    'Current create/from-selection/resource creation refuse without create authority, while pointer/markup doors stay inert and twins remain truthful.',
    [
      call('canCreate'),
      call('create', [pr(1), { subtype: 'square', box }]),
      call('createFromSelection', ['highlight']),
      call('create', [
        pr(1),
        { subtype: 'file-attachment', rect: box, file: { name: 'a.bin' } },
        { file: { $bytes: 'AAEC' } },
      ]),
      call('createMarkup', ['highlight', pr(1), []]),
      ptr('square', 'down', 10, 10),
      call('draft.get'),
      call('canUpdate', [objRef(1)]),
      call('canDelete', [objRef(1)]),
      call('canUpdate', [objRef(99)]),
    ],
    [],
    {
      engine: {
        documents: {
          a: doc([hydrate([square(1)])], {
            security: {
              allows: ['doc.annotate.read'],
              allowsAnnotation: { create: false, update: false, delete: false },
            },
          }),
        },
      },
    },
  ),
  scenario(
    'update-in-page-space',
    'Current engine field patches update each shape family and rich bodies; bad geometry, absent refs, locks and engine refusals stay explicit.',
    [
      ...listenAll,
      call('update', [
        objRef(1),
        { box: { x: 10, y: 20, width: 30, height: 40 }, hidden: true, contents: 'new' },
      ]),
      call('get', [objRef(1)]),
      call('update', [objRef(2), { linePoints: { start: { x: 1, y: 2 }, end: { x: 30, y: 40 } } }]),
      call('update', [
        objRef(3),
        {
          vertices: [
            { x: 0, y: 0 },
            { x: 5, y: 0 },
            { x: 5, y: 5 },
          ],
        },
      ]),
      call('update', [
        objRef(4),
        {
          inkList: [
            [
              { x: 0, y: 0 },
              { x: 9, y: 9 },
            ],
          ],
        },
      ]),
      call('update', [objRef(5), { quadPoints: [quad(10, 10, 50, 12)] }]),
      call('update', [
        objRef(6),
        { box: { x: 100, y: 100, width: 150, height: 50 }, rotation: 90, calloutLine: null },
      ]),
      call('update', [objRef(7), { richText: { paragraphs: [{ runs: [{ text: 'rich' }] }] } }]),
      call('update', [objRef(1), { box: { x: 0, y: 0, width: -1, height: 1 } }]),
      call('update', [objRef(99), { contents: 'x' }]),
      call('update', [objRef(1), {}]),
      call('update', [objRef(8), { color: '#000000' }]),
      call('delete', [objRef(4)]),
      call('delete', [objRef(3)]),
      call('list'),
    ],
    [
      hydrate([
        square(1),
        line(2),
        polygon(3),
        ink(4),
        markup('highlight', 5),
        callout(6),
        freeText(7),
        square(8, { locked: true }),
      ]),
      ...[
        [
          square(1),
          { box: { x: 10, y: 20, width: 30, height: 40 }, hidden: true, contents: 'new' },
        ],
        [line(2), { linePoints: { start: { x: 1, y: 2 }, end: { x: 30, y: 40 } } }],
        [
          polygon(3),
          {
            vertices: [
              { x: 0, y: 0 },
              { x: 5, y: 0 },
              { x: 5, y: 5 },
            ],
          },
        ],
        [
          ink(4),
          {
            inkList: [
              [
                { x: 0, y: 0 },
                { x: 9, y: 9 },
              ],
            ],
          },
        ],
        [markup('highlight', 5), { quadPoints: [quad(10, 10, 50, 12)] }],
        [
          callout(6),
          { box: { x: 100, y: 100, width: 150, height: 50 }, rotation: 90, calloutLine: null },
        ],
        [freeText(7), { richText: { paragraphs: [{ runs: [{ text: 'rich' }] }] } }],
      ].map(([record, patch], i) =>
        write(i + 1, [
          {
            item: updated(
              annotationAfter(record as Annotation, patch as Parameters<typeof annotationAfter>[1]),
              true,
            ),
          },
        ]),
      ),
      refused('intended invalid geometry', 'InvalidArg'),
      write(9, [{ item: updated(square(8, { locked: true, color: '#000000' })) }]),
      refused('intended locked delete'),
      write(11, [{ item: deleted(objRef(3)), changed: [objRef(3)] }]),
    ],
  ),
  scenario(
    'selection-verbs',
    'Nested selectAll/selectInRect/rotate/reset/update/delete use actual current contracts and whole-change batch results.',
    [
      call('selection.selectAll'),
      call('selection.list'),
      call('selection.selectAll', [pr(2)]),
      call('selection.list'),
      call('selection.selectInRect', [pr(1), { x: 90, y: 90, width: 120, height: 120 }]),
      call('selection.list'),
      call('selection.selectInRect', [pr(1), { x: 590, y: 790, width: 5, height: 5 }]),
      call('selection.list'),
      select(objRef(1)),
      call('selection.rotateBy', [-90]),
      call('selection.resetRotation'),
      call('selection.update', [{ color: '#000000' }]),
      select(objRef(20, 2)),
      call('selection.delete'),
      call('selection.list'),
    ],
    [
      hydrate([
        square(1),
        circle(2, {
          box: { x: 300, y: 300, width: 100, height: 100 },
          rect: { x: 300, y: 300, width: 100, height: 100 },
        }),
        square(20, { page: pr(2), ref: objRef(20, 2) }),
      ]),
      write(1, [{ item: updated(annotationAfter(square(1), { rotation: 270 })) }]),
      write(2, [{ item: updated(square(1)) }]),
      refused('intended selection refusal'),
      write(4, [{ item: deleted(objRef(20, 2)), changed: [objRef(20, 2)] }]),
    ],
  ),
  scenario(
    'set-contents-settles-with-the-engine',
    'Explicit current contents/richText patches settle with their own refused engine writes.',
    [
      call('update', [objRef(2), { contents: 'plain' }]),
      call('update', [objRef(2), { richText: { paragraphs: [{ runs: [{ text: 'a' }] }] } }]),
    ],
    [hydrate([freeText(2)]), refused('read-only'), refused('read-only')],
  ),
  scenario(
    'text-range-needs-an-active-edit',
    'Current selection properties read range styles only while editing a nonempty text selection.',
    [
      select(objRef(1)),
      call('setTextSelection', [objRef(1), { start: 3, end: 5 }]),
      call('selection.getProperties'),
      call('text.begin', [objRef(1)]),
      call('selection.getProperties'),
      call('setTextSelection', [objRef(1), { start: 4, end: 4 }]),
      call('selection.getProperties'),
    ],
    [hydrate([rich])],
  ),
  scenario(
    'rich-text-editing',
    'Current rich editor holds expose paragraphs immediately; range changes, format toggles and text.end drive actual engine patches.',
    [
      ...listenAll,
      select(objRef(1)),
      call('text.begin', [objRef(1)]),
      call('text.getEditing'),
      call('setTextSelection', [objRef(1), { start: 1, end: 4 }]),
      call('selection.getProperties'),
      call('text.toggleFormat', ['italic']),
      call('selection.getProperties'),
      call('draftRichText', [
        objRef(1),
        { paragraphs: [{ runs: [{ text: 'Typing', style: { italic: true } }] }] },
      ]),
      call('text.getEditing'),
      call('setTextSelection', [objRef(1), null]),
      call('text.end'),
      call('get', [objRef(1)]),
      call('text.getEditing'),
      call('getCssFontFamily', ['Arial']),
      call('getCssFontFamily', ['Fancy']),
    ],
    [
      hydrate([rich]),
      write(1, [
        {
          item: updated(
            freeText(1, {
              contents: 'Typing',
              fontSize: 20,
              richText: {
                body: rich.richText.body,
                paragraphs: [{ runs: [{ text: 'Typing', style: { italic: true } }] }],
              },
            }),
          ),
        },
      ]),
    ],
  ),
  scenario(
    'attachments-stamps-and-downloads',
    'Current resource-separated create, downloadResource and stamps.place use complete drafts/resources; junk and absent pages refuse.',
    [
      ...listenAll,
      call('create', [
        pr(1),
        {
          subtype: 'file-attachment',
          rect: { x: 40, y: 50, width: 20, height: 20 },
          file: { name: 'a.bin', mimeType: 'application/octet-stream' },
        },
        { file: { $bytes: 'AAEC' } },
      ]),
      call('create', [
        pr(7),
        { subtype: 'file-attachment', rect: box, file: { name: 'a.bin' } },
        { file: { $bytes: 'AAEC' } },
      ]),
      call('create', [
        pr(1),
        { subtype: 'file-attachment', rect: box, file: { name: 'b.bin' } },
        { file: { $bytes: 'AAEC' } },
      ]),
      call('stamps.place', [
        { source: PNG, name: 'Approved', subject: 'Stamp' },
        { page: pr(1), center: { x: 300, y: 400 }, rotation: 90 },
      ]),
      call('stamps.place', [
        { source: { $bytes: 'AAEC' } },
        { page: pr(1), center: { x: 300, y: 400 } },
      ]),
      call('stamps.place', [{ source: PNG }, { page: pr(7), center: { x: 1, y: 1 } }]),
      call('downloadResource', [objRef(1), 'file']),
      call('selection.list'),
    ],
    [
      hydrate([fileAttachment(1)]),
      write(1, [
        {
          item: created(
            fileAttachment(10001, {
              color: '#ffff00',
              file: {
                name: 'a.bin',
                mimeType: 'application/octet-stream',
                size: 3,
                description: null,
                checksum: null,
                createdAt: null,
                modifiedAt: null,
              },
              rect: { x: 40, y: 50, width: 20, height: 20 },
            }),
          ),
        },
      ]),
      refused('too big'),
      write(3, [
        {
          item: created(
            materialize(10003, {
              subtype: 'stamp',
              box: { x: 250, y: 375, width: 100, height: 50 },
              rotation: 90,
              fit: 'contain',
              name: 'Approved',
              subject: 'Stamp',
            }),
          ),
        },
      ]),
      { path: 'page.annotations.downloadResource', pon: 1, value: { $bytes: 'AAEC' } },
    ],
  ),
  scenario(
    'measurement-distance-drafts',
    'Distance creation needs loaded viewport scales, retains the first scale through its offset stage, and calibration captures without creating.',
    [
      call('onDraftCaptured', [{ $fn: 'captured' }]),
      ptr('distance', 'down', 100, 100),
      call('draft.get'),
      call('setPageViewports', [
        pr(1),
        [
          {
            bbox: { x: 0, y: 0, width: 600, height: 800 },
            name: null,
            measure: METRES,
            owned: true,
          },
        ],
        FEET,
      ]),
      ptr('distance', 'down', 100, 100),
      ptr('distance', 'move', 150, 100),
      ptr('distance', 'up', 244, 100),
      call('distanceCreationPage'),
      call('draft.get'),
      ptr('distance', 'move', 170, 70),
      ptr('distance', 'down', 170, 70),
      call('list'),
      call('distanceCreationPage'),
      ptr('calibrate', 'down', 10, 10),
      ptr('calibrate', 'move', 60, 10),
      ptr('calibrate', 'up', 110, 10),
      call('list'),
    ],
    [
      hydrate(),
      write(1, [
        {
          item: created(
            dimension(10001, {
              subject: 'Distance',
              interiorColor: '#e5484d',
              leader: { length: 30, extension: 5, offset: 0 },
            }),
          ),
        },
      ]),
    ],
  ),
  scenario(
    'dimension-drafts-continue-without-viewports',
    'Perimeter and distance drafts continue on their own page after viewport withdrawal; another page cannot steal a draft.',
    [
      call('setPageViewports', [pr(1), [], METRES]),
      ptr('perimeter', 'down', 400, 400),
      ptr('perimeter', 'down', 500, 400),
      call('setPageViewports', [pr(1), undefined, METRES]),
      ptr('perimeter', 'down', 10, 10, 2),
      call('draft.get'),
      ptr('perimeter', 'down', 500, 500),
      ptr('perimeter', 'down', 450, 550, 1, true),
      call('setPageViewports', [pr(1), [], METRES]),
      ptr('distance', 'down', 100, 100),
      ptr('distance', 'up', 244, 100),
      call('setPageViewports', [pr(1), undefined, METRES]),
      ptr('distance', 'down', 10, 10, 2),
      call('draft.get'),
      ptr('distance', 'down', 170, 70),
      call('draft.get'),
      call('list'),
    ],
    [
      hydrate(),
      write(1, [
        {
          item: created(
            materialize(10001, {
              subtype: 'polyline',
              vertices: [
                { x: 400, y: 400 },
                { x: 500, y: 400 },
                { x: 500, y: 500 },
              ],
              intent: 'polyline-dimension',
              measure: METRES,
              captionEnabled: true,
              color: '#e5484d',
              strokeWidth: 1,
              subject: 'Perimeter',
            }),
          ),
        },
      ]),
      write(2, [
        {
          item: created(
            dimension(10002, {
              subject: 'Distance',
              interiorColor: '#e5484d',
              leader: { length: 30, extension: 5, offset: 0 },
            }),
          ),
        },
      ]),
    ],
  ),
  scenario(
    'calibration-needs-modify',
    'The current calibration capture needs doc.annotate.modify even when create is allowed.',
    [
      call('onDraftCaptured', [{ $fn: 'captured' }]),
      ptr('calibrate', 'down', 10, 10),
      ptr('calibrate', 'up', 110, 10),
      call('draft.get'),
      ptr('square', 'down', 10, 10),
      call('draft.get'),
    ],
    [],
    {
      engine: {
        documents: {
          a: doc([hydrate()], {
            security: { allows: ['doc.annotate.read'], allowsAnnotation: { create: true } },
          }),
        },
      },
    },
  ),
  scenario(
    'remeasure-page',
    'Recalibration reports actual updated, locked, foreign-measure and engine-refused records; other pages stay isolated.',
    [
      call('remeasurePage', [pr(1), FEET]),
      call('list'),
      call('remeasurePage', [pr(2), { ...FEET, distance: [] }]),
    ],
    [
      hydrate([
        dimension(1),
        dimension(2, { locked: true }),
        dimension(3, { measure: { subtype: 'geospatial' } }),
        dimension(6),
        dimension(20, { page: pr(2), ref: objRef(20, 2) }),
      ]),
      write(1, [{ item: updated(annotationAfter(dimension(1), { measure: FEET })) }]),
      refused('intended stale measurement', 'Conflict'),
    ],
  ),
  scenario(
    'remeasure-without-authority-and-after-errors',
    'No-authority dimensions are reported as skipped; an unreadable mirror reports its failure rather than a silent successful calibration.',
    [
      call('remeasurePage', [pr(1), FEET]),
      emit({ type: 'stream.desynced', reason: 'backlog-overflow', ts: 1 }),
      call('remeasurePage', [pr(1), FEET]),
    ],
    [],
    {
      engine: {
        documents: {
          a: doc(
            [
              { ...hydrate([dimension(1)]), times: 1 },
              {
                path: 'annotations.list',
                error: { name: 'EngineError', message: 'intended offline', code: 'Unavailable' },
              },
            ],
            { security: { allows: '*', allowsAnnotation: { update: false } } },
          ),
        },
      },
    },
  ),
  scenario(
    'v3-document-events',
    'The current document.versioned event reloads the flat annotation mirror; annotation restores cause page-specific reloads and keep prior data on failure.',
    [
      call('onResynced', [{ $fn: 'resynced' }]),
      emit({
        type: 'document.versioned',
        version: { sha256: '0'.repeat(64), byteLength: 100 },
        origin: REMOTE,
      }),
      call('list'),
      emit(restoredFact(pr(1))),
      call('getStatus'),
      call('list'),
    ],
    [
      { ...hydrate([square(1), square(2)]), times: 1 },
      { path: 'annotations.list', args: [], value: { annotations: [square(1)] }, times: 1 },
      {
        path: 'annotations.list',
        error: { name: 'EngineError', message: 'intended busy', code: 'Unavailable' },
      },
    ],
  ),
  scenario(
    'finish-drafts-and-the-edit-hub',
    'Nested draft.finish returns a real confirmed polygon; finish with none returns null and cancel clears another draft.',
    [
      call('draft.finish'),
      ptr('polygon', 'down', 10, 10),
      ptr('polygon', 'down', 100, 10),
      ptr('polygon', 'down', 100, 100),
      call('draft.get'),
      call('draft.finish'),
      call('list'),
      ptr('polyline', 'down', 10, 10),
      call('draft.cancel'),
      call('draft.get'),
    ],
    [
      hydrate(),
      write(1, [
        {
          item: created(
            materialize(10001, {
              subtype: 'polygon',
              vertices: [
                { x: 10, y: 10 },
                { x: 100, y: 10 },
                { x: 100, y: 100 },
              ],
              color: '#e5484d',
              strokeWidth: 6,
            }),
          ),
        },
      ]),
    ],
  ),
  scenario(
    'awaited-writes-and-pending-text',
    'A refused quarter-turn rejects its own promise; text deletion synchronously forgets its editor; file-picker removal keeps only the current provider.',
    [
      select(objRef(1)),
      call('selection.rotateBy', [90]),
      call('text.begin', [objRef(2)]),
      call('draftContents', [objRef(2), 'typing']),
      emit(deletedFact([objRef(2)])),
      call('text.getEditing'),
      call('text.end'),
      advance(300),
      call('setFilePickerProvider', [{ $fn: 'first', resolves: null }]),
      call('setFilePickerProvider', [{ $fn: 'second', resolves: null }]),
      { op: 'call', target: { ref: 0 } },
      {
        op: 'call',
        target: { token: 'interaction', doc: 'a' },
        method: 'activateTool',
        args: ['attachment'],
      },
      call('placeAt', [pr(1), { x: 50, y: 60 }]),
      { op: 'call', target: { ref: 1 } },
      call('placeAt', [pr(1), { x: 50, y: 60 }]),
      call('list'),
    ],
    [
      hydrate([square(1), freeText(2)]),
      refused('intended locked rotation'),
      write(2, [{ item: updated(freeText(2, { contents: 'typing' })) }]),
    ],
  ),
  scenario(
    'stamp-preview-providers',
    'Current stamps.arm and renderArmedStampPreview cache real scripted providers by size bucket, including an explicit failed-preview callback.',
    [
      call('stamps.arm', [
        {
          source: { $bytes: 'JVBERi0x' },
          intrinsicSize: { width: 100, height: 50 },
          preview: {
            $fn: 'render',
            seq: [
              { resolves: { bytes: { $bytes: 'AAEC' }, mimeType: 'image/png' } },
              { rejects: { name: 'Error', message: 'intended raster failure' } },
            ],
          },
        },
      ]),
      call('renderArmedStampPreview', [200]),
      call('renderArmedStampPreview', [250]),
      call('renderArmedStampPreview', [9000]),
      call('renderArmedStampPreview', [5000]),
      call('stamps.isArmed'),
      call('stamps.disarm'),
      call('stamps.isArmed'),
    ],
  ),
];
export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/annotation-api.json',
    describes:
      'Current annotation engine drafts/patches, nested public nouns, text, resources, stamps and measurement host seam.',
    cases,
  },
];
