/**
 * `packages/plugin/annotation`: grouping (`/IRT` + `/RT /Group`), and the
 * grouped create the Replace Text gesture commits (Caret primary, StrikeOut
 * subordinate) with its compensating rollback. Replayed by
 * `crates/plugins/annotation/tests/conformance.rs`.
 */
import type { ScenarioCase } from '../scenario';
import type { Suite } from '../vectors';

import {
  PLUGINS,
  SNAP,
  boot,
  call,
  caret,
  circle,
  created,
  doc,
  fail,
  hydrate,
  markup,
  objRef,
  settle,
  square,
  updated,
} from './annotation-fixtures';

/** Why these cases record no state; see `ScenarioCase` in `../scenario`. */
const UNSNAPSHOTTED = {
  snapshot: 'none',
  reason: "Pins the authority refusal and the engine refusal of a group write, through call outcomes and settlements. Its dispatched actions are compared; the state they build is not.",
} as const;

const tq = (x: number, y: number, w: number, h: number) => ({
  upperStart: { x, y },
  upperEnd: { x: x + w, y },
  lowerStart: { x, y: y + h },
  lowerEnd: { x: x + w, y: y + h },
});
const anchor = { glyphQuad: tq(85, 45, 5, 12), advance: 1 };

const grouped = (n: number, parent: number) => ({ inReplyTo: objRef(parent), replyType: 'group' });

const cases: ScenarioCase[] = [
  {
    name: 'a-pair-groups',
    covers: 'Two committed members on one page are enough: canGroup says so and group writes the one subordinate.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([
          hydrate({ 1: [square(1), square(2)] }),
          { path: 'page.annotations.update', pon: 1, args: [objRef(2), { subtype: 'square', inReplyTo: objRef(1), replyType: 'group' }], value: updated(square(2, grouped(2, 1))) },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [...boot, call('select', [objRef(1)]), call('select', [objRef(2), { add: true }]), call('canGroup'), call('group')],
  },
  {
    name: 'group-and-ungroup',
    covers:
      'canGroup needs 2+ committed members on one page, each passing the update check, and not already one complete group. group makes the bottom-most member the primary and writes /IRT + /RT /Group onto the rest (a relationship-only patch, source preserved); ungroup clears it on every subordinate of the touched groups.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([
          hydrate({ 1: [square(1), square(2), circle(3)], 2: [square(9, { pageObjectNumber: 2 })] }),
          { path: 'page.annotations.update', pon: 1, args: [objRef(2), { subtype: 'square', inReplyTo: objRef(1), replyType: 'group' }], value: updated(square(2, grouped(2, 1))) },
          { path: 'page.annotations.update', pon: 1, args: [objRef(3), { subtype: 'circle', inReplyTo: objRef(1), replyType: 'group' }], value: updated(circle(3, grouped(3, 1))) },
          { path: 'page.annotations.update', pon: 1, args: [objRef(2), { subtype: 'square', inReplyTo: null }], value: updated(square(2)) },
          { path: 'page.annotations.update', pon: 1, args: [objRef(3), { subtype: 'circle', inReplyTo: null }], value: updated(circle(3)) },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      call('canGroup'),
      call('group'),
      call('select', [objRef(1)]),
      call('canGroup'),
      call('group'),
      call('select', [objRef(9), { add: true }]),
      call('canGroup'),
      call('group'),
      call('select', [objRef(3)]),
      call('select', [objRef(2), { add: true }]),
      call('select', [objRef(1), { add: true }]),
      call('canGroup'),
      call('canUngroup'),
      call('group'),
      call('selection'),
      call('canGroup'),
      call('canUngroup'),
      call('ungroup'),
      call('canUngroup'),
      call('list', [1]),
    ],
  },
  {
    name: 'group-refused-per-record',
    covers: 'Without update authority neither verb is offered; the engine’s refusal of a group write rejects group().',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc(
          [
            hydrate({ 1: [square(1, { userId: 'u1', groupId: 'g1' }), square(2, { ...grouped(2, 1), userId: 'u2' }), square(3, { groupId: 'g2' })] }),
            { path: 'page.annotations.update', pon: 1, error: { name: 'EngineError', message: 'nope', code: 'Refused' } },
          ],
          { security: { allows: '*', allowsAnnotationMutation: { update: false, delete: true } } },
        ),
      },
    },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      call('select', [objRef(3)]),
      call('select', [objRef(1), { add: true }]),
      call('canGroup'),
      call('canUngroup'),
      call('group'),
      call('ungroup'),
    ],
  },
  {
    name: 'replace-text-creates-a-group',
    covers:
      'createReplaceText commits the Caret first, then the StrikeOut with /IRT to the created Caret and /RT /Group, reconciling each temp id and syncing each DTO as vector; both stay selected.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([
          hydrate(),
          { path: 'page.annotations.create', pon: 1, times: 1, value: created(caret(10, { intent: 'replace' })) },
          { path: 'page.annotations.create', pon: 1, times: 1, value: created(markup('strikeout', 11, { intent: 'strikeout-text-edit', ...grouped(11, 10) })) },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [...boot, call('createReplaceText', [1, [tq(10, 40, 80, 15)], anchor, 'replace-text']), call('selection')],
  },
  {
    name: 'replace-text-rolls-back',
    covers:
      'A refused StrikeOut create deletes the committed Caret (compensation in reverse) and removes both parts; a Caret whose rollback delete also fails stays visible. A refused primary removes both temp parts. A group on a page without a crop never reaches the engine.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([
          hydrate(),
          { path: 'page.annotations.create', pon: 1, times: 1, value: created(caret(10, { intent: 'replace' })) },
          { path: 'page.annotations.create', pon: 1, times: 1, error: { name: 'EngineError', message: 'strikeout failed', code: 'Refused' } },
          { path: 'page.annotations.create', pon: 1, times: 1, value: created(caret(12, { intent: 'replace' })) },
          { path: 'page.annotations.create', pon: 1, times: 1, error: { name: 'EngineError', message: 'strikeout failed again', code: 'Refused' } },
          { path: 'page.annotations.create', pon: 1, times: 1, defer: 'primary' },
          { path: 'page.annotations.delete', pon: 1, times: 1, value: {} },
          { path: 'page.annotations.delete', pon: 1, error: { name: 'EngineError', message: 'cannot delete', code: 'Refused' } },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      call('createReplaceText', [1, [tq(10, 40, 80, 15)], anchor, 'replace-text']),
      call('list', [1]),
      call('createReplaceText', [1, [tq(10, 80, 80, 15)], anchor]),
      call('list', [1]),
      call('createReplaceText', [1, [tq(10, 120, 80, 15)], anchor]),
      fail('primary', 'caret failed'),
      call('createReplaceText', [99, [tq(10, 120, 80, 15)], anchor]),
      call('list', [1]),
    ],
  },
];

void settle;

export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/annotation-grouping.json',
    describes: 'packages/plugin/annotation: group, ungroup, their twins, and the grouped Replace Text create with rollback.',
    cases,
  },
];
