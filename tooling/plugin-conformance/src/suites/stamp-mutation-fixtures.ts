/** Static current engine answers for stamp mutations; authored before the controller runs. */
import type {
  Annotation,
  AnnotationDraft,
  AnnotationCreateResult,
  LocalDocumentHandle,
  PageLayout,
  PageListSnapshot,
  PageNameInput,
  PieceInfoEntry,
  PieceInfoPatch,
  PieceInfoSnapshot,
} from '@embedpdf/engine-core/runtime';
import { annotationOfDraft } from '@embedpdf/engine-core/runtime';
import type { FakeDocumentSpec, ResponseSpec } from '../fake-engine';
import type { ScenarioCase, Step } from '../scenario';
import type { Suite } from '../vectors';
import { canonical } from '../canonical';
import {
  stampLibraryPieceInfo,
  stampPieceInfo,
} from '../../../../packages/plugin/stamp/src/convention';
import {
  assetDoc,
  assetPage,
  metadata,
  mutationMeta,
  pieceInfo,
  pdf,
} from './stamp-current-fixtures';
import { objRef, pr, write, created } from './annotation-live-fixtures';
import { WITH_ANNOTATION } from './form-live-fixtures';
export const suites: Suite[] = [];
export { WITH_ANNOTATION, objRef, pr, write, created };
export const ONE = [assetPage(0, 100)];
export const TWO = [...ONE, assetPage(1, 101)];
export const THREE = [...TWO, assetPage(2, 102)];
export const SEED = [assetPage(0, 3, 612, 792)];
export const openDoc = (id = 'a'): Step => ({
  op: 'call',
  target: 'documents',
  method: 'open',
  args: [{ kind: 'bytes', id, bytes: canonical(new Uint8Array()) }],
});
export const named = (
  name: string,
  objectNumber: number,
): PageListSnapshot['namedPages'][number] => ({
  name,
  target: { kind: 'page', page: pr(objectNumber) },
});
export const fn = (name: string, over: Record<string, unknown> = {}) => ({ $fn: name, ...over });
export const EVENTS = [
  'onLibraryChanged',
  'onLibraryCreated',
  'onLibraryUpdated',
  'onLibraryDeleted',
  'onAssetCreated',
  'onAssetUpdated',
  'onAssetDeleted',
  'onArmChanged',
] as const;
export const listenAll: Step[] = EVENTS.map((method) => ({
  op: 'call',
  target: { token: 'stamp' },
  method,
  args: [fn(method.slice(2))],
}));

