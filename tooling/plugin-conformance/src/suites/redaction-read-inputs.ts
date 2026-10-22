/** Minimal synchronous read ports, guarded against undeclared upstream dependencies. */
import type { Annotation, PageLayout } from '@embedpdf/engine-core/runtime';
import { createPendingReads } from '../../../../packages/plugin/redaction/src/read/pending';
export const suites = [];
import type { AnnotationHostCapability } from '../../../../packages/plugin/annotation/src/host-contract';
/** A read-only host projection: every unprovided member fails immediately. No wire input is cast. */
export function annotationReadPort(
  records: readonly Annotation[],
  layouts: readonly PageLayout[],
): AnnotationHostCapability {
  const list: AnnotationHostCapability['list'] = (filter) =>
    filter?.pages
      ? records.filter((r) =>
          filter.pages!.some((p) =>
            typeof p === 'number'
              ? layouts[p]?.ref.objectNumber === r.page.objectNumber
              : p.objectNumber === r.page.objectNumber,
          ),
        )
      : records;
  const stable = new Map<string, readonly Annotation[]>();
  const cached: AnnotationHostCapability['list'] = (filter) => {
    const key = JSON.stringify(filter ?? {});
    let answer = stable.get(key);
    if (!answer) {
      answer = list(filter);
      stable.set(key, answer);
    }
    return answer;
  };
  const port: Pick<AnnotationHostCapability, 'list'> = { list: cached };
  return new Proxy(port, {
    get(target, key) {
      if (key === 'list') return target.list;
      throw new Error('undeclared pure annotation port ' + String(key));
    },
  }) as AnnotationHostCapability;
}
/** Only serializes the document/annotation read ports; all redaction policy is shipping code. */
export function pending(pages: readonly PageLayout[] | null, records: readonly Annotation[]) {
  const layouts = pages ?? [];
  return createPendingReads(
    {
      getPage: (target) =>
        layouts.find((p) =>
          typeof target === 'number'
            ? p.index === target
            : p.ref.objectNumber === target.objectNumber,
        ) ?? null,
    },
    {
      store: {
        pages: () => layouts.map((p) => p.ref),
        pageIndexOf: (page) =>
          layouts.find((p) => p.ref.objectNumber === page.objectNumber)?.index ?? -1,
        canApply: () => false,
        requireService: () => {
          throw new Error('pure read must not acquire service');
        },
      },
      siblings: {
        annotation: annotationReadPort(records, layouts),
        selection: () => null,
        search: () => null,
      },
    },
  );
}
