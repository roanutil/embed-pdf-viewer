/** Reconciliation moved out of the pure Session; these scenarios exercise its real current owner. */
import type { Annotation, AnnotationDraft, AnnotationPatch } from '@embedpdf/engine-core/runtime';
import { annotationAfter, shapeOf, styleOf } from '../../../../packages/core/annotation/src/record';
import { measurementOf } from '../../../../packages/core/annotation/src/measurement';
import { measurementLayout } from '../../../../packages/core/annotation/src/measurement-shape';
import type { ScenarioCase, Step } from '../scenario';
import type { Suite } from '../vectors';
import {
  scenario,
  call,
  hydrate,
  write,
  confirmation,
  created,
  updated,
  materialize,
  square,
  freeText,
  textNote,
  link,
  baseRef,
  pr,
  objRef,
  emit,
  createdFact,
  updatedFact,
  deletedFact,
  settle,
  fail,
  METRES,
} from './annotation-live-fixtures';
const box = { x: 100, y: 100, width: 100, height: 100 };
const draft: AnnotationDraft = {
  subtype: 'square',
  box,
  color: '#ef4444',
  strokeWidth: 2,
  print: true,
};
const ref = objRef(10001);
const select = (...refs: ReturnType<typeof objRef>[]) => call('selection.set', [refs]);
const edit = (phase: string, x: number, y: number): Step =>
  call('editPointer', [phase, pr(1), { x, y }, false]);
const point = { move: { x: 100, y: 125 }, handle: { x: 200, y: 200 }, rotate: { x: 150, y: 76 } };
const end = { move: { x: 120, y: 145 }, handle: { x: 220, y: 220 }, rotate: { x: 210, y: 150 } };
const measuredDraft = {
  subtype: 'line',
  linePoints: { start: { x: 100, y: 300 }, end: { x: 244, y: 300 } },
  intent: 'line-dimension',
  measure: METRES,
  captionEnabled: true,
  captionPosition: 'inline',
  color: '#ef4444',
  strokeWidth: 1,
  leader: { length: 12, extension: 5, offset: 0 },
  print: true,
} satisfies AnnotationDraft;
const measured = materialize(1, measuredDraft);
const caption = measurementLayout(shapeOf(measured), measurementOf(measured)!, styleOf(measured))!
  .caption!.center;
