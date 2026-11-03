// Dedicated test entry: the production React wrapper and built-in viewer door.
// All actions come from Playwright mouse/keyboard input. The bridge only reads
// public capabilities; it never opens a document or drives a plugin itself.
import * as React from 'react';
import { createRoot } from 'react-dom/client';
import {
  PDFViewer,
  StageToken,
  SearchToken,
  SelectionToken,
  AnnotationToken,
  FormToken,
} from '@embedpdf/viewer-react';
import type { Viewer as ViewerHandle } from '@embedpdf/viewer-react';

import { localEngine } from '@embedpdf/engine';
import {
  boundaryTextOffset,
  sliceText,
  type LocalDocumentHandle,
  type PageTextSnapshot,
} from '@embedpdf/engine-core/runtime';
import { pageTransform } from '@embedpdf/core-geometry';

const parityEnabled = new URLSearchParams(location.search).get('parity') === '1';
let parityDocument: LocalDocumentHandle | undefined;
const backend = new URLSearchParams(location.search).get('backend');
if (backend !== 'typescript' && backend !== 'rust') {
  throw new Error(`Unsupported viewer test backend: ${backend}`);
}

/**
 * The engine the flows run against.
 *
 * `rust` turns on the core's routing. It is ignored on a runtime payload
 * built without the core, which is why the flows assert the backend they
 * actually got rather than the one they asked for: a silent fallback would
 * make the Rust projects a second, slower copy of the TypeScript ones.
 *
 * `rustSession` stays off. G3w routes no reads, so a viewer on it could not
 * paint a page; it moves here when G10 does.
 */
const engine = parityEnabled
  ? () => {
      if (backend !== 'typescript')
        throw new Error('Parity requires the explicitly configured TypeScript reader');
      const local = localEngine({
        rustRouting: { rustReads: false, rustSession: false },
        ...window.parityFontConfiguration,
      });
      const open = local.open.bind(local);
      local.open = (...args) => {
        const operation = open(...args);
        void operation.then(
          (document) => {
            parityDocument = document;
          },
          () => {},
        );
        return operation;
      };
      return local;
    }
  : backend === 'rust'
    ? { rustRouting: { rustReads: true } }
    : undefined;

/**
 * Which fixture to open.
 *
 * `baseline` is twelve plain pages and is the oracle for F1 through F5:
 * their pixel checks, page count and match counts all read it. `features`
 * carries links, annotations and form fields, which is what F6 through F8
 * need and what adding to the baseline would have invalidated. `encrypted`
 * needs a password, which is F9's whole subject.
 */
const documents = {
  baseline: '/baseline.pdf',
  features: '/features.pdf',
  encrypted: '/encrypted.pdf',
  roundtrip: '/roundtrip.pdf',
  // The two real documents this example already ships in `public/`. Nothing
  // in the suite opens them: they are here so the same entry can be pointed
  // at a document with actual content when somebody wants to compare the
  // two backends by hand.
  ebook: '/ebook.pdf',
  testlab: '/testlab.pdf',
} as const;
const requested = new URLSearchParams(location.search).get('doc') ?? 'baseline';
if (!(requested in documents)) throw new Error(`Unknown viewer test document: ${requested}`);
const src = documents[requested as keyof typeof documents];

