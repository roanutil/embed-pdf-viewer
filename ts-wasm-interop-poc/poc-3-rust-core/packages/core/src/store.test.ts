import { beforeAll, describe, expect, it, vi } from 'vitest';
import { initCoreFromDisk } from './init-node';
import { createCoreStore, crossings, resetCrossings } from './index';
import type { CoreStore, Effect, ViewEnv } from './index';

const V = (zoom: number, rotation: 0 | 90 | 180 | 270 = 0): ViewEnv => ({ zoom, rotation });

beforeAll(async () => {
  await initCoreFromDisk();
});

const fresh = (): CoreStore => createCoreStore();

describe('the boundary', () => {
  it('lets Rust call back into TypeScript', () => {
    const lines: string[] = [];
    const store = createCoreStore({ log: (m) => lines.push(m) });
    expect(lines.some((l) => l.startsWith('poc-core '))).toBe(true);
    store.destroy();
  });

  it('costs THREE crossings on the first frame after the shape set changes', () => {
    const store = fresh();
    store.seed(10, true);

    resetCrossings();
    store.scene(V(3, 90));
    // topologyVersion, then shapeTable because it moved, then scene.
    expect(crossings()).toBe(3);
    store.destroy();
  });

  it('costs exactly ONE crossing per frame at 10 shapes', () => {
    const store = fresh();
    store.seed(10, true);
    store.scene(V(3, 90)); // warm the id/colour cache

    resetCrossings();
    store.scene(V(2, 90));
    expect(crossings()).toBe(1);
    store.destroy();
  });

  it('costs exactly ONE crossing per frame at 200 shapes too', () => {
    const store = fresh();
    store.seed(200, true);
    store.scene(V(3, 90));

    resetCrossings();
    store.scene(V(2, 90));
    // poc-2 cost 7 crossings per anchored shape, so 200 shapes would have been
    // 1,400 hops. Here the count does not depend on the shape count at all.
    expect(crossings()).toBe(1);
    store.destroy();
  });

  it('holds at one crossing per frame across a zoom sweep', () => {
    const store = fresh();
    store.seed(5, true);
    store.scene(V(1));

    resetCrossings();
    for (let i = 0; i < 10; i++) store.scene(V(1 + i * 0.1, 90));
    expect(crossings()).toBe(10);
    store.destroy();
  });

  it('costs TWO crossings on the frame after a non-topological change', () => {
    const store = fresh();
    store.seed(3, true);
    store.scene(V(2));

    store.dispatch({ t: 'setRot', id: 's1', rot: 45 });
    resetCrossings();
    store.scene(V(2));
    // The change listener marked the cache dirty, so we pay once to learn the
    // shape SET did not move, then once for the frame.
    expect(crossings()).toBe(2);
    store.destroy();
  });
});

describe('the version counter', () => {
  it('is a number, so useSyncExternalStore can compare it with ===', () => {
    const store = fresh();
    expect(typeof store.getVersion()).toBe('number');
    expect(store.getVersion()).toBe(store.getVersion());
    store.destroy();
  });

  it('moves on a change and holds still on a no-op', () => {
    const store = fresh();
    const before = store.getVersion();
    store.dispatch({ t: 'add', at: { x: 50, y: 50 }, color: '#c00' });
    const after = store.getVersion();
    expect(after).toBeGreaterThan(before);

    store.dispatch({ t: 'delete', id: 's999' });
    expect(store.getVersion()).toBe(after);
    store.destroy();
  });

  it('notifies subscribers from the Rust side', () => {
    const store = fresh();
    const listener = vi.fn();
    store.subscribe(listener);
    store.dispatch({ t: 'add', at: { x: 10, y: 10 }, color: '#c00' });
    expect(listener).toHaveBeenCalledTimes(1);

    store.dispatch({ t: 'delete', id: 's999' });
    expect(listener).toHaveBeenCalledTimes(1);
    store.destroy();
  });

  it('separates the topology version from the state version', () => {
    const store = fresh();
    store.dispatch({ t: 'add', at: { x: 10, y: 10 }, color: '#c00' });
    const rowsBefore = store.rows();

    // A rotation changes state but not the shape set, so the cached table
    // object survives. That identity IS the optimisation.
    store.dispatch({ t: 'setRot', id: 's1', rot: 45 });
    expect(store.rows()).toBe(rowsBefore);

    store.dispatch({ t: 'add', at: { x: 200, y: 200 }, color: '#0c0' });
    expect(store.rows()).not.toBe(rowsBefore);
    store.destroy();
  });
});

describe('messages and effects cross intact', () => {
  it('round-trips the whole Msg union without a serde error', () => {
    const store = fresh();
    store.dispatch({ t: 'add', at: { x: 100, y: 100 }, color: '#c00' });
    expect(() => {
      store.dispatch({ t: 'select', id: 's1' });
      store.dispatch({ t: 'select', id: null });
      store.dispatch({ t: 'pointerDown', at: { x: 100, y: 100 }, view: V(2, 90) });
      store.dispatch({ t: 'pointerMove', at: { x: 110, y: 100 } });
      store.dispatch({ t: 'pointerUp' });
      store.dispatch({ t: 'setAnchored', id: 's1', anchored: true });
      store.dispatch({ t: 'setRot', id: 's1', rot: 30 });
      store.dispatch({ t: 'delete', id: 's1' });
    }).not.toThrow();
    expect(store.shapeCount()).toBe(0);
    store.destroy();
  });

  it('rejects a message the Rust union does not know', () => {
    const store = fresh();
    // The drift failure the hand-written types invite: a runtime error, not a
    // compile error.
    expect(() => store.dispatch({ t: 'nope' } as never)).toThrow(/bad message/);
    store.destroy();
  });

  it('hands effects out in the TypeScript shape', () => {
    const store = fresh();
    const effects: Effect[] = [];
    store.onEffect((e) => effects.push(e));

    store.dispatch({ t: 'add', at: { x: 0, y: 0 }, color: '#000' });
    expect(effects).toEqual([
      { t: 'log', message: 'added s1' },
      { t: 'persist', ids: ['s1'] },
    ]);
    store.destroy();
  });
});

describe('the decoded display list', () => {
  it('carries ids, colours and flags back from the flat buffer', () => {
    const store = fresh();
    store.dispatch({ t: 'add', at: { x: 100, y: 100 }, color: '#abcdef' });
    store.dispatch({ t: 'setAnchored', id: 's1', anchored: true });

    const plain = store.scene(V(1));
    expect(plain.items).toHaveLength(1);
    expect(plain.items[0]).toMatchObject({
      handle: 1,
      id: 's1',
      color: '#abcdef',
      selected: true,
      projected: false,
    });

    const zoomed = store.scene(V(3));
    expect(zoomed.items[0]!.projected).toBe(true);
    // The projection holds the bounds top-left.
    expect(zoomed.items[0]!.quad[0]).toEqual(plain.items[0]!.quad[0]);
    store.destroy();
  });

  it('hit-tests through the projection, in Rust', () => {
    const store = fresh();
    store.dispatch({ t: 'add', at: { x: 100, y: 100 }, color: '#c00' });
    store.dispatch({ t: 'setAnchored', id: 's1', anchored: true });

    expect(store.hitTest({ x: 100, y: 100 }, V(1))).toBe('s1');
    // At zoom 4 the shape shrinks toward its anchor, so the old centre misses.
    expect(store.hitTest({ x: 100, y: 100 }, V(4))).toBeNull();
    store.destroy();
  });
});