/** Only primitive tagging and PDF merge semantics; no stamp policy or runtime requests. */
export function merged(
  initial: Record<string, PieceInfoEntry>,
  patch: PieceInfoPatch,
): PieceInfoSnapshot {
  const entries = { ...initial };
  for (const [key, value] of Object.entries(patch)) {
    if (value === null) delete entries[key];
    else if (typeof value === 'string') entries[key] = { type: 'string', value };
    else if (typeof value === 'number') entries[key] = { type: 'number', value };
    else if (typeof value === 'boolean') entries[key] = { type: 'boolean', value };
    else if ('name' in value) entries[key] = { type: 'name', value: value.name };
    else entries[key] = { type: 'string-array', value: [...value] };
  }
  return pieceInfo(entries);
}
interface Input {
  readonly title?: string;
  readonly metadataTitles?: readonly string[];
  readonly catalog?: Record<string, PieceInfoEntry>;
  readonly catalogPatches?: readonly PieceInfoPatch[];
  readonly page?: Readonly<Record<number, Record<string, PieceInfoEntry>>>;
  readonly pagePatches?: readonly { page: number; patch: PieceInfoPatch }[];
  readonly names?: readonly PageNameInput[];
  readonly namedPages?: PageListSnapshot['namedPages'];
  readonly download?: string;
  readonly responses?: readonly ResponseSpec[];
  readonly inserted?: readonly PageLayout[];
  readonly deleted?: readonly number[];
}
export function mutationDoc(pages: PageLayout[], input: Input = {}): FakeDocumentSpec {
  const registry = input.inserted ? [...pages, ...input.inserted] : pages;
  const snapshot: PageListSnapshot = {
    pageCount: registry.length,
    pages: registry,
    namedPages: input.namedPages ?? [],
  };
  let catalog = input.catalog ?? {};
  const catalogAnswers: ResponseSpec[] = (input.catalogPatches ?? []).map((patch) => {
    const result = merged(catalog, patch);
    catalog = result.entries;
    return {
      path: 'pieceInfo.update',
      args: ['EMBD_StampLibrary', patch],
      value: { pieceInfo: result, meta: mutationMeta() } satisfies Awaited<
        ReturnType<LocalDocumentHandle['pieceInfo']['update']>
      >,
    };
  });
  const pageEntries: Record<number, Record<string, PieceInfoEntry>> = { ...input.page };
  const pageAnswers: ResponseSpec[] = (input.pagePatches ?? []).map(({ page, patch }) => {
    const result = merged(pageEntries[page] ?? {}, patch);
    pageEntries[page] = result.entries;
    return {
      path: 'page.pieceInfo.update',
      pon: page,
      args: ['EMBD_Stamp', patch],
      value: {
        pieceInfo: result,
        meta: mutationMeta(registry.filter((p) => p.ref.objectNumber === page)),
      } satisfies Awaited<
        ReturnType<ReturnType<LocalDocumentHandle['page']>['pieceInfo']['update']>
      >,
    };
  });
  const names = new Map(snapshot.namedPages.map((entry) => [entry.name, entry]));
  const nameAnswers: ResponseSpec[] = (input.names ?? []).map((write) => {
    if (write.replace) names.delete(write.replace);
    names.set(write.name, { name: write.name, target: { kind: 'page', page: write.page } });
    const layout: PageListSnapshot = {
      ...snapshot,
      namedPages: [...names.values()].sort((a, b) =>
        a.name < b.name ? -1 : a.name > b.name ? 1 : 0,
      ),
    };
    return {
      path: 'pages.setName',
      args: [write],
      value: {
        layout,
        meta: mutationMeta(registry.filter((p) => p.ref.objectNumber === write.page.objectNumber)),
      } satisfies Awaited<ReturnType<LocalDocumentHandle['pages']['setName']>>,
    };
  });
  const afterDelete = registry
    .filter((p) => !input.deleted?.includes(p.ref.objectNumber))
    .map((p, index) => ({ ...p, index }));
  const layoutAfterDelete: PageListSnapshot = {
    pageCount: afterDelete.length,
    pages: afterDelete,
    namedPages: [...names.values()]
      .filter(
        (e) => e.target.kind !== 'page' || !input.deleted?.includes(e.target.page.objectNumber),
      )
      .sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0)),
  };
  return assetDoc(registry, {
    ...(input.title ? { title: input.title } : {}),
    catalog: input.catalog,
    page: input.page,
    namedPages: snapshot.namedPages,
    download: input.download,
    responses: [
      ...(input.responses ?? []),
      ...(input.metadataTitles ?? []).map((title) => ({
        path: 'metadata.update',
        args: [{ title }],
        value: { metadata: metadata(title), meta: mutationMeta() } satisfies Awaited<
          ReturnType<LocalDocumentHandle['metadata']['update']>
        >,
      })),
      ...catalogAnswers,
      ...pageAnswers,
      ...nameAnswers,
      ...(input.inserted
        ? [
            {
              path: 'pages.insert',
              value: {
                insertedPages: input.inserted.map((p) => p.ref),
                layout: snapshot,
                meta: mutationMeta([...input.inserted]),
              } satisfies Awaited<ReturnType<LocalDocumentHandle['pages']['insert']>>,
            },
            {
              path: 'pages.insertBlank',
              value: {
                insertedPages: input.inserted.map((p) => p.ref),
                layout: snapshot,
                meta: mutationMeta([...input.inserted]),
              } satisfies Awaited<ReturnType<LocalDocumentHandle['pages']['insertBlank']>>,
            },
          ]
        : []),
      ...(input.deleted
        ? [
            {
              path: 'pages.delete',
              value: {
                layout: layoutAfterDelete,
                meta: mutationMeta(
                  registry.filter((p) => input.deleted?.includes(p.ref.objectNumber)),
                ),
              } satisfies Awaited<ReturnType<LocalDocumentHandle['pages']['delete']>>,
            },
          ]
        : []),
    ],
  });
}
export const seedDoc = (
  id: string,
  title: string,
  kind = 'stamps',
  categories: string[] | null = null,
) =>
  mutationDoc(SEED, {
    metadataTitles: [title],
    catalogPatches: [stampLibraryPieceInfo(id, { kind, ...(categories ? { categories } : {}) })],
    download: `seed-${id}`,
  });
