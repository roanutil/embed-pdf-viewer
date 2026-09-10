/**
 * Cold-start instrumentation.
 *
 * Four marks, so the wasm cost is attributable rather than inferred: module
 * entry, wasm instantiated, first display list produced, first paint. typescript-baseline has
 * nothing to instantiate and reports 0 for that leg, which is the point of
 * measuring it at all.
 *
 * Read the numbers from the console, or from `window.__coldStart` after load.
 */
export interface ColdStart {
  boot: number;
  wasmReady: number | null;
  firstFrame: number | null;
  fcp: number | null;
}

const state: ColdStart = { boot: 0, wasmReady: null, firstFrame: null, fcp: null };

declare global {
  interface Window {
    __coldStart?: ColdStart;
  }
}

export function markBoot(): void {
  state.boot = performance.now();
  performance.mark('epdf:boot');
  window.__coldStart = state;

  try {
    new PerformanceObserver((list) => {
      for (const e of list.getEntries()) {
        if (e.name === 'first-contentful-paint') state.fcp = e.startTime;
      }
    }).observe({ type: 'paint', buffered: true });
  } catch {
    // Paint timing is not available everywhere; the other three legs still are.
  }
}

export function markWasmReady(): void {
  state.wasmReady = performance.now();
  performance.mark('epdf:wasm-ready');
}

export function markFirstFrame(): void {
  if (state.firstFrame !== null) return;
  state.firstFrame = performance.now();
  performance.mark('epdf:first-frame');
  // One line, after the fact, so reading it does not need the devtools open.
  queueMicrotask(() => {
    const { boot, wasmReady, firstFrame, fcp } = state;
    const leg = (v: number | null) => (v === null ? 'n/a' : `${v.toFixed(1)}ms`);
    console.info(
      `cold start: boot=${leg(boot)} wasm=${leg(wasmReady)} firstFrame=${leg(firstFrame)} fcp=${leg(fcp)}` +
        (wasmReady !== null ? ` (wasm leg ${(wasmReady - boot).toFixed(1)}ms)` : ''),
    );
  });
}
