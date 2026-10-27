/** Current confirmed facts used by selection/search/stage fixture construction. */
import type { DocumentEvent, PageLayout, PageRef } from '@embedpdf/engine-core/runtime';
import { annotationOfDraft } from '../../../../packages/engine/core/src/pageSpace/helpers';

export const fixtureOrigin = {
  kind: 'local' as const,
  sessionId: 's1',
  sub: null,
  ts: 0,
  serverId: null,
};
type Kind =
  | 'annotations.created'
  | 'annotations.updated'
  | 'metadata.updated'
  | 'pages.reordered'
  | 'pages.rotated'
  | 'pages.deleted'
  | 'pages.inserted'
  | 'pages.flattened'
  | 'redaction.applied';
export function confirmedEvent(
  type: Kind,
  page: PageRef,
  pages: readonly PageLayout[],
  affected: readonly PageRef[] = [page],
): DocumentEvent {
  const refs = [...affected];
  const meta = {
    affectedPages: refs,
    cacheDelta: null,
    opId: 'fixture',
    undoable: !['pages.flattened', 'redaction.applied'].includes(type),
  };
  const origin = fixtureOrigin;
  const layout = { pageCount: pages.length, pages: [...pages], namedPages: [] };
  switch (type) {
    case 'pages.reordered':
      return { type, pages: refs, layout, meta, origin };
    case 'pages.rotated':
      return { type, pages: refs, rotation: 90, layout, meta, origin };
    case 'pages.deleted':
      return { type, pages: refs, layout, meta, origin };
    case 'pages.inserted':
      return { type, insertedPages: refs, layout, meta, origin };
    case 'pages.flattened':
      return {
        type,
        pages: refs,
        usage: 'display',
        results: refs.map((page) => ({ page, status: 'applied' })),
        meta,
        origin,
      };
    case 'redaction.applied':
      return {
        type,
        scope: { pages: refs },
        results: refs.map((page) => ({ page, status: 'applied', removedAnnotationCount: 0 })),
        removedAnnotationCount: 0,
        meta,
        origin,
      };
    case 'metadata.updated':
      return {
        type,
        metadata: {
          title: null,
          author: null,
          subject: null,
          keywords: null,
          producer: null,
          creator: null,
          createdAt: null,
          modifiedAt: null,
          trapped: 'unknown',
        },
        meta,
        origin,
      };
    case 'annotations.created':
    case 'annotations.updated': {
      const ref = { kind: 'objectNumber' as const, page, objectNumber: 1000 };
      const annotation = annotationOfDraft(
        { subtype: 'square', box: { x: 10, y: 20, width: 30, height: 40 } },
        { ref, nm: 'event-square' },
      );
      const annotMeta = { ...meta, changed: [ref] };
      return type === 'annotations.created'
        ? { type, page, annotation, meta: annotMeta, origin }
        : {
            type,
            page,
            annotation,
            appearance: { action: 'preserved', changed: false },
            meta: annotMeta,
            origin,
          };
    }
  }
}

export const suites = [];
