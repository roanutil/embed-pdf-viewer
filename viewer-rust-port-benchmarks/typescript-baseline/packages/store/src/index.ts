import { emptyModel, update } from '@poc/shapes';
import type { Effect, Model, Msg } from '@poc/shapes';

export type Unsubscribe = () => void;
export type EffectHandler = (effect: Effect) => void;

/**
 * The store. Holds the model, runs `update`, and fans out two channels:
 * `subscribe` for "state changed" and `subscribeAction` for "a msg was
 * dispatched".
 *
 * Change notification is non-re-entrant: a listener that dispatches gets its
 * pass queued rather than nested, which keeps listener order deterministic.
 */
export interface Store {
  getModel(): Model;
  dispatch(msg: Msg): void;
  subscribe(listener: () => void): Unsubscribe;
  subscribeAction(listener: (msg: Msg) => void): Unsubscribe;
  /** The shell's side-effect sink. The brain only ever describes effects. */
  onEffect(handler: EffectHandler): Unsubscribe;
  destroy(): void;
}

export function createStore(
  initial: Model = emptyModel(),
  report: (e: unknown) => void = console.error,
): Store {
  let model = initial;
  const changeListeners = new Set<() => void>();
  const actionListeners = new Set<(m: Msg) => void>();
  const effectHandlers = new Set<EffectHandler>();

  const guarded = (fn: () => void) => {
    try {
      fn();
    } catch (e) {
      report(e);
    }
  };

  let emitting = false;
  let pending = false;
  const emitChange = () => {
    if (emitting) {
      pending = true;
      return;
    }
    emitting = true;
    try {
      do {
        pending = false;
        changeListeners.forEach((l) => guarded(l));
      } while (pending);
    } finally {
      emitting = false;
    }
  };

  return {
    getModel: () => model,

    dispatch(msg) {
      const [next, effects] = update(model, msg);
      if (next !== model) {
        model = next;
        emitChange();
      }
      actionListeners.forEach((l) => guarded(() => l(msg)));
      for (const effect of effects) {
        effectHandlers.forEach((h) => guarded(() => h(effect)));
      }
    },

    subscribe(listener) {
      changeListeners.add(listener);
      return () => void changeListeners.delete(listener);
    },

    subscribeAction(listener) {
      actionListeners.add(listener);
      return () => void actionListeners.delete(listener);
    },

    onEffect(handler) {
      effectHandlers.add(handler);
      return () => void effectHandlers.delete(handler);
    },

    destroy() {
      model = emptyModel();
      changeListeners.clear();
      actionListeners.clear();
      effectHandlers.clear();
    },
  };
}
