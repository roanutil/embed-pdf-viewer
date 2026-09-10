/**
 * Interop instrumentation for the Rust-geometry build.
 *
 * In typescript-baseline `crossings` was hard-wired to 0. Here it reads the counter inside
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

/**
 * The crossings mirror is REFCOUNTED, not one subscription per subscriber.
 *
 * Crossings move without any `bumpStats()` call, because geometry counts them
 * from inside, so this channel has to be mirrored or the meter freezes between
 * renders. Mirroring it per subscriber does not work: `subscribeCrossings`
 * keeps its listeners in a `Set` and `publish` is one module-level function, so
 * a second subscribe deduped to the same entry while still handing back a
 * deleter for it. Two components calling `useInteropStats()` and one
 * unmounting then froze the survivor's crossings meter, which reads as "the
 * port is free" rather than as a broken subscription.
 */
let wasmMirrors = 0;
let offWasm: (() => void) | null = null;

export function subscribeStats(listener: () => void): () => void {
  listeners.add(listener);
  if (wasmMirrors === 0) offWasm = subscribeCrossings(publish);
  wasmMirrors += 1;
  let live = true;
  return () => {
    if (!live) return;
    live = false;
    listeners.delete(listener);
    wasmMirrors -= 1;
    if (wasmMirrors === 0) {
      offWasm?.();
      offWasm = null;
    }
  };
}