const snapshotSteps = [
  call('list'),
  call('selection.list'),
  call('listPageItems', [pr(1)]),
  call('text.getEditing'),
  call('getHovered'),
];
const finishPatch: Record<keyof typeof point, AnnotationPatch> = {
  move: { box: { x: 120, y: 120, width: 100, height: 100 } },
  handle: { box: { x: 100, y: 100, width: 120, height: 120 } },
  rotate: { rotation: 90 },
};
/** Inputs are current public calls. No removed core reconciliation message is recreated. */
function pending(
  name: string,
  gesture: keyof typeof point,
  outcome: 'confirm' | 'refuse' | 'delete' | 'disappear' = 'confirm',
  echo = false,
): ScenarioCase {
  const a = materialize(10001, draft),
    p = point[gesture],
    to = end[gesture];
  const confirmed = confirmation(1, [{ item: created(a) }]);
  const steps: Step[] = [
    call('onWriteFailed', [{ $fn: 'writeFailed' }]),
    call('create', [pr(1), draft]),
    call('isPending', [ref]),
    select(ref),
    edit('down', p.x, p.y),
    edit('move', p.x + 2, p.y + 2),
  ];
  if (outcome === 'refuse') steps.push(fail('born', 'intended pending-create refusal'));
  else {
    steps.push(settle('born', confirmed.value));
    if (echo) steps.push(emit(updatedFact({ ...a, contents: 'remote echo' }, false)));
    if (outcome === 'delete') steps.push(emit(deletedFact([ref])));
    if (outcome === 'disappear') steps.push(call('refresh'));
  }
  steps.push(
    call('selection.list'),
    edit('move', to.x, to.y),
    edit('up', to.x, to.y),
    ...snapshotSteps,
  );
  return scenario(
    `reconcile/${name}`,
    `Current stable-born ${gesture}: ${outcome}; gesture begins before the engine confirms the create.`,
    steps,
    [
      hydrate(),
      write(1, [{ item: created(a) }], { defer: 'born' }),
      ...(outcome === 'confirm'
        ? [
            write(2, [
              {
                item: updated(
                  annotationAfter(
                    echo ? { ...a, contents: 'remote echo' } : a,
                    finishPatch[gesture],
                  ),
                ),
              },
            ]),
          ]
        : []),
    ]
      .map((r, i) => (i === 0 && outcome === 'disappear' ? { ...r, times: 1 } : r))
      .concat(outcome === 'disappear' ? [hydrate()] : []),
  );
}
const stampDraft = {
  subtype: 'stamp',
  box: { x: 100, y: 100, width: 100, height: 50 },
  rotation: null,
  fit: 'contain',
  name: 'Approved',
} satisfies AnnotationDraft;
const PNG = {
  $bytes: Buffer.from([
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 100, 0, 0, 0, 50,
  ]).toString('base64'),
};
function pendingStamp(refuse: boolean): ScenarioCase {
  const a = materialize(10001, stampDraft);
  return scenario(
    `reconcile/h3-pending-stamp-${refuse ? 'refuse' : 'confirm'}`,
    'A placed stamp uses the real resource/preview path, starts a move before acknowledgement, and confirms or is forgotten on refusal.',
    [
      call('onWriteFailed', [{ $fn: 'writeFailed' }]),
      call('stamps.place', [
        { source: PNG, name: 'Approved' },
        { page: pr(1), center: { x: 150, y: 125 } },
      ]),
      select(ref),
      call('renderAppearances', [pr(1), 1]),
      edit('down', 150, 125),
      edit('move', 160, 135),
      ...(refuse
        ? [fail('stamp', 'intended stamp refusal')]
        : [settle('stamp', confirmation(1, [{ item: created(a) }]).value)]),
      edit('move', 180, 155),
      edit('up', 180, 155),
      call('renderAppearances', [pr(1), 1]),
      ...snapshotSteps,
    ],
    [
      hydrate(),
      write(1, [{ item: created(a) }], { defer: 'stamp' }),
      ...(refuse
        ? []
        : [
            write(2, [
              {
                item: updated(
                  annotationAfter(a, { box: { x: 130, y: 130, width: 100, height: 50 } }),
                ),
              },
            ]),
          ]),
    ],
  );
}
const cases: ScenarioCase[] = [
  scenario(
    'reconcile/loaded',
    'A confirmed full load replaces existing geometry and adds new records without a removed loaded message.',
    [call('list'), call('refresh'), ...snapshotSteps],
    [
      { ...hydrate([square(1)]), times: 1 },
      hydrate([
        square(1, {
          box: { x: 0, y: 0, width: 10, height: 10 },
          rect: { x: 0, y: 0, width: 10, height: 10 },
        }),
        square(2),
        square(3),
      ]),
    ],
  ),
  scenario(
    'reconcile/hydrated-reaps',
    'Reload disappearance forgets selected/hovered/edited records and preserves surviving selection.',
    [
      select(objRef(1), objRef(2)),
      call('hoverAt', [{ page: pr(1), point: { x: 300, y: 350 } }]),
      call('text.begin', [objRef(2)]),
      call('refresh'),
      ...snapshotSteps,
    ],
    [
      {
        ...hydrate([
          square(1),
          freeText(2, {
            box: { x: 300, y: 300, width: 100, height: 100 },
            rect: { x: 300, y: 300, width: 100, height: 100 },
          }),
        ]),
        times: 1,
      },
      hydrate([square(1)]),
    ],
  ),
  scenario(
    'reconcile/hydrated-bump',
    'Full refresh advances surviving picture versions and discards grouped stale parts.',
    [
      call('getAppearanceEpoch', [pr(1)]),
      call('refresh'),
      call('getAppearanceEpoch', [pr(1)]),
      ...snapshotSteps,
    ],
    [
      { ...hydrate([square(1), square(2, { reply: { to: objRef(1), type: 'group' } })]), times: 1 },
      hydrate([square(1), square(3)]),
    ],
  ),
  scenario(
    'reconcile/hydrated-keeps-dragged',
    'Current H3 deliberately forgets a drag target that disappears during refresh, rather than retaining a stale dragged record.',
    [
      select(objRef(1)),
      edit('down', 100, 125),
      edit('move', 110, 160),
      call('refresh'),
      edit('move', 120, 170),
      edit('up', 120, 170),
      ...snapshotSteps,
    ],
    [{ ...hydrate([square(1)]), times: 1 }, hydrate()],
  ),
  scenario(
    'reconcile/hydrated-nothing-reaped',
    'Refreshing identical records preserves valid selection and all records.',
    [select(objRef(1)), call('refresh'), ...snapshotSteps],
    [hydrate([square(1)])],
  ),
  scenario(
    'reconcile/hydrated-empty-hover',
    'An empty mirror has no hovered/editor refs; refresh and text.end keep the empty session safe.',
    [call('hoverAt', [null]), call('text.end'), call('refresh'), ...snapshotSteps],
  ),
  scenario(
    'reconcile/hydrated-keeps-surviving-hover',
    'A full refresh preserves hover and editor refs still in the mirror while removing another record.',
    [
      call('hoverAt', [{ page: pr(1), point: { x: 100, y: 150 } }]),
      call('text.begin', [objRef(2)]),
      call('refresh'),
      ...snapshotSteps,
    ],
    [
      {
        ...hydrate([
          square(1),
          freeText(2, {
            box: { x: 300, y: 300, width: 100, height: 100 },
            rect: { x: 300, y: 300, width: 100, height: 100 },
          }),
          square(3, {
            box: { x: 450, y: 450, width: 50, height: 50 },
            rect: { x: 450, y: 450, width: 50, height: 50 },
          }),
        ]),
        times: 1,
      },
      hydrate([
        square(1),
        freeText(2, {
          box: { x: 300, y: 300, width: 100, height: 100 },
          rect: { x: 300, y: 300, width: 100, height: 100 },
        }),
      ]),
    ],
  ),
  scenario(
    'reconcile/created',
    'A pending free text can be selected and edited by its final born ref; confirmation keeps both refs stable.',
    [
      call('create', [pr(1), { subtype: 'free-text', box, contents: 'born' }]),
      select(ref),
      call('text.begin', [ref]),
      call('text.getEditing'),
      settle(
        'born',
        confirmation(1, [
          { item: created(materialize(10001, { subtype: 'free-text', box, contents: 'born' })) },
        ]).value,
      ),
      call('text.getEditing'),
      ...snapshotSteps,
    ],
    [
      hydrate(),
      write(
        1,
        [{ item: created(materialize(10001, { subtype: 'free-text', box, contents: 'born' })) }],
        { defer: 'born' },
      ),
    ],
  ),
  scenario(
    'reconcile/created-index-ref',
    'A baseIndex read keeps its name while a new annotation is born under a reserved objectNumber; no mutable index remap exists.',
    [
      select(baseRef(0)),
      call('create', [pr(1), draft]),
      call('get', [ref]),
      settle('born', confirmation(1, [{ item: created(materialize(10001, draft)) }]).value),
      call('get', [baseRef(0)]),
      ...snapshotSteps,
    ],
    [
      hydrate([square(1, { ref: baseRef(0) })]),
      write(1, [{ item: created(materialize(10001, draft)) }], { defer: 'born' }),
    ],
  ),
  pending('created-mid-move', 'move'),
  pending('created-mid-handle', 'handle'),
  pending('created-mid-rotate', 'rotate'),
  scenario(
    'reconcile/created-mid-poly-draft',
    'An earlier pending create confirms while a polygon draft is active; the polygon keeps its points and can finish.',
    [
      call('create', [pr(1), draft]),
      call('createPointer', ['polygon', 'down', pr(1), { x: 300, y: 300 }]),
      call('draft.get'),
      settle('born', confirmation(1, [{ item: created(materialize(10001, draft)) }]).value),
      call('draft.get'),
      call('createPointer', ['polygon', 'down', pr(1), { x: 350, y: 300 }]),
      call('createPointer', ['polygon', 'down', pr(1), { x: 330, y: 350 }]),
      call('draft.finish'),
      ...snapshotSteps,
    ],
    [
      hydrate(),
      write(1, [{ item: created(materialize(10001, draft)) }], { defer: 'born' }),
      write(2, [
        {
          item: created(
            materialize(10002, {
              subtype: 'polygon',
              vertices: [
                { x: 300, y: 300 },
                { x: 350, y: 300 },
                { x: 330, y: 350 },
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
    'reconcile/created-relationships',
    'Reply and group children name their pending parent by its final ref before either engine confirmation.',
    [
      call('create', [pr(1), draft]),
      call('create', [
        pr(1),
        {
          subtype: 'text',
          rect: { x: 10, y: 10, width: 20, height: 20 },
          reply: { to: ref, type: 'reply' },
          contents: 'reply',
        },
      ]),
      call('create', [
        pr(1),
        {
          subtype: 'square',
          box: { x: 300, y: 300, width: 50, height: 50 },
          reply: { to: ref, type: 'group' },
        },
      ]),
      call('create', [
        pr(1),
        {
          subtype: 'link',
          rect: box,
          reply: { to: ref, type: 'group' },
          target: { kind: 'uri', uri: 'https://example.com' },
        },
      ]),
      call('list'),
      settle('parent', confirmation(1, [{ item: created(materialize(10001, draft)) }]).value),
      settle(
        'reply',
        confirmation(2, [
          {
            item: created(
              materialize(10002, {
                subtype: 'text',
                rect: { x: 10, y: 10, width: 20, height: 20 },
                reply: { to: ref, type: 'reply' },
                contents: 'reply',
              }),
            ),
          },
        ]).value,
      ),
      settle(
        'group',
        confirmation(3, [
          {
            item: created(
              materialize(10003, {
                subtype: 'square',
                box: { x: 300, y: 300, width: 50, height: 50 },
                reply: { to: ref, type: 'group' },
              }),
            ),
          },
        ]).value,
      ),
      settle(
        'uri',
        confirmation(4, [
          {
            item: created(
              materialize(10004, {
                subtype: 'link',
                rect: box,
                reply: { to: ref, type: 'group' },
                target: { kind: 'uri', uri: 'https://example.com' },
              }),
            ),
          },
        ]).value,
      ),
      select(objRef(10003)),
      ...snapshotSteps,
    ],
    [
      hydrate(),
      write(1, [{ item: created(materialize(10001, draft)) }], { defer: 'parent' }),
      write(
        2,
        [
          {
            item: created(
              materialize(10002, {
                subtype: 'text',
                rect: { x: 10, y: 10, width: 20, height: 20 },
                reply: { to: ref, type: 'reply' },
                contents: 'reply',
              }),
            ),
          },
        ],
        { defer: 'reply' },
      ),
      write(
        3,
        [
          {
            item: created(
              materialize(10003, {
                subtype: 'square',
                box: { x: 300, y: 300, width: 50, height: 50 },
                reply: { to: ref, type: 'group' },
              }),
            ),
          },
        ],
        { defer: 'group' },
      ),
      write(
        4,
        [
          {
            item: created(
              materialize(10004, {
                subtype: 'link',
                rect: box,
                reply: { to: ref, type: 'group' },
                target: { kind: 'uri', uri: 'https://example.com' },
              }),
            ),
          },
        ],
        { defer: 'uri' },
      ),
    ],
  ),
  pending('created-then-echo-mid-move', 'move', 'confirm', true),
  scenario(
    'reconcile/created-unknown',
    'A current remote create without a local pending entry adds exactly one baked record and keeps the local selection.',
    [select(objRef(1)), emit(createdFact(square(99))), ...snapshotSteps],
    [hydrate([square(1)])],
  ),
  scenario(
    'reconcile/create-failed',
    'A refused pending create leaves unrelated mirror records and selection intact.',
    [
      select(objRef(1)),
      call('create', [pr(1), draft]),
      call('get', [ref]),
      fail('born', 'intended create refusal'),
      ...snapshotSteps,
    ],
    [hydrate([square(1)]), { path: 'apply', defer: 'born' }],
  ),
  pending('create-failed-mid-move', 'move', 'refuse'),
  pending('create-failed-mid-handle', 'handle', 'refuse'),
  scenario(
    'reconcile/upsert',
    'Actual confirmed updates preserve order and advance picture version only when changed is true.',
    [
      emit(createdFact(square(3))),
      emit(updatedFact(square(1, { contents: 'first' }), true)),
      call('getAppearanceEpoch', [pr(1)]),
      emit(updatedFact(square(1, { contents: 'second' }), false)),
      call('getAppearanceEpoch', [pr(1)]),
      ...snapshotSteps,
    ],
    [hydrate([square(1), square(2)])],
  ),
  ...(['move', 'rotate', 'group'] as const).map((kind) =>
    scenario(
      `reconcile/upsert-while-${kind === 'move' ? 'dragging' : kind === 'rotate' ? 'rotating' : 'group'}`,
      'A current confirmed update of a surviving target keeps the real active gesture and selection safe.',
      [
        select(...(kind === 'move' ? [objRef(1)] : [objRef(1), objRef(2)])),
        edit(
          'down',
          kind === 'move' ? 100 : kind === 'rotate' ? 250 : 400,
          kind === 'rotate' ? 76 : kind === 'move' ? 125 : 150,
        ),
        emit(updatedFact(square(1, { contents: 'remote' }), false)),
        call('listPageItems', [pr(1)]),
        call('cancel'),
        ...snapshotSteps,
      ],
      [
        hydrate([
          square(1),
          square(2, {
            box: { x: 300, y: 100, width: 100, height: 100 },
            rect: { x: 300, y: 100, width: 100, height: 100 },
          }),
        ]),
      ],
    ),
  ),
  scenario(
    'reconcile/upsert-while-marquee',
    'A real marquee continues across confirmed updates and selects from the current mirrored geometry.',
    [
      call('marqueePointer', ['down', pr(1), { x: 0, y: 0 }, false]),
      call('marqueePointer', ['move', pr(1), { x: 250, y: 250 }, false]),
      emit(updatedFact(square(1, { color: '#000000' }))),
      call('marqueePointer', ['up', pr(1), { x: 250, y: 250 }, false]),
      ...snapshotSteps,
    ],
    [hydrate([square(1)])],
  ),
  scenario(
    'reconcile/bump-ap',
    'Picture epochs follow the actual engine appearance verdict; an inert update does not invalidate the raster.',
    [
      call('getAppearanceEpoch', [pr(1)]),
      emit(updatedFact(square(1), false)),
      call('getAppearanceEpoch', [pr(1)]),
      emit(updatedFact(square(1), true)),
      call('getAppearanceEpoch', [pr(1)]),
      ...snapshotSteps,
    ],
    [hydrate([square(1)])],
  ),
  scenario(
    'reconcile/measurement-create-annot',
    'Current create uses tool defaults, explicit endings, flags and URI target without a removed createAnnot message.',
    [
      call('tools.register', [
        {
          id: 'arrow',
          extends: 'line',
          defaults: { color: '#0000ff', lineEndings: { start: 'none', end: 'closed-arrow' } },
        },
      ]),
      call('create', [
        pr(1),
        { subtype: 'line', linePoints: { start: { x: 0, y: 0 }, end: { x: 50, y: 50 } } },
        undefined,
        { tool: 'arrow', select: true },
      ]),
      call('create', [
        pr(1),
        {
          subtype: 'link',
          rect: { x: 10, y: 10, width: 100, height: 30 },
          target: { kind: 'uri', uri: 'https://e.x' },
        },
      ]),
      call('create', [
        pr(2),
        {
          subtype: 'polygon',
          vertices: [
            { x: 0, y: 0 },
            { x: 20, y: 0 },
            { x: 0, y: 20 },
          ],
          hidden: true,
        },
      ]),
      ...snapshotSteps,
    ],
    [
      hydrate(),
      write(1, [
        {
          item: created(
            materialize(10001, {
              subtype: 'line',
              linePoints: { start: { x: 0, y: 0 }, end: { x: 50, y: 50 } },
              color: '#0000ff',
              lineEndings: { start: 'none', end: 'closed-arrow' },
              print: true,
              strokeWidth: 6,
            }),
          ),
        },
      ]),
      write(2, [
        {
          item: created(
            materialize(10002, {
              subtype: 'link',
              rect: { x: 10, y: 10, width: 100, height: 30 },
              target: { kind: 'uri', uri: 'https://e.x' },
            }),
          ),
        },
      ]),
      write(3, [
        {
          item: created(
            materialize(
              10003,
              {
                subtype: 'polygon',
                vertices: [
                  { x: 0, y: 0 },
                  { x: 20, y: 0 },
                  { x: 0, y: 20 },
                ],
                hidden: true,
              },
              2,
            ),
          ),
        },
      ]),
    ],
  ),
  scenario(
    'reconcile/measurement-hydrated-under-caption-drag',
    'A real current caption gesture survives a refresh that keeps its target, then synchronous forget ends it when the next refresh removes it.',
    [
      select(objRef(1)),
      edit('down', caption.x, caption.y),
      edit('move', caption.x + 10, caption.y + 10),
      call('refresh'),
      call('listPageItems', [pr(1)]),
      call('refresh'),
      edit('move', caption.x + 20, caption.y + 20),
      edit('up', caption.x + 20, caption.y + 20),
      ...snapshotSteps,
    ],
    [{ ...hydrate([measured]), times: 2 }, hydrate()],
  ),
  pending('h3-confirmed-deletion-mid-move', 'move', 'delete'),
  pending('h3-mirror-disappearance-mid-handle', 'handle', 'disappear'),
  pendingStamp(false),
  pendingStamp(true),
  scenario(
    'reconcile/h3-pending-caption-confirm',
    'A newly born measurement caption is dragged before create confirmation and commits its offset afterwards without losing identity.',
    [
      call('create', [pr(1), measuredDraft]),
      select(ref),
      edit('down', caption.x, caption.y),
      edit('move', caption.x + 10, caption.y + 10),
      settle(
        'caption',
        confirmation(1, [{ item: created(materialize(10001, measuredDraft)) }]).value,
      ),
      edit('move', caption.x + 20, caption.y + 20),
      edit('up', caption.x + 20, caption.y + 20),
      ...snapshotSteps,
    ],
    [
      hydrate(),
      write(1, [{ item: created(materialize(10001, measuredDraft)) }], { defer: 'caption' }),
      write(2, [
        {
          item: updated(
            annotationAfter(materialize(10001, measuredDraft), {
              captionOffset: { along: 20, perpendicular: -20 },
            }),
          ),
        },
      ]),
    ],
  ),
];
export const reconciliationMapping = [
  'loaded',
  'hydrated-reaps',
  'hydrated-bump',
  'hydrated-keeps-dragged',
  'hydrated-nothing-reaped',
  'hydrated-empty-hover',
  'hydrated-keeps-surviving-hover',
  'created',
  'created-index-ref',
  'created-mid-move',
  'created-mid-handle',
  'created-mid-rotate',
  'created-mid-poly-draft',
  'created-relationships',
  'created-then-echo-mid-move',
  'created-unknown',
  'create-failed',
  'create-failed-mid-move',
  'create-failed-mid-handle',
  'upsert',
  'upsert-while-dragging',
  'upsert-while-rotating',
  'upsert-while-group',
  'upsert-while-marquee',
  'bump-ap',
  'measurement-create-annot',
  'measurement-hydrated-under-caption-drag',
].map((name) => ({
  historical: name.startsWith('measurement-') ? name.slice('measurement-'.length) : name,
  family: name.startsWith('measurement-') ? 'annotation-measurement-model' : 'annotation-update',
  scenario: `reconcile/${name}`,
}));
export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/annotation-reconciliation.json',
    describes:
      'Current plugin store/mirror/pending coverage for all25 update+2 measurement reconciliation runs, plus H3 disappearance.',
    cases,
  },
];
