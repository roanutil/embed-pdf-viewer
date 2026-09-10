/**
 * Interop instrumentation. It lives in the ADAPTER, not in the pure packages,
 * because counting calls is a shell concern.
 *
 * In this all-TypeScript build `crossings` is always 0. The number is here so
 * that the rust-geometry-only and rust-geometry-and-state demos, which are otherwise the same code, can show
 * what the port actually costs per frame.
 */
export interface InteropStats {
  kind: string;
  sceneCalls: number;
  hitTests: number;
  /** Calls that crossed a language boundary. */
  crossings: number;
}

let sceneCalls = 0;
let hitTests = 0;

/**
 * The snapshot `useSyncExternalStore` compares, replaced on every publish.
 *
 * It has to be a NEW object. The first version exported one mutable
 * `InteropStats` and handed that same reference over as `getSnapshot`; React
 * compares snapshots with `Object.is`, so every notification bailed out, the
 * Interop panel froze at its first-render values, and "reset counters" did
 * nothing visible.
 */
let snapshot: InteropStats = {
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
    // A fresh object, so useSyncExternalStore sees the change.
    snapshot = { kind: snapshot.kind, sceneCalls, hitTests, crossings: 0 };
    listeners.forEach((l) => l());
  });
}

export const stats = (): InteropStats => snapshot;

export function bumpStats(patch: { sceneCalls?: number; hitTests?: number }): void {
  sceneCalls += patch.sceneCalls ?? 0;
  hitTests += patch.hitTests ?? 0;
  publish();
}

export function resetStats(): void {
  sceneCalls = 0;
  hitTests = 0;
  publish();
}

export function subscribeStats(listener: () => void): () => void {
  listeners.add(listener);
  return () => void listeners.delete(listener);
}
