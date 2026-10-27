import { born } from './annotation-live-answers';
/** Group relationships and atomic replace-text creation through the current change queue. */
import type { Suite } from '../vectors';
import {
  scenario,
  call,
  square,
  circle,
  caret,
  objRef,
  pr,
  hydrate,
  write,
  updated,
  refused,
  created,
  fail,
  doc,
  open,
} from './annotation-live-fixtures';
const group = (parent: number) => ({ to: objRef(parent), type: 'group' as const });
const select = (...numbers: number[]) => call('selection.set', [numbers.map((n) => objRef(n))]);
const quad = {
  upperLeft: { x: 10, y: 40 },
  upperRight: { x: 90, y: 40 },
  lowerLeft: { x: 10, y: 55 },
  lowerRight: { x: 90, y: 55 },
};
const anchor = {
  glyphQuad: {
    upperLeft: { x: 85, y: 45 },
    upperRight: { x: 90, y: 45 },
    lowerLeft: { x: 85, y: 57 },
    lowerRight: { x: 90, y: 57 },
  },
  advance: 1,
};
const replace = () => call('createReplaceText', [pr(1), [quad], anchor, 'replace-text']);
// Both entries are born under their reserved refs before one atomic write.
const pair = [
  created(
    born(10001, {
      subtype: 'caret',
      box: { x: 87, y: 51, width: 6, height: 6 },
      rotation: null,
      color: '#ef4444',
      opacity: 1,
      intent: 'replace',
      print: true,
    }),
  ),
  created(
    born(10002, {
      subtype: 'strikeout',
      color: '#ef4444',
      quadPoints: [quad],
      intent: 'strikeout-text-edit',
      print: true,
      reply: group(10001),
    }),
  ),
];
export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/annotation-grouping.json',
    describes: 'Current grouping/ungrouping and atomic Caret + StrikeOut changes.',
    cases: [
      scenario(
        'a-pair-groups',
        'A pair groups with one relationship patch; selection stays group-aware.',
        [
          select(1, 2),
          call('selection.canGroup'),
          call('selection.group'),
          call('selection.list'),
          call('list'),
        ],
        [
          hydrate([square(1), square(2)]),
          write(1, [{ item: updated(square(2, { reply: group(1) })) }]),
        ],
      ),
      scenario(
        'group-and-ungroup',
        'Grouping needs two changeable records on one page; grouping and ungrouping each write all subordinate patches in one atomic Change.',
        [
          call('selection.canGroup'),
          call('selection.group'),
          select(1),
          call('selection.canGroup'),
          call('selection.group'),
          call('selection.add', [[objRef(9, 2)]]),
          call('selection.canGroup'),
          call('selection.group'),
          select(3, 2, 1),
          call('selection.canGroup'),
          call('selection.canUngroup'),
          call('selection.group'),
          call('selection.list'),
          call('selection.canGroup'),
          call('selection.canUngroup'),
          call('selection.ungroup'),
          call('selection.canUngroup'),
          call('list'),
        ],
        [
          hydrate([square(1), square(2), circle(3), square(9, { page: pr(2), ref: objRef(9, 2) })]),
          write(1, [
            { item: updated(square(2, { reply: group(1) })) },
            { item: updated(circle(3, { reply: group(1) })) },
          ]),
          write(2, [{ item: updated(square(2)) }, { item: updated(circle(3)) }]),
        ],
      ),
      scenario(
        'group-refused-per-record',
        'Per-record update authority disables grouping and ungrouping while direct writes rely on engine refusal; a separately permitted selection reaches an intended atomic engine refusal.',
        [
          select(3, 1),
          call('selection.canGroup'),
          call('selection.canUngroup'),
          call('selection.group'),
          call('selection.ungroup'),
          call('selection.list'),
          open('b'),
          call('selection.set', [[objRef(1), objRef(3)]], 'b'),
          call('selection.canGroup', [], 'b'),
          call('selection.group', [], 'b'),
          call('selection.list', [], 'b'),
        ],
        [],
        {
          engine: {
            documents: {
              a: doc(
                [
                  hydrate([square(1), square(2, { reply: group(1) }), square(3)]),
                  refused('intended denied group'),
                  refused('intended denied ungroup'),
                ],
                { security: { allows: '*', allowsAnnotation: { update: false, delete: true } } },
              ),
              b: doc([hydrate([square(1), square(3)]), refused('intended group refusal')]),
            },
          },
        },
      ),
      scenario(
        'replace-text-creates-a-group',
        'One Change creates the Caret primary and StrikeOut subordinate under stable born refs; both stay selected.',
        [replace(), call('selection.list'), call('list')],
        [
          hydrate(),
          write(
            1,
            pair.map((item) => ({ item })),
          ),
        ],
      ),
      scenario(
        'replace-text-refusal-is-atomic',
        'An engine refusal removes both pending parts together; no compensation delete runs. A held refused primary change clears both parts; the host route lets the engine refuse an unknown page.',
        [
          replace(),
          call('list'),
          replace(),
          fail('pair', 'intended pair refusal'),
          call('list'),
          call('createReplaceText', [pr(99), [quad], anchor]),
          call('list'),
        ],
        [
          hydrate(),
          refused('intended atomic pair refusal'),
          { path: 'apply', times: 1, defer: 'pair' },
          refused('intended unknown replace page'),
        ],
      ),
    ],
  },
];
