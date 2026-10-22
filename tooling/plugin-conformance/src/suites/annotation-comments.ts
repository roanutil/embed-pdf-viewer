import { LIVE_ANSWERS } from './annotation-live-answers';
/** Current comments, attached links, script commits, resources and appearance ownership. */
import type { Suite } from '../vectors';
import {
  scenario,
  call,
  square,
  circle,
  textNote,
  markup,
  line,
  fileAttachment,
  unsupported,
  objRef,
  pr,
  hydrate,
  doc,
  write,
  materialize,
  settle,
  emit,
  createdFact,
  layout,
  PLUGINS,
} from './annotation-live-fixtures';
import './annotation-live-inputs';
const at = (day: number) => `2024-01-${String(day).padStart(2, '0')}T00:00:00Z`;
const thread = () => [
  square(1, {
    contents: 'root',
    createdAt: at(1),
    rect: { x: 100, y: 200, width: 100, height: 100 },
    box: { x: 100, y: 200, width: 100, height: 100 },
  }),
  square(2, {
    contents: 'higher',
    createdAt: at(2),
    rect: { x: 100, y: 50, width: 100, height: 100 },
    box: { x: 100, y: 50, width: 100, height: 100 },
  }),
  square(3, {
    contents: 'same top, later',
    createdAt: at(3),
    rect: { x: 300, y: 50, width: 100, height: 100 },
    box: { x: 300, y: 50, width: 100, height: 100 },
  }),
  textNote(4, { reply: { to: objRef(1), type: 'reply' }, contents: 'a reply', createdAt: at(4) }),
  markup('strikeout', 5, { reply: { to: objRef(1), type: 'group' } }),
  textNote(6, {
    reply: { to: objRef(1), type: 'reply' },
    state: 'accepted',
    stateModel: 'review',
    userId: 'me',
    modifiedAt: at(5),
    hidden: true,
  }),
  textNote(7, {
    reply: { to: objRef(1), type: 'reply' },
    state: 'rejected',
    stateModel: 'review',
    userId: 'them',
    modifiedAt: at(6),
    hidden: true,
  }),
];
const page2 = () =>
  circle(8, {
    page: pr(2),
    contents: 'page two',
    rect: { x: 10, y: 450, width: 40, height: 40 },
    box: { x: 10, y: 450, width: 40, height: 40 },
  });