function ready(viewer: ViewerHandle) {
  window.viewerBaseline = {
    paritySnapshot,
    snapshot: () => {
      const stage = viewer.tryGet(StageToken);
      const search = viewer.tryGet(SearchToken);
      return {
        // What was REQUESTED. The viewer has no way to ask the worker which
        // reader served a page, so a payload without the Rust core reports
        // `rust` here and falls back underneath. The payload check in CI is
        // what makes that safe to rely on.
        backend,
        documents: viewer.documents.list().map(({ status }) => status),
        currentPage: stage?.getCurrentPageIndex(),
        zoom: stage?.getZoomLevel(),
        pages:
          stage?.listVisiblePages().map((p) => ({
            index: p.pageIndex,
            x: p.screenX,
            y: p.screenY,
            width: p.transform.contentWidth,
            height: p.transform.contentHeight,
          })) ?? [],
        search: search && {
          status: search.getStatus(),
          count: search.getHitCount(),
          active: search.getActiveHitIndex(),
        },
      };
    },
    selectionText: () => viewer.get(SelectionToken).readText(),

    // A page's links, read through the ANNOTATION plane.
    //
    // Not through `LinkToken`: while a document is being read the link
    // capability is not the one that owns this data, and the viewer does not
    // register it. The annotation plane holds the link annotations and
    // resolves each one's `/A` target, which is the same answer from the
    // side that actually has it.
    //
    // From the capability rather than from the DOM either way: the anchor
    // the link layer draws is one rendering of this, and asserting against
    // it would pin the layer's markup alongside what the engine resolved.
    links: (pon: number) => {
      const annotation = viewer.tryGet(AnnotationToken);
      if (!annotation) return null;
      return annotation
        .list({ pages: [pon] })
        .filter((item) => String((item as { subtype?: unknown }).subtype ?? '') === 'link')
        .map((item) => ({
          nm: item.nm ?? null,
          rect: item.rect,
          target: annotation.links.get(item.ref),
        }));
    },

    // What the annotation plane holds for a page. The DTOs rather than the
    // render items: a render item is one projection of these and asserting
    // against it would pin the layer's chrome settings as well as the
    // engine's answer.
    annotations: (pon: number) => {
      const annotation = viewer.tryGet(AnnotationToken);
      if (!annotation) return null;
      return annotation.list({ pages: [pon] }).map((item) => ({
        nm: item.nm ?? null,
        subtype: String((item as { subtype?: unknown }).subtype ?? ''),
        rect: item.rect,
      }));
    },

    /** The ids the annotation plane currently has selected. */
    annotationSelection: () =>
      viewer
        .tryGet(AnnotationToken)
        ?.selection.list()
        .map((item) => item.nm ?? JSON.stringify(item.ref)) ?? null,

    // Every field the form model holds, reduced to the parts a reader can
    // state from the file: the name, the family, and the value.
    /** The pages in display order, with the identity each one keeps. */
    pages: () => {
      const stage = viewer.tryGet(StageToken);
      if (!stage) return null;
      return Array.from({ length: stage.getPageCount() }, (_, index) => {
        const rect = stage.getPageFrame(index);
        return {
          index,
          pon: rect?.ref.objectNumber ?? null,
          // The rendered size, which is how a rotation shows up without
          // asking the registry for a number: a quarter turn swaps these.
          width: rect?.transform.contentWidth ?? null,
          height: rect?.transform.contentHeight ?? null,
        };
      });
    },

    /**
     * Run a search and report how many matches it found.
     *
     * F13's "the text is gone" reads through this: search runs over the
     * page's EXTRACTED text, so a needle that no longer matches is a needle
     * no longer in the text. Settling is the caller's to poll, because the
     * scan is sliced and `status()` is what says it finished.
     */
    search: (needle: string) => {
      const search = viewer.tryGet(SearchToken);
      if (!search) return null;
      search.search({ text: needle });
      return true;
    },

    /** Every open document, with the status the viewer is showing for it. */
    documentStatuses: () => viewer.documents.list().map(({ id, status }) => ({ id, status })),

    /**
     * Try a password on a locked document.
     *
     * Reports the failure rather than throwing it across the bridge, because
     * what F9 asserts is the CODE: the plan's rule is one error vocabulary
     * across both engines, and message text is not part of it.
     */
    unlock: async (password: string) => {
      const first = viewer.documents.list()[0];
      if (!first) return { ok: false, code: 'no-document' };
      try {
        await viewer.documents.unlock(first.id, { password });
        return { ok: true, code: null };
      } catch (error) {
        return { ok: false, code: String((error as { code?: unknown })?.code ?? 'unknown') };
      }
    },

    formFields: () => {
      const snapshot = viewer.tryGet(FormToken)?.getSnapshot();
      if (!snapshot) return null;
      return snapshot.fields.map((field) => ({
        name: field.name,
        family: field.family,
        value: (field as { value?: unknown }).value ?? null,
        selectedValues: (field as { selectedValues?: unknown }).selectedValues ?? null,
        checked: (field as { checked?: unknown }).checked ?? null,
      }));
    },
  };
}

declare global {
  interface Window {
    parityFontConfiguration?: Pick<
      NonNullable<Parameters<typeof localEngine>[0]>,
      'fonts' | 'fallbackFonts'
    >;
    viewerBaseline: {
      paritySnapshot: typeof paritySnapshot;
      snapshot: () => {
        backend: string | null;
        documents: string[];
        currentPage?: number;
        zoom?: number;
        pages: { index: number; x: number; y: number; width: number; height: number }[];
        search: { status: string; count: number; active: number } | null;
      };
      selectionText: () => Promise<string>;
      links: (
        pon: number,
      ) => { nm: string | null; rect: unknown; target: Record<string, unknown> | null }[] | null;
      annotations: (pon: number) => { nm: string | null; subtype: string; rect: unknown }[] | null;
      annotationSelection: () => string[] | null;
      pages: () =>
        | { index: number; pon: number | null; width: number | null; height: number | null }[]
        | null;
      search: (needle: string) => boolean | null;
      documentStatuses: () => { id: string; status: string }[];
      unlock: (password: string) => Promise<{ ok: boolean; code: string | null }>;
      formFields: () =>
        | {
            name: string;
            family: string;
            value: unknown;
            selectedValues: unknown;
            checked: unknown;
          }[]
        | null;
    };
  }
}

createRoot(document.getElementById('root')!).render(
  <PDFViewer
    src={src}
    {...(engine ? { engine } : {})}
    locale="en"
    theme="light"
    onReady={ready}
    style={{ height: '100vh', display: 'block' }}
  />,
);

