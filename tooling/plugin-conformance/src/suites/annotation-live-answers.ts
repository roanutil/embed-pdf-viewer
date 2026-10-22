/** Explicit current engine answers, constructed before the scenarios run. */
import { annotationOfNew } from '../../../../packages/core/annotation/src/record';
import { annotationOfDraft, type AnnotationDraft } from '@embedpdf/engine-core/runtime';
import type { ResponseSpec } from '../fake-engine';
import {
  objRef,
  write,
  created,
  updated,
  deleted,
  refused,
  square,
  circle,
  line,
  polygon,
  ink,
  textNote,
  stamp,
  markup,
  caret,
  link,
} from './annotation-live-fixtures';
export const born = (n: number, draft: AnnotationDraft, page = 1) =>
  annotationOfNew(draft, { ref: objRef(n, page) });
const conversation = (
  n: number,
  draft: Extract<AnnotationDraft, { subtype: 'text' }>,
  at: string,
) =>
  annotationOfDraft(draft, {
    ref: objRef(n),
    attribution: { userId: 'me', createdBy: 'me', modifiedBy: 'me', createdAt: at, modifiedAt: at },
  });
const linked = (n: number, parent: number, suffix: string, x = 100) =>
  born(n, {
    subtype: 'link',
    rect: { x, y: 100, width: 100, height: 100 },
    reply: { to: objRef(parent), type: 'group' },
    target: { kind: 'uri', uri: `https://${suffix}` },
  });