export function appendDoc(
  pages: PageLayout[],
  name: string,
  input: {
    number?: number;
    label?: string;
    kind?: 'stamp' | 'signature' | 'initials';
    subject?: string;
    categories?: string[];
    size?: { width: number; height: number };
    responses?: ResponseSpec[];
    download?: string;
    namedPages?: PageListSnapshot['namedPages'];
    raster?: boolean;
  } = {},
): FakeDocumentSpec {
  const size = input.size ?? { width: 300, height: 120 };
  const added = assetPage(pages.length, input.number ?? 900, size.width, size.height);
  const annotation = annotationOfDraft(
    { subtype: 'stamp', box: { x: 0, y: 0, ...size }, fit: 'fill' },
    { ref: objRef(40, added.ref.objectNumber) },
  );
  const answer: AnnotationCreateResult = {
    annotation,
    meta: { ...mutationMeta([added]), changed: [annotation.ref] },
  };
  return mutationDoc(pages, {
    inserted: [added],
    namedPages: input.namedPages,
    names: [{ name: `${name}=${input.label ?? name}`, page: added.ref }],
    pagePatches: [
      {
        page: added.ref.objectNumber,
        patch: stampPieceInfo(input.kind ?? 'stamp', {
          subject: input.subject,
          categories: input.categories,
        }),
      },
    ],
    responses: [
      ...(input.responses ?? []),
      ...(input.raster
        ? [
            { path: 'page.annotations.create', pon: added.ref.objectNumber, value: answer },
            {
              path: 'pages.flatten',
              args: [[added.ref], { usage: 'display' }],
              value: {
                pages: [added.ref],
                usage: 'display',
                results: [{ page: added.ref, status: 'applied' }],
                meta: mutationMeta([added]),
              } satisfies Awaited<ReturnType<LocalDocumentHandle['pages']['flatten']>>,
            },
          ]
        : []),
    ],
    download: input.download ?? 'canonical-2',
  });
}
export const deleteDoc = (
  pages: PageLayout[],
  numbers = [pages[0].ref.objectNumber],
  responses: ResponseSpec[] = [],
  namedPages: PageListSnapshot['namedPages'] = [],
) =>
  mutationDoc(pages, { deleted: numbers, responses, namedPages, download: 'canonical-rewritten' });
export const scratchDoc = (draft: AnnotationDraft, input: { responses?: ResponseSpec[] } = {}) => {
  const annotation: Annotation = annotationOfDraft(draft, { ref: objRef(40, 3) });
  return mutationDoc(SEED, {
    responses: [
      ...(input.responses ?? []),
      {
        path: 'page.annotations.create',
        pon: 3,
        value: {
          annotation,
          meta: { ...mutationMeta(SEED), changed: [annotation.ref] },
        } satisfies AnnotationCreateResult,
      },
      { path: 'page.annotations.exportAppearance', pon: 3, value: canonical(pdf('mark')) },
    ],
  });
};
export const namedEngine = (
  documents: Record<string, FakeDocumentSpec>,
  order: string[],
): NonNullable<ScenarioCase['engine']> => ({
  documents,
  responses: order.map((key) => ({ path: 'open', times: 1, value: { $document: key } })),
});

import { definePlugin } from '@embedpdf/core';
import { registerPlugin } from '../scenario';
import { StampToken } from '../../../../packages/plugin/stamp/src/contract';
import {
  memoryStampStore,
  persistStampLibraries,
} from '../../../../packages/plugin/stamp/src/persistence';
/** A controllable store port for the current persistence helper's pending-work contract. */
registerPlugin('stamp-mutation-persistence', {
  factory: (_, env) =>
    definePlugin({
      id: 'stamp-mutation-persistence',
      token: env.token('stamp-persistence'),
      scope: 'workspace',
      requires: [StampToken],
      create: (ctx) => {
        const memory = memoryStampStore();
        let stop = () => {};
        ctx.cleanup(() => stop());
        return {
          api: {
            persist: () => {
              stop();
              stop = persistStampLibraries(ctx.get(StampToken), {
                list: () => memory.list(),
                delete: (id) => memory.delete(id),
                put: async (id, bytes) => {
                  env.log('store.put', [id, bytes]);
                  if (id === 'a')
                    await new Promise<void>((resolve, reject) =>
                      env.defer('put:a', () => resolve(), reject),
                    );
                  await memory.put(id, bytes);
                },
              });
            },
            rows: () => memory.list(),
            stop: () => stop(),
          },
        };
      },
    }),
});
