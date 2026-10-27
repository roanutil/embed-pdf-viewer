/**
 * A gesture whose record leaves the view ends: the store forgets the record
 * (`{ type: 'forget' }`), which clears the draft, so the next pointer move and
 * release are no-ops instead of reading a record that isn't there. The record
 * leaves when another session deletes it or when the engine refuses its create.
 */
import type { DocumentEvent } from '@embedpdf/core';
import type { AnnotationFlags, AnnotationRef } from '@embedpdf/engine-core/runtime';
import { describe, expect, it } from 'vitest';

import { annotationHarness, PAGE, type FileAnnotation } from './harness';

const NO_FLAGS: AnnotationFlags = {
  invisible: false,
  hidden: false,
  print: true,
  noZoom: false,
  noRotate: false,
  noView: false,
  readOnly: false,
  locked: false,
  toggleNoView: false,
  lockedContents: false,
};

const ref = (objectNumber: number): AnnotationRef => ({
  kind: 'objectNumber',
  page: PAGE,
  objectNumber,
});

/** A filled square in the file's coordinates (y up): on the page, x 40..60, y 520..530. */
const square = (objectNumber: number): FileAnnotation =>
  ({
    ref: ref(objectNumber),
    page: PAGE,
    index: objectNumber,
    hasAppearance: true,
    appearanceState: null,
    nm: null,
    ...NO_FLAGS,
    contents: null,
    subject: null,
    author: null,
    createdAt: null,
    modifiedAt: null,
    blendMode: 'normal',
    subtype: 'square',
    rect: { left: 40, bottom: 270, right: 60, top: 280 },
    box: { left: 40, bottom: 270, right: 60, top: 280 },
    color: '#000000',
    strokeWidth: 1,
    opacity: 1,
    interiorColor: '#ff0000',
  }) as unknown as FileAnnotation;

function held() {
  let resolve!: (value: unknown) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

type Harness = ReturnType<typeof annotationHarness>;

const draftOf = (harness: Harness) => harness.state().session.draft;

/** The rest of a drag: a move and a release, neither of which may throw or write. */
const finishDrag = (harness: Harness) => {
  const sent = harness.applied.length;
  expect(() => {
    harness.capability.editPointer('move', PAGE, { x: 80, y: 525 }, false);
    harness.capability.editPointer('up', PAGE, { x: 80, y: 525 }, false);
  }).not.toThrow();
  expect(draftOf(harness)).toBeNull();
  expect(harness.applied.length).toBe(sent);
};

describe('a gesture whose record leaves the view', () => {
  it('ends when another session deletes the record', async () => {
    const harness = annotationHarness();
    await harness.load([square(20)]);
    harness.capability.editPointer('down', PAGE, { x: 50, y: 525 }, false);
    expect(draftOf(harness)).toMatchObject({ kind: 'move', ids: ['obj:20'] });

    harness.emit({
      type: 'annotations.deleted',
      page: PAGE,
      deleted: [{ kind: 'objectNumber', objectNumber: 20 }],
      meta: { opId: 'remote-1', undoable: true },
      origin: { kind: 'remote', sub: 'alice' },
      ts: Date.now(),
    } as unknown as DocumentEvent);

    expect(harness.model().byId['obj:20']).toBeUndefined();
    expect(draftOf(harness)).toBeNull();
    finishDrag(harness);
  });

  it('ends when the engine refuses the record’s create', async () => {
    const harness = annotationHarness();
    await harness.load([]);
    const write = held();
    harness.create.mockReturnValueOnce(write.promise);

    const applied = harness.apply([
      {
        type: 'create',
        page: PAGE,
        draft: {
          subtype: 'square',
          rect: { x: 40, y: 520, width: 20, height: 10 },
          box: { x: 40, y: 520, width: 20, height: 10 },
          interiorColor: '#ff0000',
        },
      },
    ]);
    const [id] = applied.ids;
    expect(harness.model().byId[id!]!.unconfirmed).toBe(true);

    harness.capability.editPointer('down', PAGE, { x: 50, y: 525 }, false);
    expect(draftOf(harness)).toMatchObject({ kind: 'move', ids: [id] });

    write.reject(new Error('refused'));
    await applied.written;

    expect(harness.model().byId[id!]).toBeUndefined();
    expect(draftOf(harness)).toBeNull();
    finishDrag(harness);
  });
});
