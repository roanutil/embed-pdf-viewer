/**
 * `packages/plugin/annotation`: whole-document hydration and the document
 * event stream (`effects.ts` and the capability's hydration controller).
 * Replayed by `crates/plugins/annotation/tests/conformance.rs`.
 */
import type { ScenarioCase } from '../scenario';
import type { Suite } from '../vectors';

import {
  PAGES,
  PLUGINS,
  SNAP,
  boot,
  call,
  circle,
  doc,
  emit,
  fail,
  hydrate,
  id,
  layout,
  objRef,
  settle,
  snapshot,
  square,
  textNote,
  widget,
  link,
} from './annotation-fixtures';

const remote = (serverId: unknown) => ({ kind: 'remote', serverId });
const LOCAL = { kind: 'local', serverId: null };

const seed = { 1: [square(1), square(2, { rect: { left: 300, bottom: 300, right: 400, top: 400 } })], 2: [circle(3, { pageObjectNumber: 2, rect: { left: 50, bottom: 100, right: 150, top: 200 } })] };

const cases: ScenarioCase[] = [
  {
    name: 'hydrates-every-page-at-open',
    covers:
      'The effect subscribes, then kicks listRawAll once: loading until it lands, then one hydrated upsert of every page it has a crop for (a page it does not know is skipped), complete. Reads answer from the model.',
    plugins: PLUGINS,
    engine: { documents: { a: doc([{ path: 'annotations.listRawAll', defer: 'all' }]) } },
    snapshot: SNAP,
    steps: [
      ...boot,
      call('hydration'),
      settle('all', snapshot({ ...seed, 9: [square(9, { pageObjectNumber: 9 })] })),
      call('hydration'),
      call('list', [1]),
      call('list', [2]),
      call('get', [objRef(3, 2)]),
      call('get', [objRef(77)]),
      call('appearanceEpoch', [1]),
      call('ensureHydrated'),
      call('ensurePage', [1]),
      call('canRead'),
    ],
  },
  {
    name: 'no-read-grant-means-forbidden',
    covers:
      'Without doc.annotate.read hydration never asks: the state says forbidden, canRead is false. rehydrate re-checks, but it stores the resolved early-return promise as the running one, so a second rehydrate only flags a re-run and hands the same promise back: nothing is asked again.',
    plugins: PLUGINS,
    engine: { documents: { a: doc([hydrate(seed)], { security: { allows: ['doc.annotate.create'] } }) } },
    snapshot: SNAP,
    steps: [...boot, call('hydration'), call('canRead'), call('rehydrate'), call('rehydrate'), call('hydration')],
  },
  {
    name: 'a-failed-hydration-degrades-to-live',
    covers:
      'A listRawAll that rejects: remote events queued in the window apply in order, the state is error with the engine error, and live events keep applying. A rehydrate that succeeds re-ingests with bumpAp and completes.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([
          { path: 'annotations.listRawAll', times: 1, defer: 'first' },
          { path: 'annotations.listRawAll', value: snapshot(seed) },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      emit({ type: 'annotation.created', pageObjectNumber: 1, origin: remote(4), created: square(5) }),
      fail('first', 'listRawAll failed'),
      call('hydration'),
      emit({ type: 'annotation.created', pageObjectNumber: 1, origin: remote(8), created: square(6) }),
      call('rehydrate'),
      call('hydration'),
      call('list', [1]),
    ],
  },
  {
    name: 'the-hydration-window-replays-by-cursor',
    covers:
      'Remote annotation events during hydration queue, and replay after ingest: serverId <= auditHead is already in the snapshot and drops (the delete-resurrection guard), a newer one applies, one without a serverId applies. Local events and non-annotation events never queue.',
    plugins: PLUGINS,
    engine: { documents: { a: doc([{ path: 'annotations.listRawAll', defer: 'all' }]) } },
    snapshot: SNAP,
    steps: [
      ...boot,
      emit({ type: 'annotation.deleted', pageObjectNumber: 1, origin: remote(9), deleted: { kind: 'objectNumber', value: 1 } }),
      emit({ type: 'annotation.created', pageObjectNumber: 1, origin: remote(11), created: square(7) }),
      emit({ type: 'annotation.updated', pageObjectNumber: 1, origin: remote(10), updated: square(2, { contents: 'old' }), appearance: { changed: true } }),
      emit({ type: 'annotation.updated', pageObjectNumber: 1, origin: remote(12), updated: square(2, { contents: 'new' }), appearance: { changed: false } }),
      emit({ type: 'annotation.deleted', pageObjectNumber: 1, origin: { kind: 'remote' }, deleted: { kind: 'objectNumber', value: 3 } }),
      emit({ type: 'annotation.created', pageObjectNumber: 1, origin: LOCAL, created: square(8) }),
      emit({ type: 'metadata.updated', metadata: {} }),
      call('deliverRemoteAnnotationEvent', [{ type: 'annotation.created', pageObjectNumber: 1, created: square(20) }]),
      settle('all', snapshot(seed, 10)),
      call('list', [1]),
      call('list', [2]),
    ],
  },
  {
    name: 'a-local-engine-replays-everything',
    covers: 'A snapshot without auditHead (a local engine) drops nothing it queued, and a string serverId compares as JavaScript does.',
    plugins: PLUGINS,
    engine: { documents: { a: doc([{ path: 'annotations.listRawAll', defer: 'all' }]) } },
    snapshot: SNAP,
    steps: [
      ...boot,
      emit({ type: 'annotation.created', pageObjectNumber: 1, origin: remote(1), created: square(7) }),
      emit({ type: 'annotation.created', pageObjectNumber: 1, origin: remote('b'), created: square(8) }),
      settle('all', snapshot({ 1: [] })),
      call('list', [1]),
    ],
  },
  {
    name: 'string-cursors-compare-as-strings',
    covers: 'With a string auditHead, string serverIds compare lexicographically.',
    plugins: PLUGINS,
    engine: { documents: { a: doc([{ path: 'annotations.listRawAll', defer: 'all' }]) } },
    snapshot: SNAP,
    steps: [
      ...boot,
      emit({ type: 'annotation.created', pageObjectNumber: 1, origin: remote('0005'), created: square(7) }),
      emit({ type: 'annotation.created', pageObjectNumber: 1, origin: remote('0012'), created: square(8) }),
      emit({ type: 'annotation.created', pageObjectNumber: 1, origin: remote(null), created: square(9) }),
      settle('all', { ...snapshot({ 1: [] }), auditHead: '0010' }),
      call('list', [1]),
    ],
  },
  {
    name: 'live-remote-events',
    covers:
      'After hydration, remote events apply at once. A create bumps the appearance epoch, an update bumps only when the engine says the appearance changed, a move never does, a substrate-only annotation (a reply) never does. A delete removes a known id (object number or /NM) and ignores an unknown or empty one. An annotation on a page without a crop is skipped.',
    plugins: PLUGINS,
    engine: { documents: { a: doc([hydrate({ 1: [square(1), square(2), square(4, { ref: { kind: 'nm', pageObjectNumber: 1, nm: 'n4' } })] })]) } },
    snapshot: SNAP,
    steps: [
      ...boot,
      emit({ type: 'annotation.created', pageObjectNumber: 1, origin: remote(20), created: square(10) }),
      call('appearanceEpoch', [1]),
      emit({ type: 'annotation.created', pageObjectNumber: 1, origin: remote(21), created: textNote(11, { inReplyTo: objRef(10), contents: 'hi' }) }),
      emit({ type: 'annotation.updated', pageObjectNumber: 1, origin: remote(22), updated: square(1, { color: { r: 1, g: 2, b: 3 } }), appearance: { changed: true } }),
      emit({ type: 'annotation.updated', pageObjectNumber: 1, origin: remote(23), updated: square(2, { rect: { left: 0, bottom: 0, right: 10, top: 10 } }), appearance: { changed: false } }),
      emit({ type: 'annotation.moved', pageObjectNumber: 1, origin: remote(24), moved: [square(2, { index: 9 }), square(1, { index: 8 })] }),
      emit({ type: 'annotation.created', pageObjectNumber: 5, origin: remote(25), created: square(12, { pageObjectNumber: 5 }) }),
      call('appearanceEpoch', [1]),
      emit({ type: 'annotation.deleted', pageObjectNumber: 1, origin: remote(26), deleted: { kind: 'objectNumber', value: 2 } }),
      emit({ type: 'annotation.deleted', pageObjectNumber: 1, origin: remote(27), deleted: { kind: 'nm', value: 'n4' } }),
      emit({ type: 'annotation.deleted', pageObjectNumber: 1, origin: remote(28), deleted: { kind: 'objectNumber', value: 99 } }),
      emit({ type: 'annotation.deleted', pageObjectNumber: 1, origin: remote(29), deleted: null }),
      emit({ type: 'annotation.updated', pageObjectNumber: 1, origin: LOCAL, updated: square(1, { contents: 'mine' }), appearance: { changed: true } }),
      emit({ type: 'annotation.created', pageObjectNumber: 1, created: square(13) }),
      emit({ type: 'annotation.flattened', pageObjectNumber: 1, origin: remote(30) }),
      call('list', [1]),
    ],
  },
  {
    name: 'direct-delivery',
    covers: 'deliverRemoteAnnotationEvent applies at once outside a window; an event type it does not know is ignored.',
    plugins: PLUGINS,
    engine: { documents: { a: doc([hydrate({ 1: [square(1)] })]) } },
    snapshot: SNAP,
    steps: [
      ...boot,
      call('deliverRemoteAnnotationEvent', [{ type: 'annotation.created', pageObjectNumber: 1, origin: remote(1), created: square(3) }]),
      call('deliverRemoteAnnotationEvent', [{ type: 'annotation.other', pageObjectNumber: 1 }]),
      call('list', [1]),
    ],
  },
  {
    name: 'rehydrate-chains-after-the-flight',
    covers:
      'A rehydrate while one is in flight flags a re-run and returns the running promise; the finally clears it and starts the flagged run, which ingests with bumpAp. The chain settles three turns after its catch, and a stream.desynced event is a rehydrate.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([
          { path: 'annotations.listRawAll', times: 1, value: snapshot(seed) },
          { path: 'annotations.listRawAll', times: 1, defer: 'second' },
          { path: 'annotations.listRawAll', defer: 'third' },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      emit({ type: 'stream.desynced', reason: 'backlog-overflow', ts: 1 }),
      call('rehydrate'),
      call('hydration'),
      settle('second', snapshot({ 1: [square(1, { contents: 'second' })] })),
      call('hydration'),
      settle('third', snapshot({ 1: [square(1, { contents: 'third' })] })),
      call('hydration'),
      call('list', [1]),
      call('rehydrate'),
    ],
    allowUnmatched: false,
  },
  {
    name: 'page-membership-events',
    covers:
      'pages.deleted drops the deleted pages’ annotations (nothing when none match); pages.inserted reloads each new page, which is a no-op for a page the layout does not know yet. Both are origin-agnostic.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([
          hydrate(seed),
          { path: 'page.annotations.list', pon: 2, value: { annotations: [circle(4, { pageObjectNumber: 2 })] } },
          { path: 'pages.list', value: { pageCount: 2, pages: PAGES } },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      emit({ type: 'pages.deleted', pageObjectNumbers: [2], origin: LOCAL }),
      emit({ type: 'pages.deleted', pageObjectNumbers: [8], origin: remote(3) }),
      emit({ type: 'pages.inserted', insertedPageObjectNumbers: [2, 7], origin: remote(4) }),
      call('list', [2]),
    ],
  },
  {
    name: 'cross-plane-events',
    covers:
      'redaction.applied reloads each applied page once (skipped results do not); form.valueChanged and form.effectsApplied bump the appearance version of the repainted widgets (object number > 0 only).',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([
          hydrate({ 1: [square(1), widget(5, 'text')] }),
          { path: 'page.annotations.list', pon: 1, value: { annotations: [square(1)] } },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      emit({ type: 'form.valueChanged', changedWidgets: [{ annotObjectNumber: 5, pageObjectNumber: 1 }, { annotObjectNumber: 0, pageObjectNumber: 1 }], origin: LOCAL }),
      emit({ type: 'form.effectsApplied', changedWidgets: [{ annotObjectNumber: 5, pageObjectNumber: 1 }], origin: remote(2) }),
      call('appearanceEpoch', [1]),
      emit({
        type: 'redaction.applied',
        origin: LOCAL,
        results: [
          { status: 'applied', pageObjectNumber: 1 },
          { status: 'applied', pageObjectNumber: 1 },
          { status: 'skipped', pageObjectNumber: 2 },
        ],
      }),
      emit({ type: 'redaction.applied', origin: LOCAL, results: [{ status: 'skipped', pageObjectNumber: 1 }] }),
      call('list', [1]),
    ],
  },
  {
    name: 'reload-page',
    covers:
      'reloadPage replaces the page’s annotations with a fresh list, leaves other pages alone, keeps the previous view when the list fails, and does nothing for a page without a crop.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([
          hydrate(seed),
          { path: 'page.annotations.list', pon: 1, times: 1, value: { annotations: [square(2, { contents: 'fresh' }), link(6)] } },
          { path: 'page.annotations.list', pon: 1, error: { name: 'EngineError', message: 'gone', code: 'NotFound' } },
          { path: 'page.annotations.list', pon: 2, value: { annotations: [] } },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      call('reloadPage', [1]),
      call('list', [1]),
      call('reloadPage', [1]),
      call('reloadPage', [2]),
      call('reloadPage', [42]),
      call('list', [1]),
      call('list', [2]),
    ],
  },
  {
    name: 'closing-unsubscribes',
    covers: 'The effect’s event subscription is released when the document closes.',
    plugins: PLUGINS,
    engine: { documents: { a: doc([hydrate(seed)]) } },
    snapshot: 'none',
    steps: [...boot, { op: 'call', target: 'documents', method: 'close', args: ['a'] }],
  },
  {
    name: 'rotated-page-layout',
    covers: 'Ingest reads each page’s own crop: an offset crop moves the content box.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([hydrate({ 3: [square(1, { pageObjectNumber: 3, rect: { left: 60, bottom: 70, right: 90, top: 100 } })] })], {
          pages: [layout(0, 3, { left: 50, bottom: 50, right: 250, top: 350 }, 90)],
        }),
      },
    },
    snapshot: SNAP,
    steps: [...boot, call('list', [3]), call('toContentBox', [3, { left: 60, bottom: 70, right: 90, top: 100 }]), call('toContentBox', [4, { left: 0, bottom: 0, right: 1, top: 1 }])],
  },
];

void id;

export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/annotation-hydration.json',
    describes: 'packages/plugin/annotation: whole-document hydration, the hydration window, and the document event stream.',
    cases,
  },
];
