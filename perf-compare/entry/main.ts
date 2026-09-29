import EmbedPDF, { StageToken, SearchToken, SelectionToken, AnnotationToken } from '@perf/door';
import { setupKernel, kernelCounts } from '@perf/kernel';
import type { PerfArm } from './globals';

declare const __PERF_ARM__: PerfArm;
const arm = __PERF_ARM__;
const doc = new URLSearchParams(location.search).get('doc') ?? 'ebook';

await setupKernel();

const el = EmbedPDF.init({
  target: '#viewer',
  src: `/${doc}.pdf`,
  // `registerLocalEngine` spreads this bag into `localEngine()`, which is
  // where `rustRouting` is read on the impl branch. Other branches ignore it.
  ...(arm.rustReads ? { engine: { rustRouting: { rustReads: true } } as never } : {}),
});

const ready = new Promise<void>((resolve) =>
  el.addEventListener(
    'epdf:ready',
    () => {
      performance.mark('perf:ready');
      resolve();
    },
    { once: true },
  ),
);

const v = () => el.viewer!;

const opened = ready.then(
  () =>
    new Promise<void>((resolve) => {
      const done = () => {
        performance.mark('perf:opened');
        resolve();
      };
      const off = v().on('opened', () => {
        off();
        done();
      });
      // The document can finish opening before this listener attaches.
      if (v().documents.list().some((d) => d.status === 'ready')) {
        off();
        done();
      }
    }),
);

// The active document's page registry, not the stage's current spread.
const pages = () => v().documents.listPages();
const pageRef = (i: number) => pages()[i].ref;

window.__perf = {
  arm,
  ready,
  opened,
  pageCount: () => pages().length,
  goToPageIndex: (i) => v().get(StageToken).goToPageIndex(i, { behavior: 'instant' }),
  currentPageIndex: () => v().get(StageToken).getCurrentPageIndex(),
  zoomTo: (z) => v().get(StageToken).zoomTo(z),
  zoomLevel: () => v().get(StageToken).getZoomLevel(),
  search: async (text) => {
    await v().get(SearchToken).search({ text });
    return v().get(SearchToken).getHitCount();
  },
  clearSearch: () => v().get(SearchToken).clear(),
  readSelection: () => v().get(SelectionToken).readText(),
  clearSelection: () => v().get(SelectionToken).clear(),
  annotationCount: (pageIndex) => v().get(AnnotationToken).list({ page: pageRef(pageIndex) }).length,
  execute: (command) => v().execute(command),
  kernelCounts,
};
