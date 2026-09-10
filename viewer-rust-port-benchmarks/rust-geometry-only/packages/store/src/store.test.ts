import { beforeAll, describe, expect, it, vi } from 'vitest';
import { initGeometryFromDisk } from '@poc/geometry/node';
import { createStore } from './index';

// The store does not touch geometry directly. It reaches it through the brain,
// which is exactly how an async leaf ends up gating a package that has no
// business knowing about wasm.
beforeAll(async () => {
  await initGeometryFromDisk();
});

describe('createStore', () => {
  it('runs update and notifies once per changing dispatch', () => {
    const store = createStore();
    const listener = vi.fn();
    store.subscribe(listener);

    store.dispatch({ t: 'add', at: { x: 50, y: 50 }, color: '#c00' });
    expect(store.getModel().shapes).toHaveLength(1);
    expect(listener).toHaveBeenCalledTimes(1);
  });

  it('does not notify when the model is unchanged', () => {
    const store = createStore();
    const listener = vi.fn();
    store.subscribe(listener);

    store.dispatch({ t: 'delete', id: 'nope' });
    expect(listener).not.toHaveBeenCalled();
  });

  it('hands effects to the shell rather than performing them', () => {
    const store = createStore();
    const effects: string[] = [];
    store.onEffect((e) => effects.push(e.t));

    store.dispatch({ t: 'add', at: { x: 0, y: 0 }, color: '#000' });
    expect(effects).toEqual(['log', 'persist']);
  });

  it('queues a re-entrant dispatch instead of nesting it', () => {
    const store = createStore();
    const order: string[] = [];
    let armed = true;
    store.subscribe(() => {
      order.push('listener');
      if (armed) {
        armed = false;
        store.dispatch({ t: 'add', at: { x: 1, y: 1 }, color: '#000' });
      }
    });

    store.dispatch({ t: 'add', at: { x: 0, y: 0 }, color: '#000' });
    expect(order).toEqual(['listener', 'listener']);
    expect(store.getModel().shapes).toHaveLength(2);
  });
});
