import { useCallback, useMemo, useSyncExternalStore } from 'react';
import { scene as sceneOf } from '@poc/shapes';
import type { DisplayList, Model, Msg, ViewEnv } from '@poc/shapes';
import { useStore } from './context';
import { bumpStats, stats, subscribeStats } from './stats';
import type { InteropStats } from './stats';

/** The model, re-read on every store change. */
export function useModel(): Model {
  const store = useStore();
  return useSyncExternalStore(store.subscribe, store.getModel, store.getModel);
}

export function useDispatch(): (msg: Msg) => void {
  const store = useStore();
  return useCallback((msg: Msg) => store.dispatch(msg), [store]);
}

/**
 * The per-frame projection. ONE call for the whole model, never one per shape.
 *
 * In this build that choice is invisible: it is a plain function call either
 * way. It is written this way so the rust-geometry-only and rust-geometry-and-state ports do not have to
 * change the renderer at all.
 */
export function useScene(view: ViewEnv): DisplayList {
  const model = useModel();
  return useMemo(() => {
    bumpStats({ sceneCalls: 1 });
    return sceneOf(model, view);
  }, [model, view]);
}

export function useInteropStats(): InteropStats {
  return useSyncExternalStore(subscribeStats, stats, stats);
}
