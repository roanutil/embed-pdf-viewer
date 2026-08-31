/**
 * Interop instrumentation. It lives in the ADAPTER, not in the pure packages,
 * because counting calls is a shell concern.
 *
 * In this all-TypeScript build `crossings` is always 0. The number is here so
 * that the poc-2 and poc-3 demos, which are otherwise the same code, can show
 * what the port actually costs per frame.
 */
export interface InteropStats {
  kind: string;
  sceneCalls: number;
  hitTests: number;
  /** Calls that crossed a language boundary. */
  crossings: number;
}

export const stats: InteropStats = {
  kind: 'TypeScript only',
  sceneCalls: 0,
  hitTests: 0,
  crossings: 0,
};

const listeners = new Set<() => void>();

/**
 * Publishing is DEFERRED to a microtask.
 *
 * `useScene` counts a frame from inside its `useMemo`, which runs during render.
 * Notifying subscribers synchronously from there made `useInteropStats` call
 * setState mid-render, and React said so: "Cannot update a component while
 * rendering a different component". Coalescing into a microtask keeps the
 * counting where it belongs and moves the notification after render.
 */
let scheduled = false;
function publish(): void {
  if (scheduled) return;
  scheduled = true;
  queueMicrotask(() => {
    scheduled = false;
    listeners.forEach((l) => l());
  });
}

export function bumpStats(patch: Partial<InteropStats>): void {
  stats.sceneCalls += patch.sceneCalls ?? 0;
  stats.hitTests += patch.hitTests ?? 0;
  stats.crossings += patch.crossings ?? 0;
  publish();
}

export function resetStats(): void {
  stats.sceneCalls = 0;
  stats.hitTests = 0;
  stats.crossings = 0;
  publish();
}

export function subscribeStats(listener: () => void): () => void {
  listeners.add(listener);
  return () => void listeners.delete(listener);
}
