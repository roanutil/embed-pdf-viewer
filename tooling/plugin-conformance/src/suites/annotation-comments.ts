/**
 * `packages/plugin/annotation`: the conversation plane (`comments`), the
 * attached-link lens (`links`), the actions plane's script commit door, the
 * attachment download and the baked appearances. Replayed by
 * `crates/plugins/annotation/tests/conformance.rs`.
 */
import type { ScenarioCase } from '../scenario';
import type { Suite } from '../vectors';

import {
  FLAGS,
  PAGES,
  PLUGINS,
  SNAP,
  WITH_LENS,
  boot,
  call,
  circle,
  created,
  doc,
  fail,
  hydrate,
  lens,
  link,
  markup,
  objRef,
  settle,
  square,
  textNote,
  updated,
  widget,
  layout,
  base,
  pr,
} from './annotation-fixtures';

const at = (d: number) => `2024-01-${String(d).padStart(2, '0')}T00:00:00Z`;
const me = { identity: { user_id: 'me' } };

/** A root with a reply, a grouped part and two review states, and a second
 *  root higher on the page, and one on page 2. */
const thread = () => [
  square(1, { contents: 'root', created: at(1), rect: { left: 100, bottom: 500, right: 200, top: 600 } }),
  square(2, { contents: 'higher', created: at(2), rect: { left: 100, bottom: 650, right: 200, top: 750 } }),
  square(3, { contents: 'same top, later', created: at(3), rect: { left: 300, bottom: 650, right: 400, top: 750 } }),
  textNote(4, { inReplyTo: objRef(1), replyType: 'reply', contents: 'a reply', created: at(4) }),
  markup('strikeout', 5, { inReplyTo: objRef(1), replyType: 'group' }),
  textNote(6, { inReplyTo: objRef(1), state: 'accepted', stateModel: 'review', userId: 'me', modified: at(5), flags: FLAGS({ hidden: true }) }),
  textNote(7, { inReplyTo: objRef(1), state: 'rejected', stateModel: 'review', userId: 'them', modified: at(6), flags: FLAGS({ hidden: true }) }),
];
const page2 = () => [circle(8, { pageObjectNumber: 2, contents: 'page two', rect: { left: 20, bottom: 30, right: 60, top: 70 } })];

