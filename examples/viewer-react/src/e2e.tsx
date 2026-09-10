// Dedicated test entry: the production React wrapper and built-in viewer door.
// All actions come from Playwright mouse/keyboard input. The bridge only reads
// public capabilities; it never opens a document or drives a plugin itself.
import * as React from 'react';
import { createRoot } from 'react-dom/client';
import { PDFViewer, StageToken, SearchToken, SelectionToken } from '@embedpdf/viewer-react';
import type { ViewerHandle } from '@embedpdf/viewer-react';

const backend = new URLSearchParams(location.search).get('backend');
if (backend !== 'typescript') throw new Error(`Unsupported viewer test backend: ${backend}`);

function ready(viewer: ViewerHandle) {
  window.viewerBaseline = {
    snapshot: () => {
      const stage = viewer.tryGet(StageToken);
      const search = viewer.tryGet(SearchToken);
      return {
        backend,
        documents: viewer.documents.list().map(({ status }) => status),
        currentPage: stage?.currentPage(),
        zoom: stage?.zoomLevel(),
        pages:
          stage?.visiblePages().map((p) => ({
            index: p.pageIndex,
            x: p.screenX,
            y: p.screenY,
            width: p.transform.contentWidth,
            height: p.transform.contentHeight,
          })) ?? [],
        search: search && {
          status: search.status(),
          count: search.hitCount(),
          active: search.activeIndex(),
        },
      };
    },
    selectionText: () => viewer.get(SelectionToken).readText(),
  };
}

declare global {
  interface Window {
    viewerBaseline: {
      snapshot: () => {
        backend: string | null;
        documents: string[];
        currentPage?: number;
        zoom?: number;
        pages: { index: number; x: number; y: number; width: number; height: number }[];
        search: { status: string; count: number; active: number } | null;
      };
      selectionText: () => Promise<string>;
    };
  }
}

createRoot(document.getElementById('root')!).render(
  <PDFViewer
    src="/baseline.pdf"
    locale="en"
    theme="light"
    onReady={ready}
    style={{ height: '100vh', display: 'block' }}
  />,
);
