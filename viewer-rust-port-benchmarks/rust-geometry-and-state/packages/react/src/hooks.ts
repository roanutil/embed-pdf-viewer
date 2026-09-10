import { useCallback, useMemo, useSyncExternalStore } from 'react';
import type { DisplayList, Msg, ShapeRow, ViewEnv } from '@poc/core';
import { useCore } from './context';
import { bumpStats, stats, subscribeStats } from './stats';
import type { InteropStats } from './stats';

/**
 * The snapshot is a NUMBER.
 *
 * This is the load-bearing line of the whole React binding. `useSyncExternalStore`
 * needs a snapshot that is stable under `===` between changes, and the model
 * lives in WASM linear memory where there is no object to compare. Returning a
 * decoded model here would allocate on every render and spin React forever.
 * Rust's monotonic `version` counter satisfies the contract exactly.
 */
export function useVersion(): number {
  const store = useCore();
  return useSyncExternalStore(store.subscribe, store.getVersion, store.getVersion);
}

export function useDispatch(): (msg: Msg) => void {
  const store = useCore();
  return useCallback((msg: Msg) => store.dispatch(msg), [store]);
}

/**
 * ONE crossing per frame, whatever the shape count.
 *
 * rust-geometry-only's equivalent hook looked identical and cost seven crossings per
 * anchored shape, because the loop over shapes ran in TypeScript and every
 * geometry helper was a boundary hop. Here the loop runs in Rust and hands back
 * one buffer. The hook's signature did not change; only what it costs did.
 */
export function useScene(view: ViewEnv): DisplayList {
  const store = useCore();
  const version = useVersion();
  return useMemo(() => {
    bumpStats({ sceneCalls: 1 });
    return store.scene(view);
  }, [store, version, view]);
}

/** Selection, read without decoding the model. */
export function useSelected(): ShapeRow | null {
  const store = useCore();
  const version = useVersion();
  return useMemo(() => store.selectedRow(), [store, version]);
}

export function useShapeCount(): number {
  const store = useCore();
  const version = useVersion();
  return useMemo(() => store.shapeCount(), [store, version]);
}

export function useInteropStats(): InteropStats {
  return useSyncExternalStore(subscribeStats, stats, stats);
}
