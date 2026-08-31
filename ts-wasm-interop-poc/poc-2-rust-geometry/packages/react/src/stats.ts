/**
 * Interop instrumentation for the Rust-geometry build.
 *
 * In poc-1 `crossings` was hard-wired to 0. Here it reads the counter inside
 * `@poc/geometry`, which increments on every single call into wasm, so the
 * number on screen is the real number.
 */
import { crossings as wasmCrossings, resetCrossings, subscribeCrossings } from '@poc/geometry';

export interface InteropStats {
  kind: string;
  sceneCalls: number;
  hitTests: number;
  crossings: number;
}

let sceneCalls = 0;
let hitTests = 0;
let snapshot: InteropStats = {
  kind: 'Rust geometry, TypeScript brain',
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
    snapshot = { kind: snapshot.kind, sceneCalls, hitTests, crossings: wasmCrossings() };
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
  resetCrossings();
  publish();
}

export function subscribeStats(listener: () => void): () => void {
  listeners.add(listener);
  // Crossings move without any bumpStats() call, because geometry counts them
  // from inside. Mirror that channel or the meter freezes between renders.
  const offWasm = subscribeCrossings(publish);
  return () => {
    listeners.delete(listener);
    offWasm();
  };
}