type ParityProbe = {
  id: string;
  operation: string;
  page?: number;
  start?: number;
  count?: number;
  rotation?: 0 | 90 | 180 | 270;
  scale?: number;
  region?: { x: number; y: number; width: number; height: number };
  input?: Parameters<typeof pageTransform>[0];
  pagePoints?: { x: number; y: number }[];
  pageRects?: { x: number; y: number; width: number; height: number }[];
  viewPoints?: { x: number; y: number }[];
  viewRects?: { x: number; y: number; width: number; height: number }[];
  text?: Omit<PageTextSnapshot, 'charMap'> & { charMap?: { char: number; text: number }[] };
  ranges?: { start: number; count: number }[];
};

async function paritySnapshot(probes: ParityProbe[]) {
  if (!parityEnabled || backend !== 'typescript' || !parityDocument)
    throw new Error('No observed TypeScript viewer document');
  const pages = (await parityDocument.pages.list()).pages;
  const results = [];
  for (const probe of probes) {
    const layout = pages[probe.page ?? 0];
    if (!layout) throw new Error('Parity page index outside document');
    const page = parityDocument.page(layout.ref);
    const range = { start: probe.start ?? 0, count: probe.count ?? 0 };
    let value: unknown;
    switch (probe.operation) {
      case 'pages':
        value = pages.map((p) => ({ ...p, actions: p.actions ?? null }));
        break;
      case 'text': {
        const snapshot = await page.text.get();
        value = {
          ...snapshot,
          ...(snapshot.charMap
            ? { charMap: snapshot.charMap.map(([char, text]) => ({ char, text })) }
            : {}),
        };
        break;
      }
      case 'geometry':
        value = {
          runs: (await page.text.layout()).runs.map((run) => ({
            rect: run.rect,
            start: run.start,
            fontSize: run.fontSize ?? null,
            ...('rotation' in run
              ? { rotation: run.rotation, ascentFlip: Boolean(run.ascentFlip) }
              : {}),
            glyphs: run.glyphs.map((g) => ({
              loose: g.loose,
              tight: g.tight ?? null,
              space: Boolean(g.space),
              empty: Boolean(g.empty),
            })),
          })),
        };
        break;
      case 'segments':
        value = (await page.text.layout()).segments(range);
        break;
      case 'slice':
        value = await page.text.slice(range);
        break;
      case 'transform': {
        if (!probe.input) throw new Error('Missing transform input');
        const t = pageTransform(probe.input);
        value = {
          transform: Object.fromEntries(
            [
              'viewWidth',
              'viewHeight',
              'contentWidth',
              'contentHeight',
              'deviceWidth',
              'deviceHeight',
              'renderScale',
              'viewScale',
              'baseScale',
              'zoom',
              'rotation',
              'cssMatrix',
            ].map((k) => [k, t[k as keyof typeof t]]),
          ),
          pageToView: (probe.pagePoints ?? []).map(t.pageToView),
          pageRectsToView: (probe.pageRects ?? []).map(t.pageToViewRect),
          viewToPage: (probe.viewPoints ?? []).map(t.viewToPage),
          viewRectsToPage: (probe.viewRects ?? []).map(t.viewToPageRect),
        };
        break;
      }
      case 'render': {
        const image = await page.render.raw({
          target: probe.region ? { kind: 'rect', rect: probe.region } : { kind: 'page' },
          viewport: { kind: 'scale', scale: probe.scale },
          rotation: probe.rotation,
          background: 'transparent',
          includeAnnotations: false,
          includeFormFields: false,
        });
        const digest = await crypto.subtle.digest('SHA-256', image.data);
        const bytes = new Uint8Array(image.data);
        const chunks: string[] = [];
        for (let index = 0; index < bytes.length; index += 32768)
          chunks.push(String.fromCharCode(...bytes.subarray(index, index + 32768)));
        value = {
          width: image.width,
          height: image.height,
          stride: image.stride,
          area: image.transform.pixelsToPage({
            x: 0,
            y: 0,
            width: image.width,
            height: image.height,
          }),
          color: image.color,
          premultipliedAlpha: image.premultipliedAlpha,
          pixels: btoa(chunks.join('')),
          sha256: Array.from(new Uint8Array(digest), (n) => n.toString(16).padStart(2, '0')).join(
            '',
          ),
        };
        break;
      }
      case 'helperText': {
        if (!probe.text) throw new Error('Missing text snapshot');
        const text: PageTextSnapshot = {
          ...probe.text,
          charMap: probe.text.charMap?.map(({ char, text }) => [char, text] as const),
        };
        value = {
          offsets: Array.from({ length: text.charCount + 1 }, (_, i) =>
            boundaryTextOffset(text, i),
          ),
          slices: (probe.ranges ?? []).map((r) =>
            sliceText(text, { start: r.start, count: r.count }),
          ),
        };
        break;
      }
      default:
        throw new Error(`Unknown parity operation ${probe.operation}`);
    }
    results.push({ id: probe.id, operation: probe.operation, value });
  }
  return JSON.stringify(results);
}