const cases: ScenarioCase[] = [
  {
    name: 'threads-and-members',
    covers:
      'threads() are display-ordered (page order, then top of page, then creation); thread(ref) finds the thread of any member (root, reply, grouped part, status) and null for none. The lens reads hydration too.',
    plugins: WITH_LENS,
    engine: { documents: { a: doc([hydrate({ 2: page2(), 1: thread() })], { security: { allows: '*', ...me } }) } },
    snapshot: 'none',
    steps: [
      ...boot,
      lens('threads'),
      lens('thread', [objRef(4)]),
      lens('thread', [objRef(5)]),
      lens('thread', [objRef(7)]),
      lens('thread', [objRef(99)]),
      lens('hydration'),
    ],
  },
  {
    name: 'threads-off-the-layout',
    covers: 'A thread on a page the layout does not list sorts last.',
    plugins: WITH_LENS,
    engine: {
      documents: {
        a: doc(
          [
            {
              path: 'annotations.listRawAll',
              value: {
                pages: [
                  {
                    pageState: { page: pr(1) },
                    annotations: [
                      square(1, { contents: 'a' }),
                      square(2, { contents: 'tie one' }),
                      square(3, { contents: 'tie two' }),
                      square(4, { contents: 'lost page', pageObjectNumber: 9, ref: objRef(4, 9) }),
                      square(5, { contents: 'lost page too', pageObjectNumber: 8, ref: objRef(5, 8) }),
                    ],
                  },
                  { pageState: { page: pr(2) }, annotations: page2() },
                ],
              },
            },
          ],
          { pages: [layout(0, 1), layout(1, 2)] },
        ),
      },
    },
    snapshot: 'none',
    steps: [
      ...boot,
      call('deliverRemoteAnnotationEvent', [{ type: 'annotation.moved', pageObjectNumber: 1, origin: { kind: 'remote', serverId: 1 }, moved: [square(9, { pageObjectNumber: 1, contents: 'z' })] }]),
      call('rehydrate'),
      lens('threads'),
    ],
  },
  {
    name: 'reply-edit-status-marked',
    covers:
      'reply writes a flat /IRT to the root whatever member was passed, with the reply flags; edit patches contents through the one update path; setStatus chains to the caller’s previous status (else the root); setMarked writes the Marked model. Each create syncs baked and resolves after the engine confirms. An unknown ref rejects.',
    plugins: WITH_LENS,
    engine: {
      documents: {
        a: doc(
          [
            hydrate({ 1: thread() }),
            { path: 'page.annotations.create', pon: 1, times: 1, value: created(textNote(20, { inReplyTo: objRef(1), contents: 'thanks' })) },
            { path: 'page.annotations.create', pon: 1, times: 1, value: created(textNote(21, { inReplyTo: objRef(6), state: 'completed', stateModel: 'review', userId: 'me' })) },
            { path: 'page.annotations.create', pon: 1, times: 1, value: created(textNote(22, { inReplyTo: objRef(1), state: 'marked', stateModel: 'marked' })) },
            { path: 'page.annotations.create', pon: 1, times: 1, value: created(textNote(23, { inReplyTo: objRef(1), state: 'unmarked', stateModel: 'marked' })) },
            { path: 'page.annotations.update', pon: 1, value: updated(textNote(4, { inReplyTo: objRef(1), contents: 'edited' })) },
          ],
          { security: { allows: '*', ...me } },
        ),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      lens('reply', [objRef(5), 'thanks']),
      lens('edit', [objRef(4), 'edited']),
      lens('edit', [objRef(99), 'nope']),
      lens('setStatus', [objRef(4), 'completed']),
      lens('setMarked', [objRef(1), true]),
      lens('setMarked', [objRef(1), false]),
      lens('reply', [objRef(99), 'lost']),
      lens('thread', [objRef(1)]),
    ],
  },
  {
    name: 'status-without-a-previous-one',
    covers: 'Without a session identity, or without a previous status of the caller’s, setStatus replies to the root.',
    plugins: WITH_LENS,
    engine: {
      documents: {
        a: doc([
          hydrate({ 1: thread() }),
          { path: 'page.annotations.create', pon: 1, value: created(textNote(30, { inReplyTo: objRef(1), state: 'none', stateModel: 'review' })) },
        ]),
      },
    },
    snapshot: 'none',
    steps: [...boot, lens('setStatus', [objRef(1), 'none'])],
  },
  {
    name: 'remove-and-remove-thread',
    covers:
      'remove deletes one annotation. removeThread deletes children first, root last, one at a time; a member failing the delete gate blocks the whole thread up front (every blocked ref listed, nothing deleted); a mid-flight refusal stops the cascade before the root.',
    plugins: WITH_LENS,
    engine: {
      documents: {
        a: doc([
          hydrate({
            1: [
              ...thread(),
              square(10, { contents: 'locked root' }),
              textNote(11, { inReplyTo: objRef(10), flags: FLAGS({ locked: true }) }),
              square(12, { contents: 'racy' }),
              textNote(13, { inReplyTo: objRef(12), contents: 'r1' }),
              textNote(14, { inReplyTo: objRef(12), contents: 'r2' }),
            ],
          }),
          { path: 'page.annotations.delete', pon: 1, args: [objRef(14)], error: { name: 'EngineError', message: 'already gone', code: 'NotFound' } },
          { path: 'page.annotations.delete', pon: 1, value: {} },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      lens('remove', [objRef(3)]),
      lens('removeThread', [objRef(11)]),
      lens('removeThread', [objRef(4)]),
      lens('removeThread', [objRef(12)]),
      lens('removeThread', [objRef(99)]),
      lens('threads'),
    ],
  },
  {
    name: 'comment-permissions',
    covers:
      'permissionsFor: reply and status are create authority; text edits are gated by lockedContents, deletes by locked; the thread gate covers every member. Without create authority neither verb is offered; an unknown ref can do nothing but reply.',
    plugins: WITH_LENS,
    engine: {
      documents: {
        a: doc(
          [
            hydrate({
              1: [
                square(1, { flags: FLAGS({ lockedContents: true }) }),
                textNote(2, { inReplyTo: objRef(1), flags: FLAGS({ locked: true }) }),
                square(3),
              ],
            }),
          ],
          { security: { allows: ['doc.annotate.read', 'doc.annotate.modify'] } },
        ),
      },
    },
    snapshot: 'none',
    steps: [
      ...boot,
      lens('permissionsFor', [objRef(1)]),
      lens('permissionsFor', [objRef(2)]),
      lens('permissionsFor', [objRef(3)]),
      lens('permissionsFor', [objRef(99)]),
    ],
  },
  {
    name: 'attached-links',
    covers:
      'links.set on a committed parent creates the /RT /Group link child over its footprint and resolves once it is committed, after which links.of reads it; a second set retargets the child in place; links.clear deletes it. A read-only target is carried, never written. of on a link reads its own target, on an unknown ref null.',
    plugins: WITH_LENS,
    engine: {
      documents: {
        a: doc([
          hydrate({ 1: [square(1), link(9, { target: { kind: 'uri', uri: 'https://own' } }), link(10, { target: null })] }),
          { path: 'page.annotations.create', pon: 1, times: 1, value: created(link(20, { inReplyTo: objRef(1), replyType: 'group', target: { kind: 'uri', uri: 'https://a' } })) },
          { path: 'page.annotations.update', pon: 1, times: 1, value: updated(link(20, { inReplyTo: objRef(1), replyType: 'group', target: { kind: 'uri', uri: 'https://b' } })) },
          { path: 'page.annotations.update', pon: 1, times: 1, value: updated(link(20, { inReplyTo: objRef(1), replyType: 'group', target: { kind: 'uri', uri: 'https://b' } })) },
          { path: 'page.annotations.delete', pon: 1, value: {} },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      lens('linksOf', [objRef(1)]),
      lens('linksSet', [objRef(1), { kind: 'uri', uri: 'https://a' }]),
      lens('linksOf', [objRef(1)]),
      lens('linksSet', [objRef(1), { kind: 'uri', uri: 'https://b' }]),
      lens('linksSet', [objRef(1), { kind: 'javascript' }]),
      lens('linksOf', [objRef(1)]),
      lens('linksClear', [objRef(1)]),
      lens('linksOf', [objRef(1)]),
      lens('linksOf', [objRef(9)]),
      lens('linksOf', [objRef(99)]),
      lens('linksOf', [objRef(10)]),
      lens('linksSet', [objRef(9), { kind: 'uri', uri: 'https://x' }]),
      lens('linksSet', [objRef(99), { kind: 'uri', uri: 'https://x' }]),
      call('linkItemsOn', [1]),
    ],
  },
  {
    name: 'links-follow-geometry-and-props',
    covers:
      'A committed geometry patch of a parent with children schedules a keep-sync that re-rects them toward whatever target the substrate holds. updateSelection({ link }) goes through the syncLink effect. A markup parent gets one child per quad. A failing reconcile reports to console.error.',
    plugins: WITH_LENS,
    engine: {
      documents: {
        a: doc([
          hydrate({
            1: [
              square(1),
              link(20, { inReplyTo: objRef(1), replyType: 'group', target: { kind: 'uri', uri: 'https://a' } }),
              markup('highlight', 2, { quadPoints: [markup('x', 0).quadPoints, markup('x', 0, { quadPoints: [] }).quadPoints].flat().concat([{ p1: { x: 50, y: 715 }, p2: { x: 120, y: 715 }, p3: { x: 50, y: 700 }, p4: { x: 120, y: 700 } }]) }),
              circle(3),
            ],
          }),
          { path: 'page.annotations.update', pon: 1, times: 1, value: updated(square(1, { rect: { left: 110, bottom: 600, right: 210, top: 700 } })) },
          { path: 'page.annotations.update', pon: 1, times: 1, value: updated(link(20, { inReplyTo: objRef(1), replyType: 'group' })) },
          { path: 'page.annotations.create', pon: 1, times: 1, value: created(link(21, { inReplyTo: objRef(2), replyType: 'group' })) },
          { path: 'page.annotations.create', pon: 1, times: 1, value: created(link(22, { inReplyTo: objRef(2), replyType: 'group' })) },
          { path: 'page.annotations.create', pon: 1, error: { name: 'EngineError', message: 'no links here', code: 'Refused' } },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      call('select', [objRef(1)]),
      call('editPointer', ['down', 1, { x: 150, y: 150 }, false]),
      call('editPointer', ['move', 1, { x: 160, y: 150 }, false]),
      call('editPointer', ['up', 1, { x: 160, y: 150 }, false]),
      call('select', [objRef(2)]),
      call('updateSelection', [{ link: { kind: 'uri', uri: 'https://m' } }]),
      call('select', [objRef(3)]),
      call('updateSelection', [{ link: { kind: 'uri', uri: 'https://c' } }]),
      call('list', [1]),
    ],
  },
  {
    name: 'rapid-link-sets-chain',
    covers: 'Two sets on one parent chain: the second starts after the first settles, and each reads the model at run time.',
    plugins: WITH_LENS,
    engine: {
      documents: {
        a: doc([
          hydrate({ 1: [square(1)] }),
          { path: 'page.annotations.create', pon: 1, times: 1, defer: 'child' },
          { path: 'page.annotations.update', pon: 1, value: updated(link(20, { inReplyTo: objRef(1), replyType: 'group', target: { kind: 'uri', uri: 'https://2' } })) },
        ]),
      },
    },
    snapshot: 'none',
    steps: [
      ...boot,
      lens('linksSet', [objRef(1), { kind: 'uri', uri: 'https://1' }]),
      lens('linksSet', [objRef(1), { kind: 'uri', uri: 'https://2' }]),
      settle('child', created(link(20, { inReplyTo: objRef(1), replyType: 'group', target: { kind: 'uri', uri: 'https://1' } }))),
      lens('linksOf', [objRef(1)]),
    ],
  },
  {
    name: 'link-sync-chains-per-parent',
    covers:
      'Link syncs chain per parent: a third set on one parent waits for the second, not the first; a set after the first settles still waits for the latest; and when one parent’s chain drains, another parent’s pending chain is kept, so its next set still waits.',
    plugins: WITH_LENS,
    engine: {
      documents: {
        a: doc([
          hydrate({ 1: [square(1), square(2, { rect: { left: 300, bottom: 600, right: 400, top: 700 } })] }),
          { path: 'page.annotations.create', pon: 1, times: 1, defer: 'a1' },
          { path: 'page.annotations.create', pon: 1, times: 1, defer: 'b1' },
          { path: 'page.annotations.create', pon: 1, value: created(link(22, { inReplyTo: objRef(2), replyType: 'group', target: { kind: 'uri', uri: 'https://b2' } })) },
          { path: 'page.annotations.update', pon: 1, times: 1, defer: 'a2' },
          { path: 'page.annotations.update', pon: 1, times: 1, value: updated(link(20, { inReplyTo: objRef(1), replyType: 'group', target: { kind: 'uri', uri: 'https://3' } })) },
          { path: 'page.annotations.update', pon: 1, times: 1, value: updated(link(20, { inReplyTo: objRef(1), replyType: 'group', target: { kind: 'uri', uri: 'https://4' } })) },
          { path: 'page.annotations.update', pon: 1, value: updated(link(21, { inReplyTo: objRef(2), replyType: 'group', target: { kind: 'uri', uri: 'https://b2' } })) },
        ]),
      },
    },
    snapshot: 'none',
    steps: [
      ...boot,
      lens('linksSet', [objRef(1), { kind: 'uri', uri: 'https://1' }]),
      lens('linksSet', [objRef(1), { kind: 'uri', uri: 'https://2' }]),
      lens('linksSet', [objRef(1), { kind: 'uri', uri: 'https://3' }]),
      lens('linksSet', [objRef(2), { kind: 'uri', uri: 'https://b1' }]),
      settle('a1', created(link(20, { inReplyTo: objRef(1), replyType: 'group', target: { kind: 'uri', uri: 'https://1' } }))),
      lens('linksSet', [objRef(1), { kind: 'uri', uri: 'https://4' }]),
      settle('a2', updated(link(20, { inReplyTo: objRef(1), replyType: 'group', target: { kind: 'uri', uri: 'https://2' } }))),
      lens('linksSet', [objRef(2), { kind: 'uri', uri: 'https://b2' }]),
      settle('b1', created(link(21, { inReplyTo: objRef(2), replyType: 'group', target: { kind: 'uri', uri: 'https://b1' } }))),
      lens('linksOf', [objRef(1)]),
      lens('linksOf', [objRef(2)]),
    ],
  },
  {
    name: 'script-commits',
    covers:
      'commitScriptEffects: per entry, resolve the ref from the model (or the engine’s page list for an unloaded one), lower the Acrobat patch (RGB, gray and CMYK colours, a transparent fill as null, borderStyle, dash, rect, contents, flags, opacity, width), write it, stop on the first refusal (the rest skip), and reload each touched page once. Unresolvable entries (a loaded one without a subtype asks the page list first) and transparent strokes fail without writing; a listed page is searched for the entry’s own object number.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([
          hydrate({ 1: [square(1), circle(2), widget(3, 'text'), base(4, '')] }),
          { path: 'page.annotations.list', pon: 2, times: 1, value: { annotations: [square(39, { pageObjectNumber: 2 }), square(40, { pageObjectNumber: 2 })] } },
          { path: 'page.annotations.list', pon: 2, times: 1, error: { name: 'EngineError', message: 'no', code: 'NotFound' } },
          { path: 'page.annotations.list', pon: 1, value: { annotations: [square(1), circle(2), widget(3, 'text')] } },
          { path: 'page.annotations.list', pon: 2, value: { annotations: [] } },
          { path: 'page.annotations.update', pon: 1, times: 3, value: updated(square(1)) },
          { path: 'page.annotations.update', pon: 2, times: 1, value: updated(square(40, { pageObjectNumber: 2 })) },
          { path: 'page.annotations.update', pon: 1, error: { name: 'PermissionDenied', message: 'permission denied: doc.annotate.modify', required: 'doc.annotate.modify' } },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      call('commitScriptEffects', [
        [
          { annotObjectNumber: 1, page: pr(1), patch: { strokeColor: ['RGB', 1, 0, 0.5], fillColor: ['T'], opacity: 0.5, width: 3, borderStyle: 'D', dash: [3, 1], rect: [10, 20, 30, 40], contents: 'x', flags: { hidden: true } } },
          { annotObjectNumber: 2, patch: { strokeColor: ['G', 0.5], fillColor: ['CMYK', 0.1, 0.2, 0.3, 0.4], borderStyle: 'S' } },
          { annotObjectNumber: 40, page: pr(2), patch: { contents: 'far' } },
          { annotObjectNumber: 41, page: pr(2), patch: { contents: 'missing' } },
          { annotObjectNumber: 42, patch: { contents: 'no page' } },
          { annotObjectNumber: 4, patch: { contents: 'no kind' } },
          { annotObjectNumber: 3, patch: { strokeColor: ['T'] } },
          { annotObjectNumber: 3, patch: { width: 1 } },
          { annotObjectNumber: 2, patch: { opacity: 1 } },
          { annotObjectNumber: 1, patch: { opacity: 1 } },
        ],
      ]),
      call('list', [1]),
    ],
  },
  {
    name: 'script-commits-through-actions',
    covers: 'With the actions plugin present, init registers commitScriptEffects as its annotation commit sink and releases it on close.',
    plugins: [{ factory: 'interaction' }, { factory: 'actions' }, { factory: 'annotation' }],
    engine: { documents: { a: doc([hydrate({ 1: [square(1)] })]) } },
    snapshot: 'none',
    steps: [...boot, { op: 'call', target: 'documents', method: 'close', args: ['a'] }],
  },
  {
    name: 'attachment-download',
    covers: 'downloadAttachment asks the page service for the bytes and resolves with them; an unknown ref rejects.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([
          hydrate({ 1: [square(1)] }),
          { path: 'page.annotations.downloadFile', pon: 1, value: { data: { $bytes: 'AQID' }, name: 'a.bin', mimeType: 'application/octet-stream' } },
        ]),
      },
    },
    snapshot: 'none',
    steps: [...boot, call('downloadAttachment', [objRef(1)]), call('downloadAttachment', [objRef(7)])],
  },
  {
    name: 'baked-appearances',
    covers:
      'appearances asks for the page’s /AP images at a scale and resolves their list; a failure resolves []. A signal already aborted aborts the request at once; one aborted later aborts it then. An unknown page throws synchronously.',
    plugins: [...PLUGINS, { factory: 'annotation-signals' }],
    engine: {
      documents: {
        a: doc([
          hydrate(),
          { path: 'page.annotations.renderAppearanceImages', pon: 1, times: 1, value: { appearances: [{ ref: objRef(1), rect: { left: 0, bottom: 0, right: 1, top: 1 } }] } },
          { path: 'page.annotations.renderAppearanceImages', pon: 1, times: 1, error: { name: 'EngineError', message: 'render failed', code: 'Internal' } },
          { path: 'page.annotations.renderAppearanceImages', pon: 1, defer: 'ap' },
        ]),
      },
    },
    snapshot: 'none',
    steps: [
      ...boot,
      call('appearances', [1, 2]),
      call('appearances', [1, 2]),
      { op: 'call', target: { token: 'annotation-test/signals' }, method: 'abort', args: ['early'] },
      call('appearances', [1, 1, { $abortSignal: 'early' }]),
      call('appearances', [1, 1, { $abortSignal: 'late' }]),
      { op: 'call', target: { token: 'annotation-test/signals' }, method: 'abort', args: ['late'] },
      call('appearances', [1, 3]),
      settle('ap', { appearances: [] }),
      call('appearances', [42, 1]),
    ],
  },
  {
    name: 'bake-scale-on-a-lattice',
    covers: 'bakeScale snaps up the document’s appearance lattice and caps at its top rung.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc(
          [
            hydrate(),
            {
              path: 'render.policy',
              value: { kind: 'lattice', fullPage: { widths: [512, 1024] }, appearances: { scales: [2, 1, 4] } },
            },
          ],
          { services: ['render'], pages: PAGES },
        ),
      },
    },
    snapshot: 'none',
    steps: [...boot, call('bakeScale', [0.5]), call('bakeScale', [1.5]), call('bakeScale', [9])],
  },
];

void fail;

export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/annotation-comments.json',
    describes:
      'packages/plugin/annotation: comments, attached links, the script commit door, attachment download and baked appearances.',
    cases,
  },
];