export const LIVE_ANSWERS: Readonly<Record<string, ResponseSpec[]>> = {
  'threads-and-members': [],
  'threads-nan-top': [],
  'threads-off-the-layout': [],
  'reply-edit-status-marked': [
    write(
      1,
      [
        {
          item: created(
            conversation(
              10001,
              {
                subtype: 'text',
                rect: { x: 100, y: 200, width: 20, height: 20 },
                icon: 'comment',
                contents: 'thanks',
                reply: {
                  to: {
                    kind: 'objectNumber',
                    page: { kind: 'objectNumber', objectNumber: 1 },
                    objectNumber: 1,
                  },
                },
                noZoom: true,
                noRotate: true,
              },
              '2024-01-07T00:00:00Z',
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
    write(
      2,
      [
        {
          item: updated(
            textNote(4, {
              contents: 'edited',
              reply: {
                to: {
                  kind: 'objectNumber',
                  page: { kind: 'objectNumber', objectNumber: 1 },
                  objectNumber: 1,
                },
                type: 'reply',
              },
              createdAt: '2024-01-04T00:00:00Z',
            }),
          ),
        },
      ],
      { ms: 0 },
    ),
    write(
      3,
      [
        {
          item: created(
            conversation(
              10002,
              {
                subtype: 'text',
                rect: { x: 100, y: 200, width: 20, height: 20 },
                reply: {
                  to: {
                    kind: 'objectNumber',
                    page: { kind: 'objectNumber', objectNumber: 1 },
                    objectNumber: 6,
                  },
                },
                state: 'completed',
                stateModel: 'review',
                hidden: true,
                noZoom: true,
                noRotate: true,
              },
              '2024-01-08T00:00:00Z',
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
    write(
      4,
      [
        {
          item: created(
            conversation(
              10003,
              {
                subtype: 'text',
                rect: { x: 100, y: 200, width: 20, height: 20 },
                reply: {
                  to: {
                    kind: 'objectNumber',
                    page: { kind: 'objectNumber', objectNumber: 1 },
                    objectNumber: 1,
                  },
                },
                state: 'marked',
                stateModel: 'marked',
                hidden: true,
                noZoom: true,
                noRotate: true,
              },
              '2024-01-09T00:00:00Z',
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
    write(
      5,
      [
        {
          item: created(
            conversation(
              10004,
              {
                subtype: 'text',
                rect: { x: 100, y: 200, width: 20, height: 20 },
                reply: {
                  to: {
                    kind: 'objectNumber',
                    page: { kind: 'objectNumber', objectNumber: 1 },
                    objectNumber: 1,
                  },
                },
                state: 'unmarked',
                stateModel: 'marked',
                hidden: true,
                noZoom: true,
                noRotate: true,
              },
              '2024-01-10T00:00:00Z',
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
  ],
  'status-without-a-previous-one': [
    write(
      1,
      [
        {
          item: created(
            born(
              10001,
              {
                subtype: 'text',
                rect: { x: 100, y: 200, width: 20, height: 20 },
                reply: {
                  to: {
                    kind: 'objectNumber',
                    page: { kind: 'objectNumber', objectNumber: 1 },
                    objectNumber: 1,
                  },
                },
                state: 'none',
                stateModel: 'review',
                hidden: true,
                noZoom: true,
                noRotate: true,
              },
              1,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
  ],
  'remove-and-remove-thread-atomically': [
    write(
      1,
      [
        {
          item: deleted({
            kind: 'objectNumber',
            page: { kind: 'objectNumber', objectNumber: 1 },
            objectNumber: 3,
          }),
          changed: [
            {
              kind: 'objectNumber',
              page: { kind: 'objectNumber', objectNumber: 1 },
              objectNumber: 3,
            },
          ],
        },
      ],
      { ms: 0 },
    ),
    write(
      2,
      [
        {
          item: deleted({
            kind: 'objectNumber',
            page: { kind: 'objectNumber', objectNumber: 1 },
            objectNumber: 15,
          }),
          changed: [
            {
              kind: 'objectNumber',
              page: { kind: 'objectNumber', objectNumber: 1 },
              objectNumber: 15,
            },
            {
              kind: 'objectNumber',
              page: { kind: 'objectNumber', objectNumber: 1 },
              objectNumber: 16,
            },
          ],
        },
      ],
      { ms: 0 },
    ),
    refused('intended atomic thread refusal'),
  ],
  'comment-permissions': [],
  'attached-links': [
    write(1, [{ item: created(linked(10001, 1, 'a')) }]),
    write(2, [{ item: updated(linked(10001, 1, 'b')) }]),
    write(3, [{ item: updated(linked(10001, 1, 'b')) }]),
    write(4, [{ item: deleted(objRef(10001)), changed: [objRef(10001)] }]),
  ],
  'links-follow-geometry-and-props': [
    write(
      1,
      [
        {
          item: updated(
            square(1, {
              rect: { x: 110, y: 100, width: 100, height: 100 },
              box: { x: 110, y: 100, width: 100, height: 100 },
            }),
          ),
        },
        {
          item: updated(
            link(20, {
              rect: { x: 110, y: 100, width: 100, height: 100 },
              reply: {
                to: {
                  kind: 'objectNumber',
                  page: { kind: 'objectNumber', objectNumber: 1 },
                  objectNumber: 1,
                },
                type: 'group',
              },
              actions: {
                activate: {
                  root: { type: 'uri', subtype: 'URI', uri: 'https://a', isMap: false, next: [] },
                  incomplete: false,
                  warningFlags: 0,
                  warnings: [],
                },
              },
              target: { kind: 'uri', uri: 'https://a' },
            }),
          ),
        },
      ],
      { ms: 0 },
    ),
    write(
      2,
      [
        {
          item: created(
            born(
              10001,
              {
                subtype: 'link',
                rect: { x: 50, y: 55, width: 150, height: 15 },
                target: { kind: 'uri', uri: 'https://m' },
                reply: {
                  to: {
                    kind: 'objectNumber',
                    page: { kind: 'objectNumber', objectNumber: 1 },
                    objectNumber: 2,
                  },
                  type: 'group',
                },
              },
              1,
            ),
          ),
        },
        {
          item: created(
            born(
              10002,
              {
                subtype: 'link',
                rect: { x: 50, y: 85, width: 70, height: 15 },
                target: { kind: 'uri', uri: 'https://m' },
                reply: {
                  to: {
                    kind: 'objectNumber',
                    page: { kind: 'objectNumber', objectNumber: 1 },
                    objectNumber: 2,
                  },
                  type: 'group',
                },
              },
              1,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
    refused('intended link refusal'),
  ],
  'rapid-link-sets-use-pending-identity': [
    write(1, [{ item: created(linked(10001, 1, '1')) }], { defer: 'child' }),
    write(2, [{ item: updated(linked(10001, 1, '2')) }], { defer: 'retarget' }),
  ],
  'link-updates-queue-across-parents': [
    write(1, [{ item: created(linked(10001, 1, '1')) }], { defer: 'a1' }),
    write(2, [{ item: updated(linked(10001, 1, '2')) }], { defer: 'a2' }),
    write(3, [{ item: updated(linked(10001, 1, '3')) }], { defer: 'a3' }),
    write(4, [{ item: created(linked(10002, 2, 'b1', 300)) }], { defer: 'b1' }),
    write(5, [{ item: updated(linked(10001, 1, '4')) }], { defer: 'a4' }),
    write(6, [{ item: updated(linked(10002, 2, 'b2', 300)) }], { defer: 'b2' }),
  ],
  'script-commits': [
    write(1, [
      {
        item: updated(
          square(1, {
            rect: { x: 10, y: 760, width: 20, height: 20 },
            box: { x: 10, y: 760, width: 20, height: 20 },
            contents: 'x',
            hidden: true,
            color: '#ff0080',
            opacity: 0.5,
            strokeWidth: 3,
            borderStyle: 'dashed',
            dashArray: [3, 1],
          }),
          true,
        ),
      },
    ]),
    write(2, [{ item: updated(circle(2, { color: '#808080', interiorColor: '#80664d' }), true) }]),
    refused('intended script refusal'),
  ],
  'script-commits-through-actions': [],
  'attachment-download': [],
  'baked-appearances-through-render': [],
  'appearance-scale-owned-by-render': [],
  'hover-state-on-change-only': [],
  'hover-feeds-the-actions-pump': [
    write(
      1,
      [
        {
          item: created(
            born(
              10001,
              {
                color: '#e5484d',
                strokeWidth: 6,
                box: { x: 60, y: 470, width: 80, height: 60 },
                rotation: null,
                print: true,
                subtype: 'square',
              },
              1,
            ),
          ),
        },
      ],
      { ms: 0, defer: 'hover-create' },
    ),
  ],
  'permission-twins': [],
  'highlight-by-drag': [
    write(
      1,
      [
        {
          item: created(
            born(
              10001,
              {
                color: '#ffe16a',
                blendMode: 'multiply',
                quadPoints: [
                  {
                    upperLeft: { x: 50, y: 50 },
                    upperRight: { x: 116, y: 50 },
                    lowerLeft: { x: 50, y: 62 },
                    lowerRight: { x: 116, y: 62 },
                  },
                  {
                    upperLeft: { x: 50, y: 70 },
                    upperRight: { x: 86, y: 70 },
                    lowerLeft: { x: 50, y: 82 },
                    lowerRight: { x: 86, y: 82 },
                  },
                ],
                print: true,
                subtype: 'highlight',
              },
              1,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
    write(
      2,
      [
        {
          item: created(
            born(
              10002,
              {
                color: '#ef4444',
                quadPoints: [
                  {
                    upperLeft: { x: 50, y: 50 },
                    upperRight: { x: 74, y: 50 },
                    lowerLeft: { x: 50, y: 62 },
                    lowerRight: { x: 74, y: 62 },
                  },
                ],
                print: true,
                subtype: 'squiggly',
              },
              1,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
  ],
  'insert-and-replace-text': [
    write(
      1,
      [
        {
          item: created(
            born(
              10001,
              {
                color: '#ef4444',
                box: { x: 77, y: 56, width: 6, height: 6 },
                rotation: null,
                print: true,
                subtype: 'caret',
              },
              1,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
    write(
      2,
      [
        {
          item: created(
            born(
              10002,
              {
                box: { x: 83, y: 76, width: 6, height: 6 },
                rotation: null,
                color: '#ef4444',
                opacity: 1,
                intent: 'replace',
                print: true,
                subtype: 'caret',
              },
              1,
            ),
          ),
        },
        {
          item: created(
            born(
              10003,
              {
                color: '#ef4444',
                quadPoints: [
                  {
                    upperLeft: { x: 50, y: 50 },
                    upperRight: { x: 116, y: 50 },
                    lowerLeft: { x: 50, y: 62 },
                    lowerRight: { x: 116, y: 62 },
                  },
                  {
                    upperLeft: { x: 50, y: 70 },
                    upperRight: { x: 86, y: 70 },
                    lowerLeft: { x: 50, y: 82 },
                    lowerRight: { x: 86, y: 82 },
                  },
                ],
                intent: 'strikeout-text-edit',
                print: true,
                subtype: 'strikeout',
                reply: {
                  to: {
                    kind: 'objectNumber',
                    page: { kind: 'objectNumber', objectNumber: 1 },
                    objectNumber: 10002,
                  },
                  type: 'group',
                },
              },
              1,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
  ],
  'replace-text-anchors-at-the-end-glyph': [
    write(
      1,
      [
        {
          item: created(
            born(
              10001,
              {
                box: { x: 65, y: 56, width: 6, height: 6 },
                rotation: null,
                color: '#ef4444',
                opacity: 1,
                intent: 'replace',
                print: true,
                subtype: 'caret',
              },
              1,
            ),
          ),
        },
        {
          item: created(
            born(
              10002,
              {
                color: '#ef4444',
                quadPoints: [
                  {
                    upperLeft: { x: 50, y: 50 },
                    upperRight: { x: 68, y: 50 },
                    lowerLeft: { x: 50, y: 74 },
                    lowerRight: { x: 68, y: 74 },
                  },
                ],
                intent: 'strikeout-text-edit',
                print: true,
                subtype: 'strikeout',
                reply: {
                  to: {
                    kind: 'objectNumber',
                    page: { kind: 'objectNumber', objectNumber: 1 },
                    objectNumber: 10001,
                  },
                  type: 'group',
                },
              },
              1,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
  ],
  'create-from-selection-anchors-at-the-end-glyph': [
    write(
      1,
      [
        {
          item: created(
            born(
              10001,
              {
                box: { x: 65, y: 56, width: 6, height: 6 },
                rotation: null,
                color: '#ef4444',
                opacity: 1,
                intent: 'replace',
                print: true,
                subtype: 'caret',
              },
              1,
            ),
          ),
        },
        {
          item: created(
            born(
              10002,
              {
                color: '#ef4444',
                quadPoints: [
                  {
                    upperLeft: { x: 50, y: 50 },
                    upperRight: { x: 68, y: 50 },
                    lowerLeft: { x: 50, y: 74 },
                    lowerRight: { x: 68, y: 74 },
                  },
                ],
                intent: 'strikeout-text-edit',
                print: true,
                subtype: 'strikeout',
                reply: {
                  to: {
                    kind: 'objectNumber',
                    page: { kind: 'objectNumber', objectNumber: 1 },
                    objectNumber: 10001,
                  },
                  type: 'group',
                },
              },
              1,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
  ],
  'replace-text-across-pages': [
    write(
      1,
      [
        {
          item: created(
            born(
              10001,
              {
                box: { x: 113, y: 76, width: 6, height: 6 },
                rotation: null,
                color: '#ef4444',
                opacity: 1,
                intent: 'replace',
                print: true,
                subtype: 'caret',
              },
              1,
            ),
          ),
        },
        {
          item: created(
            born(
              10002,
              {
                color: '#ef4444',
                quadPoints: [
                  {
                    upperLeft: { x: 50, y: 70 },
                    upperRight: { x: 116, y: 70 },
                    lowerLeft: { x: 50, y: 82 },
                    lowerRight: { x: 116, y: 82 },
                  },
                ],
                intent: 'strikeout-text-edit',
                print: true,
                subtype: 'strikeout',
                reply: {
                  to: {
                    kind: 'objectNumber',
                    page: { kind: 'objectNumber', objectNumber: 1 },
                    objectNumber: 10001,
                  },
                  type: 'group',
                },
              },
              1,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
    write(
      2,
      [
        {
          item: created(
            born(
              10003,
              {
                box: { x: 51, y: 26, width: 6, height: 6 },
                rotation: null,
                color: '#ef4444',
                opacity: 1,
                intent: 'replace',
                print: true,
                subtype: 'caret',
              },
              2,
            ),
          ),
        },
        {
          item: created(
            born(
              10004,
              {
                color: '#ef4444',
                quadPoints: [
                  {
                    upperLeft: { x: 30, y: 20 },
                    upperRight: { x: 54, y: 20 },
                    lowerLeft: { x: 30, y: 32 },
                    lowerRight: { x: 54, y: 32 },
                  },
                ],
                intent: 'strikeout-text-edit',
                print: true,
                subtype: 'strikeout',
                reply: {
                  to: {
                    kind: 'objectNumber',
                    page: { kind: 'objectNumber', objectNumber: 2 },
                    objectNumber: 10003,
                  },
                  type: 'group',
                },
              },
              2,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
  ],
  'preview-follows-the-tool': [],
  'markup-from-selection': [
    write(
      1,
      [
        {
          item: created(
            born(
              10001,
              {
                color: '#ef4444',
                quadPoints: [
                  {
                    upperLeft: { x: 50, y: 50 },
                    upperRight: { x: 116, y: 50 },
                    lowerLeft: { x: 50, y: 62 },
                    lowerRight: { x: 116, y: 62 },
                  },
                  {
                    upperLeft: { x: 50, y: 70 },
                    upperRight: { x: 116, y: 70 },
                    lowerLeft: { x: 50, y: 82 },
                    lowerRight: { x: 116, y: 82 },
                  },
                ],
                print: true,
                subtype: 'underline',
              },
              1,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
    write(
      2,
      [
        {
          item: created(
            born(
              10002,
              {
                color: '#ef4444',
                quadPoints: [
                  {
                    upperLeft: { x: 30, y: 20 },
                    upperRight: { x: 54, y: 20 },
                    lowerLeft: { x: 30, y: 32 },
                    lowerRight: { x: 54, y: 32 },
                  },
                ],
                print: true,
                subtype: 'underline',
              },
              2,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
  ],
  'create-from-selection-text-edits': [
    write(
      1,
      [
        {
          item: created(
            born(
              10001,
              {
                color: '#ef4444',
                box: { x: 71, y: 56, width: 6, height: 6 },
                rotation: null,
                print: true,
                subtype: 'caret',
              },
              1,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
    write(
      2,
      [
        {
          item: created(
            born(
              10002,
              {
                box: { x: 113, y: 76, width: 6, height: 6 },
                rotation: null,
                color: '#ef4444',
                opacity: 1,
                intent: 'replace',
                print: true,
                subtype: 'caret',
              },
              1,
            ),
          ),
        },
        {
          item: created(
            born(
              10003,
              {
                color: '#ef4444',
                quadPoints: [
                  {
                    upperLeft: { x: 50, y: 70 },
                    upperRight: { x: 116, y: 70 },
                    lowerLeft: { x: 50, y: 82 },
                    lowerRight: { x: 116, y: 82 },
                  },
                ],
                intent: 'strikeout-text-edit',
                print: true,
                subtype: 'strikeout',
                reply: {
                  to: {
                    kind: 'objectNumber',
                    page: { kind: 'objectNumber', objectNumber: 1 },
                    objectNumber: 10002,
                  },
                  type: 'group',
                },
              },
              1,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
    write(
      3,
      [
        {
          item: created(
            born(
              10004,
              {
                box: { x: 51, y: 26, width: 6, height: 6 },
                rotation: null,
                color: '#ef4444',
                opacity: 1,
                intent: 'replace',
                print: true,
                subtype: 'caret',
              },
              2,
            ),
          ),
        },
        {
          item: created(
            born(
              10005,
              {
                color: '#ef4444',
                quadPoints: [
                  {
                    upperLeft: { x: 30, y: 20 },
                    upperRight: { x: 54, y: 20 },
                    lowerLeft: { x: 30, y: 32 },
                    lowerRight: { x: 54, y: 32 },
                  },
                ],
                intent: 'strikeout-text-edit',
                print: true,
                subtype: 'strikeout',
                reply: {
                  to: {
                    kind: 'objectNumber',
                    page: { kind: 'objectNumber', objectNumber: 2 },
                    objectNumber: 10004,
                  },
                  type: 'group',
                },
              },
              2,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
    write(
      4,
      [
        {
          item: created(
            born(
              10006,
              {
                color: '#ffe16a',
                blendMode: 'multiply',
                quadPoints: [
                  {
                    upperLeft: { x: 50, y: 50 },
                    upperRight: { x: 116, y: 50 },
                    lowerLeft: { x: 50, y: 62 },
                    lowerRight: { x: 116, y: 62 },
                  },
                  {
                    upperLeft: { x: 50, y: 70 },
                    upperRight: { x: 116, y: 70 },
                    lowerLeft: { x: 50, y: 82 },
                    lowerRight: { x: 116, y: 82 },
                  },
                ],
                print: true,
                subtype: 'highlight',
              },
              1,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
    write(
      5,
      [
        {
          item: created(
            born(
              10007,
              {
                color: '#ffe16a',
                blendMode: 'multiply',
                quadPoints: [
                  {
                    upperLeft: { x: 30, y: 20 },
                    upperRight: { x: 54, y: 20 },
                    lowerLeft: { x: 30, y: 32 },
                    lowerRight: { x: 54, y: 32 },
                  },
                ],
                print: true,
                subtype: 'highlight',
              },
              2,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
  ],
  'markup-needs-a-selection-plugin-and-authority': [],
  'malformed-stamp-source-inputs': [],
  'init-registers-the-tools': [],
  'init-with-selection': [],
  'a-bad-config-tool-fails-the-capability': [],
  'edit-gestures': [
    write(
      1,
      [
        {
          item: updated(
            square(1, {
              rect: { x: 125, y: 112, width: 100, height: 100 },
              box: { x: 125, y: 112, width: 100, height: 100 },
            }),
          ),
        },
      ],
      { ms: 0 },
    ),
  ],
  'edit-gesture-cancel-and-off-page-up': [
    write(
      1,
      [
        {
          item: updated(
            square(1, {
              rect: { x: 110, y: 100, width: 100, height: 100 },
              box: { x: 110, y: 100, width: 100, height: 100 },
            }),
          ),
        },
      ],
      { ms: 0 },
    ),
  ],
  'touch-consent': [],
  'double-click-edits-free-text': [],
  'marquee-selects': [],
  'draw-shapes': [
    write(
      1,
      [
        {
          item: created(
            born(
              10001,
              {
                color: '#e5484d',
                strokeWidth: 6,
                box: { x: 100, y: 100, width: 50, height: 30 },
                rotation: null,
                print: true,
                subtype: 'square',
              },
              1,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
    write(
      2,
      [
        {
          item: created(
            born(
              10002,
              {
                color: '#e5484d',
                strokeWidth: 6,
                box: { x: 260, y: 270, width: 80, height: 60 },
                rotation: null,
                print: true,
                subtype: 'square',
              },
              1,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
    write(
      3,
      [
        {
          item: created(
            born(
              10003,
              {
                color: '#e5484d',
                strokeWidth: 2,
                fontColor: '#ef4444',
                box: { x: 130, y: 90, width: 180, height: 40 },
                rotation: 270,
                intent: 'free-text',
                contents: '',
                print: true,
                subtype: 'free-text',
              },
              1,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
    write(4, [
      {
        item: created(
          born(10004, {
            subtype: 'line',
            color: '#e5484d',
            strokeWidth: 6,
            linePoints: { start: { x: 10, y: 50 }, end: { x: 90, y: 50 } },
            rotation: null,
            print: true,
          }),
        ),
      },
    ]),
  ],
  'draw-without-authority': [],
  'ink-strokes-group': [
    write(
      1,
      [
        {
          item: created(
            born(10001, {
              subtype: 'ink',
              color: '#ef4444',
              strokeWidth: 6,
              blendMode: 'normal',
              inkList: [
                [
                  { x: 10, y: 10 },
                  { x: 20, y: 15 },
                ],
                [
                  { x: 40, y: 40 },
                  { x: 50, y: 45 },
                ],
              ],
              rotation: null,
              print: true,
            }),
          ),
        },
      ],
      { ms: 1200 },
    ),
    write(
      2,
      [
        {
          item: created(
            born(10002, {
              subtype: 'ink',
              color: '#ef4444',
              strokeWidth: 6,
              blendMode: 'normal',
              inkList: [
                [
                  { x: 200, y: 200 },
                  { x: 210, y: 215 },
                ],
              ],
              rotation: null,
              print: true,
            }),
          ),
        },
      ],
      { ms: 1200 },
    ),
    write(
      3,
      [
        {
          item: created(
            born(
              10003,
              {
                subtype: 'ink',
                color: '#ef4444',
                strokeWidth: 6,
                blendMode: 'normal',
                inkList: [
                  [
                    { x: 5, y: 5 },
                    { x: 60, y: 70 },
                  ],
                ],
                rotation: null,
                print: true,
              },
              2,
            ),
          ),
        },
      ],
      { ms: 1200 },
    ),
    write(
      4,
      [
        {
          item: created(
            born(10004, {
              subtype: 'ink',
              color: '#ffcd45',
              strokeWidth: 14,
              blendMode: 'multiply',
              intent: 'ink-highlight',
              inkList: [
                [
                  { x: 300, y: 300 },
                  { x: 350, y: 302 },
                ],
              ],
              rotation: null,
              print: true,
            }),
          ),
        },
      ],
      { ms: 1200 },
    ),
  ],
  'grab-reach-follows-the-scale': [],
  'page-box-of-an-offset-crop': [
    write(
      1,
      [
        {
          item: created(
            born(
              10001,
              {
                icon: 'comment',
                color: '#facc15',
                rect: { x: 380, y: 480, width: 20, height: 20 },
                noZoom: true,
                noRotate: true,
                subtype: 'text',
              },
              2,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
  ],
  'chrome-follows-the-view': [],
  'handlers-outrank-a-lower-handler': [
    write(
      1,
      [
        {
          item: created(
            born(
              10001,
              {
                icon: 'comment',
                color: '#facc15',
                rect: { x: 90, y: 90, width: 20, height: 20 },
                noZoom: true,
                noRotate: true,
                subtype: 'text',
              },
              1,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
    write(
      2,
      [
        {
          item: created(
            born(
              10002,
              {
                color: '#e5484d',
                strokeWidth: 6,
                box: { x: 300, y: 300, width: 50, height: 50 },
                rotation: null,
                print: true,
                subtype: 'square',
              },
              1,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
  ],
  'handlers-wake-only-for-their-tags': [],
  'edit-press-inside-the-touch-reach-only': [],
  'marquee-starts-at-exactly-4px': [],
  'hover-drives-only-a-multi-click-draft': [
    write(
      1,
      [
        {
          item: created(
            born(
              10001,
              {
                color: '#ef4444',
                strokeWidth: 6,
                blendMode: 'normal',
                inkList: [
                  [
                    { x: 10, y: 10 },
                    { x: 20, y: 15 },
                  ],
                ],
                rotation: null,
                print: true,
                subtype: 'ink',
              },
              1,
            ),
          ),
        },
      ],
      { ms: 800 },
    ),
    write(
      2,
      [
        {
          item: created(
            born(10002, {
              subtype: 'ink',
              color: '#ef4444',
              strokeWidth: 6,
              blendMode: 'normal',
              inkList: [
                [
                  { x: 200, y: 200 },
                  { x: 210, y: 215 },
                ],
              ],
              rotation: null,
              print: true,
            }),
          ),
        },
      ],
      { ms: 1000 },
    ),
  ],
  'ink-cancel-inside-the-grouping-window': [],
  'hover-follows-the-subtype-the-draft-began-with': [],
  'poly-and-callout-clicks': [
    write(1, [
      {
        item: created(
          born(10001, {
            subtype: 'polygon',
            color: '#e5484d',
            strokeWidth: 6,
            vertices: [
              { x: 10, y: 10 },
              { x: 60, y: 20 },
              { x: 40, y: 60 },
            ],
            print: true,
          }),
        ),
      },
    ]),
    write(2, [
      {
        item: created(
          born(10002, {
            subtype: 'free-text',
            color: '#e5484d',
            strokeWidth: 6,
            lineEnding: 'open-arrow',
            box: { x: 410, y: 380, width: 100, height: 40 },
            rotation: null,
            calloutLine: [
              { x: 300, y: 300 },
              { x: 340, y: 330 },
              { x: 410, y: 400 },
            ],
            intent: 'free-text-callout',
            contents: '',
            print: true,
          }),
        ),
      },
    ]),
  ],
  'note-placement-and-ghost': [
    write(
      1,
      [
        {
          item: created(
            born(
              10001,
              {
                icon: 'comment',
                color: '#facc15',
                rect: { x: 90, y: 90, width: 20, height: 20 },
                noZoom: true,
                noRotate: true,
                subtype: 'text',
              },
              1,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
    refused('intended note refusal'),
  ],
  'click-create-ghosts': [],
  'stamp-prompt-through-the-file-picker': [
    write(
      1,
      [
        {
          item: created(
            born(
              10001,
              {
                subtype: 'stamp',
                box: { x: 50, y: 75, width: 100, height: 50 },
                rotation: null,
                fit: 'contain',
              },
              1,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
    write(
      2,
      [
        {
          item: created(
            born(
              10002,
              {
                subtype: 'stamp',
                box: { x: 480, y: 685, width: 40, height: 30 },
                rotation: 90,
                fit: 'contain',
              },
              1,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
  ],
  'file-picker-remover-clears-only-its-own': [],
  'attachment-placement': [
    write(
      1,
      [
        {
          item: created(
            born(
              10001,
              {
                icon: 'paperclip',
                color: '#facc15',
                rect: { x: 40, y: 40, width: 20, height: 20 },
                noZoom: true,
                noRotate: true,
                subtype: 'file-attachment',
                file: { name: 'a.bin', mimeType: 'application/octet-stream' },
              },
              1,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
    refused('intended attachment refusal'),
  ],
  'armed-stamps': [
    write(
      1,
      [
        {
          item: created(
            born(
              10001,
              {
                subtype: 'stamp',
                box: { x: 50, y: 75, width: 100, height: 50 },
                rotation: null,
                fit: 'contain',
              },
              1,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
    write(2, [
      {
        item: created(
          born(10002, {
            subtype: 'stamp',
            box: { x: 150, y: 250, width: 300, height: 100 },
            rotation: 270,
            fit: 'contain',
          }),
        ),
      },
    ]),
    write(
      3,
      [
        {
          item: created(
            born(
              10003,
              {
                subtype: 'stamp',
                box: { x: 10, y: 20, width: 80, height: 60 },
                rotation: null,
                fit: 'contain',
              },
              1,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
  ],
  'armed-stamps-need-the-stamp-tag': [
    write(
      1,
      [
        {
          item: created(
            born(
              10001,
              {
                subtype: 'stamp',
                box: { x: 250, y: 275, width: 100, height: 50 },
                rotation: 270,
                fit: 'contain',
              },
              1,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
    write(
      2,
      [
        {
          item: created(
            born(
              10002,
              {
                subtype: 'stamp',
                box: { x: 350, y: 575, width: 100, height: 50 },
                rotation: null,
                fit: 'contain',
              },
              1,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
    write(
      3,
      [
        {
          item: created(
            born(
              10003,
              {
                subtype: 'stamp',
                box: { x: 250, y: 275, width: 100, height: 50 },
                rotation: null,
                fit: 'contain',
              },
              1,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
    write(
      4,
      [
        {
          item: created(
            born(
              10004,
              {
                subtype: 'stamp',
                box: { x: 250, y: 275, width: 100, height: 50 },
                rotation: null,
                fit: 'contain',
              },
              1,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
    write(
      5,
      [
        {
          item: created(
            born(
              10005,
              {
                subtype: 'stamp',
                box: { x: 150, y: 250, width: 300, height: 100 },
                rotation: null,
                fit: 'contain',
              },
              1,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
    write(
      6,
      [
        {
          item: created(
            born(
              10006,
              {
                subtype: 'stamp',
                box: { x: 260, y: 280, width: 80, height: 40 },
                rotation: null,
                fit: 'contain',
              },
              1,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
    write(
      7,
      [
        {
          item: created(
            born(
              10007,
              {
                subtype: 'stamp',
                box: { x: 260, y: 280, width: 80, height: 40 },
                rotation: null,
                fit: 'contain',
              },
              1,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
  ],
  'placement-rotation-follows-upright': [
    write(
      1,
      [
        {
          item: created(
            born(
              10001,
              {
                subtype: 'stamp',
                box: { x: 50, y: 75, width: 100, height: 50 },
                rotation: null,
                fit: 'contain',
              },
              1,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
    write(
      2,
      [
        {
          item: created(
            born(
              10002,
              {
                subtype: 'stamp',
                box: { x: 150, y: 175, width: 100, height: 50 },
                rotation: null,
                fit: 'contain',
              },
              1,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
    write(
      3,
      [
        {
          item: created(
            born(
              10003,
              {
                icon: 'comment',
                color: '#facc15',
                rect: { x: 290, y: 290, width: 20, height: 20 },
                noZoom: true,
                noRotate: true,
                subtype: 'text',
              },
              1,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
    write(
      4,
      [
        {
          item: created(
            born(
              10004,
              {
                icon: 'comment',
                color: '#facc15',
                rect: { x: 390, y: 410, width: 20, height: 20 },
                noZoom: true,
                noRotate: true,
                subtype: 'text',
              },
              1,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
  ],
  'fixed-bytes-stamp-tool': [
    write(
      1,
      [
        {
          item: created(
            born(
              10001,
              {
                subtype: 'stamp',
                box: { x: 50, y: 75, width: 100, height: 50 },
                rotation: 270,
                fit: 'contain',
              },
              1,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
    write(
      2,
      [
        {
          item: created(
            born(
              10002,
              {
                subtype: 'stamp',
                box: { x: 225, y: 225, width: 150, height: 150 },
                rotation: null,
                fit: 'contain',
              },
              1,
            ),
          ),
        },
      ],
      { ms: 0 },
    ),
  ],
  'behaviors-make-annotations-inert': [],
  'placement-declines': [],
};