const select = (n: number) => call('selection.set', [[objRef(n)]]);
const uri = (suffix: string) => ({ kind: 'uri' as const, uri: `https://${suffix}` });
const actions = [...PLUGINS, { factory: 'annotation-live-actions' }];
const noSnapshot = {
  snapshot: 'none' as const,
  reason: 'Current outcomes, settlements, engine calls and callbacks carry the observable result.',
};
export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/annotation-comments.json',
    describes: 'Current thread, link and script/resource/appearance boundaries.',
    cases: [
      scenario(
        'threads-and-members',
        'Threads sort by page, page-space y and creation date; every reply/group/status member resolves its root.',
        [
          call('comments.listThreads'),
          ...[4, 5, 7, 99].map((n) => call('comments.getThread', [objRef(n)])),
          call('getStatus'),
        ],
        [hydrate([...thread(), page2()]), ...LIVE_ANSWERS['threads-and-members']],
        {
          engine: {
            documents: {
              a: doc([hydrate([...thread(), page2()]), ...LIVE_ANSWERS['threads-and-members']], {
                security: { allows: '*', identity: { userId: 'me' } },
              }),
            },
          },
        },
      ),
      scenario(
        'threads-nan-top',
        'An explicitly malformed NaN page-space y compares as a tie without panicking; engine protocol input is tagged.',
        [call('comments.listThreads')],
        [
          hydrate(
            [10, 20, { $number: 'NaN' }, 30].map((y, i) =>
              square(i + 1, {
                contents: `top ${typeof y === 'number' ? y : 'NaN'}`,
                rect: { x: 100, y: y as number, width: 100, height: 10 },
              }),
            ),
          ),
          ...LIVE_ANSWERS['threads-nan-top'],
        ],
      ),
      scenario(
        'threads-off-the-layout',
        'Off-layout pages sort last, including after a current remote creation and mirror refresh.',
        [
          emit(createdFact(square(9, { contents: 'z' }))),
          call('refresh'),
          call('comments.listThreads'),
        ],
        [
          hydrate([
            square(1, { contents: 'a' }),
            square(2, { contents: 'tie one' }),
            square(3, { contents: 'tie two' }),
            square(4, { page: pr(9), contents: 'lost page' }),
            square(5, { page: pr(8), contents: 'lost page too' }),
            page2(),
          ]),
          ...LIVE_ANSWERS['threads-off-the-layout'],
        ],
      ),
      scenario(
        'reply-edit-status-marked',
        'Replies target the root; text updates and derived measurement rejection are current; review states chain to the current reviewer and marked states target the root.',
        [
          call('comments.onThreadChanged', [{ $fn: 'threadChanged' }]),
          call('comments.reply', [objRef(5), 'thanks']),
          call('comments.setText', [objRef(4), 'edited']),
          call('comments.setText', [objRef(99), 'nope']),
          call('comments.setText', [objRef(9), 'derived']),
          call('comments.setStatus', [objRef(4), 'completed']),
          call('comments.setMarked', [objRef(1), true]),
          call('comments.getThread', [objRef(1)]),
          call('comments.setMarked', [objRef(1), false]),
          call('comments.reply', [objRef(99), 'lost']),
          call('comments.getThread', [objRef(1)]),
        ],
        [
          hydrate([...thread(), line(9, { intent: 'line-dimension' })]),
          ...LIVE_ANSWERS['reply-edit-status-marked'],
        ],
        {
          engine: {
            documents: {
              a: doc(
                [
                  hydrate([...thread(), line(9, { intent: 'line-dimension' })]),
                  ...LIVE_ANSWERS['reply-edit-status-marked'],
                ],
                { security: { allows: '*', identity: { userId: 'me' } } },
              ),
            },
          },
        },
      ),
      scenario(
        'status-without-a-previous-one',
        'Without caller identity, review status replies to the root.',
        [call('comments.setStatus', [objRef(1), 'none']), call('comments.getThread', [objRef(1)])],
        [hydrate(thread()), ...LIVE_ANSWERS['status-without-a-previous-one']],
      ),
      scenario(
        'remove-and-remove-thread-atomically',
        'Delete preflight blocks the whole thread; permitted root deletion is atomic, and an engine refusal preserves every member.',
        [
          call('comments.onThreadChanged', [{ $fn: 'threadChanged' }]),
          call('comments.delete', [objRef(3)]),
          call('comments.deleteThread', [objRef(11)]),
          call('comments.deleteThread', [objRef(4)]),
          call('comments.deleteThread', [objRef(15)]),
          call('comments.deleteThread', [objRef(12)]),
          call('comments.deleteThread', [objRef(99)]),
          call('comments.listThreads'),
          call('list'),
        ],
        [
          hydrate([
            ...thread(),
            square(10, { contents: 'locked root' }),
            textNote(11, { reply: { to: objRef(10), type: 'reply' }, locked: true }),
            square(12, { contents: 'racy' }),
            square(15, { contents: 'visible thread' }),
            textNote(16, { reply: { to: objRef(15), type: 'reply' } }),
            textNote(13, { reply: { to: objRef(12), type: 'reply' }, contents: 'r1' }),
            textNote(14, { reply: { to: objRef(12), type: 'reply' }, contents: 'r2' }),
          ]),
          ...LIVE_ANSWERS['remove-and-remove-thread-atomically'],
        ],
      ),
      scenario(
        'comment-permissions',
        'Named current can* members expose create, text and whole-thread delete authority, including locked contents, dimension and unknown refs.',
        [
          ...[1, 2, 3, 4, 99].flatMap((n) =>
            [
              'canReply',
              'canSetText',
              'canSetStatus',
              'canSetMarked',
              'canDelete',
              'canDeleteThread',
            ].map((method) => call(`comments.${method}`, [objRef(n)])),
          ),
        ],
        [
          hydrate([
            square(1, { lockedContents: true }),
            textNote(2, { reply: { to: objRef(1), type: 'reply' }, locked: true }),
            square(3),
            line(4, { intent: 'line-dimension' }),
          ]),
          ...LIVE_ANSWERS['comment-permissions'],
        ],
        {
          engine: {
            documents: {
              a: doc(
                [
                  hydrate([
                    square(1, { lockedContents: true }),
                    textNote(2, { reply: { to: objRef(1), type: 'reply' }, locked: true }),
                    square(3),
                    line(4, { intent: 'line-dimension' }),
                  ]),
                  ...LIVE_ANSWERS['comment-permissions'],
                ],
                { security: { allows: ['doc.annotate.read', 'doc.annotate.modify'] } },
              ),
            },
          },
        },
      ),
      scenario(
        'attached-links',
        'Current links set/create, retarget, preserve read-only targets and clear the attached group child; a link annotation and missing parents are read without mutation.',
        [
          call('links.get', [objRef(1)]),
          call('links.set', [objRef(1), uri('a')]),
          call('links.get', [objRef(1)]),
          call('links.set', [objRef(1), uri('b')]),
          call('links.set', [objRef(1), { kind: 'javascript' }]),
          call('links.get', [objRef(1)]),
          call('links.clear', [objRef(1)]),
          call('links.get', [objRef(1)]),
          call('links.get', [objRef(9)]),
          call('links.get', [objRef(99)]),
          call('links.get', [objRef(10)]),
          call('links.set', [objRef(9), uri('x')]),
          call('links.set', [objRef(99), uri('x')]),
          call('listLinkItems', [pr(1)]),
          call('list'),
        ],
        [
          hydrate([
            square(1),
            materialize(9, {
              subtype: 'link',
              rect: { x: 100, y: 100, width: 100, height: 100 },
              target: uri('own'),
            }),
            materialize(10, {
              subtype: 'link',
              rect: { x: 100, y: 100, width: 100, height: 100 },
              target: null,
            }),
          ]),
          ...LIVE_ANSWERS['attached-links'],
        ],
      ),
      scenario(
        'links-follow-geometry-and-props',
        'A parent move writes its link child in the same Change; selected markup creates one child per quad, and an intended child refusal rolls back the whole link change.',
        [
          select(1),
          call('editPointer', ['down', pr(1), { x: 150, y: 150 }, false]),
          call('editPointer', ['move', pr(1), { x: 160, y: 150 }, false]),
          call('editPointer', ['up', pr(1), { x: 160, y: 150 }, false]),
          select(2),
          call('selection.updateLink', [uri('m')]),
          select(3),
          call('selection.updateLink', [uri('c')]),
          call('list'),
        ],
        [
          hydrate([
            square(1),
            materialize(20, {
              subtype: 'link',
              rect: { x: 100, y: 100, width: 100, height: 100 },
              target: uri('a'),
              reply: { to: objRef(1), type: 'group' },
            }),
            markup('highlight', 2, {
              quadPoints: [
                {
                  upperLeft: { x: 50, y: 55 },
                  upperRight: { x: 200, y: 55 },
                  lowerLeft: { x: 50, y: 70 },
                  lowerRight: { x: 200, y: 70 },
                },
                {
                  upperLeft: { x: 50, y: 85 },
                  upperRight: { x: 120, y: 85 },
                  lowerLeft: { x: 50, y: 100 },
                  lowerRight: { x: 120, y: 100 },
                },
              ],
            }),
            circle(3),
          ]),
          ...LIVE_ANSWERS['links-follow-geometry-and-props'],
        ],
      ),
      scenario(
        'rapid-link-sets-use-pending-identity',
        'Two sets use the same stable-born pending child; both writes name the same child before ordered confirmations.',
        [
          call('links.set', [objRef(1), uri('1')]),
          call('links.set', [objRef(1), uri('2')]),
          settle('child'),
          settle('retarget'),
          call('links.get', [objRef(1)]),
          call('list'),
        ],
        [hydrate([square(1)]), ...LIVE_ANSWERS['rapid-link-sets-use-pending-identity']],
      ),
      scenario(
        'link-updates-queue-across-parents',
        'Rapid link changes preserve each parent target and stable child identity while every pending write retains its stated target and ordered confirmations settle them.',
        [
          call('links.set', [objRef(1), uri('1')]),
          call('links.set', [objRef(1), uri('2')]),
          call('links.set', [objRef(1), uri('3')]),
          call('links.set', [objRef(2), uri('b1')]),
          settle('a1'),
          call('links.set', [objRef(1), uri('4')]),
          settle('a2'),
          call('links.set', [objRef(2), uri('b2')]),
          settle('a3'),
          settle('b1'),
          settle('a4'),
          settle('b2'),
          call('links.get', [objRef(1)]),
          call('links.get', [objRef(2)]),
          call('list'),
        ],
        [
          hydrate([
            square(1),
            square(2, {
              box: { x: 300, y: 100, width: 100, height: 100 },
              rect: { x: 300, y: 100, width: 100, height: 100 },
            }),
          ]),
          ...LIVE_ANSWERS['link-updates-queue-across-parents'],
        ],
      ),
      scenario(
        'script-commits',
        'The script sink lowers current loaded fields and colors; an unloaded resolved ref fails staging before apply, while a separate batch exercises unresolved/transparent inputs and stops on an explicit engine refusal.',
        [
          call('commitScriptEffects', [
            [
              {
                annotObjectNumber: 1,
                page: pr(1),
                patch: {
                  strokeColor: ['RGB', 1, 0, 0.5],
                  fillColor: ['T'],
                  opacity: 0.5,
                  width: 3,
                  borderStyle: 'D',
                  dash: [3, 1],
                  rect: [10, 20, 30, 40],
                  contents: 'x',
                  flags: { hidden: true },
                },
              },
              {
                annotObjectNumber: 2,
                patch: {
                  strokeColor: ['G', 0.5],
                  fillColor: ['CMYK', 0.1, 0.2, 0.3, 0.4],
                  borderStyle: 'S',
                },
              },
              { annotObjectNumber: 40, page: pr(2), patch: { contents: 'far' } },
              { annotObjectNumber: 1, patch: { opacity: 1 } },
            ],
          ]),
          call('commitScriptEffects', [
            [
              { annotObjectNumber: 41, page: pr(2), patch: { contents: 'missing' } },
              { annotObjectNumber: 42, patch: { contents: 'no page' } },
              { annotObjectNumber: 44, patch: { contents: 'no loaded record' } },
              { annotObjectNumber: 3, patch: { strokeColor: ['T'] } },
              { annotObjectNumber: 3, patch: { width: 1 } },
              { annotObjectNumber: 2, patch: { opacity: 1 } },
              { annotObjectNumber: 1, patch: { opacity: 1 } },
            ],
          ]),
          call('list'),
        ],
        [
          hydrate([square(1), circle(2), square(3), unsupported(4)]),
          {
            path: 'page.annotations.list',
            pon: 2,
            value: { annotations: [square(39, { page: pr(2) }), square(40, { page: pr(2) })] },
          },
          ...LIVE_ANSWERS['script-commits'],
        ],
      ),
      scenario(
        'script-commits-through-actions',
        'The real actions plugin and annotation plugin connect their current script sink and release it on close.',
        [{ op: 'call', target: 'documents', method: 'close', args: ['a'] }],
        [hydrate([square(1)]), ...LIVE_ANSWERS['script-commits-through-actions']],
        { plugins: actions },
      ),
      scenario(
        'attachment-download',
        'The current resource door reads file bytes by role and rejects an unknown ref.',
        [
          call('downloadResource', [objRef(1), 'file']),
          call('downloadResource', [objRef(7), 'file']),
        ],
        [
          hydrate([fileAttachment(1)]),
          { path: 'page.annotations.downloadResource', pon: 1, value: { $bytes: 'AQID' } },
          ...LIVE_ANSWERS['attachment-download'],
        ],
      ),
      scenario(
        'baked-appearances-through-render',
        'Engine appearances belong to the render plugin; the annotation projection preserves current image/version data and returns an empty list after a render refusal.',
        [
          call('renderAppearances', [pr(1), 2]),
          call('renderAppearances', [pr(1), 3]),
          call('renderAppearances', [pr(42), 1]),
        ],
        [
          hydrate([square(1)]),
          {
            path: 'page.annotations.renderAppearances',
            pon: 1,
            times: 1,
            value: {
              page: pr(1),
              appearances: [
                {
                  ref: objRef(1),
                  mode: 'normal',
                  state: null,
                  rect: { x: 100, y: 100, width: 100, height: 100 },
                  image: {
                    width: 2,
                    height: 2,
                    format: 'png',
                    contentType: 'image/png',
                    source: { kind: 'bytes', bytes: { $bytes: 'AQID' } },
                    blob: {
                      $fn: 'appearance.blob',
                      throws: { name: 'Error', message: 'projection does not request bytes' },
                    },
                    objectUrl: {
                      $fn: 'appearance.url',
                      throws: { name: 'Error', message: 'projection does not request a URL' },
                    },
                  },
                },
              ],
            },
          },
          {
            path: 'page.annotations.renderAppearances',
            pon: 1,
            error: {
              name: 'EngineError',
              message: 'intended appearance refusal',
              code: 'Internal',
            },
          },
          ...LIVE_ANSWERS['baked-appearances-through-render'],
        ],
        { plugins: [...PLUGINS, { factory: 'annotation-live-render' }] },
      ),
      scenario(
        'appearance-scale-owned-by-render',
        'The current render capability snaps appearance scale to the advertised engine lattice.',
        [
          ...[0.5, 1.5, 9].map((scale) => ({
            op: 'call' as const,
            target: { token: 'render', doc: 'a' },
            method: 'getAppearanceScale',
            args: [scale],
          })),
        ],
        [hydrate(), ...LIVE_ANSWERS['appearance-scale-owned-by-render']],
        {
          plugins: [...PLUGINS, { factory: 'annotation-live-render' }],
          engine: {
            documents: {
              a: doc(
                [
                  hydrate(),
                  {
                    path: 'render.getPolicy',
                    value: {
                      kind: 'lattice',
                      fullPage: { widths: [512, 1024] },
                      appearances: { scales: [2, 1, 4] },
                    },
                  },
                  ...LIVE_ANSWERS['appearance-scale-owned-by-render'],
                ],
                { services: ['render'] },
              ),
            },
          },
        },
      ),
    ],
  },
];
