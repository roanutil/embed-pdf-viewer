import { countCrossing } from './crossings';
import { required } from './init';
import type { CoreHost } from './init';
import type { DisplayItem, DisplayList, Effect, Msg, ShapeRow, ViewEnv } from './types';

export type Unsubscribe = () => void;
export type EffectHandler = (effect: Effect) => void;

/**
 * The TypeScript face of the Rust-owned store.
 *
 * The interesting problem this class solves is that `useSyncExternalStore`
 * wants a `getSnapshot` that returns a value stable under `===` between
 * changes. In typescript-baseline that was free: the model was an immutable object and the
 * reducer returned the same reference when nothing changed.
 *
 * The model now lives in linear memory. There is no object to compare, and
 * reading one out per render would allocate a new one every time and loop React
 * forever. So the snapshot is the Rust `version` counter, a `number`, which is
 * stable under `===` by construction and changes exactly when the model does.
 *
 * That one substitution is what makes a WASM-owned core React-friendly, and it
 * is worth more than any amount of interop cleverness.
 */
export interface CoreStore {
  /** The React snapshot: a monotonic counter, not the model. */
  getVersion(): number;
  dispatch(msg: Msg): void;
  subscribe(listener: () => void): Unsubscribe;
  onEffect(handler: EffectHandler): Unsubscribe;

  /** ONE crossing, whole frame, any number of shapes. */
  scene(view: ViewEnv): DisplayList;

  /**
   * The same frame, undecoded: `[count, (handle, 8 coords, flags) * count]`.
   *
   * `scene()` turns that buffer into `DisplayList` objects because an SVG
   * renderer wants objects. That decode allocates one item plus four points per
   * shape, and it is the SAME allocation the all-TypeScript build does, so it
   * cancels out most of what moving the math to Rust buys. A renderer that can
   * read floats directly, a canvas or WebGL path, should take this instead and
   * skip the objects entirely.
   */
  sceneRaw(view: ViewEnv): Float64Array;
  hitTest(at: { x: number; y: number }, view: ViewEnv): string | null;

  /** Cheap reads that do not need the whole model. */
  shapeCount(): number;
  /** Monotonic add counter, typescript-baseline and rust-geometry-only's `model.seq`. Never lowered by a delete. */
  seq(): number;
  selectedId(): string | null;
  selectedRow(): ShapeRow | null;
  isDragging(): boolean;
  rows(): readonly ShapeRow[];

  /** Rebuilds the scene at a given size, for the crossing comparison. */
  seed(count: number, anchored: boolean): void;
  destroy(): void;
}

export function createCoreStore(host: CoreHost = {}): CoreStore {
  const wasm = required();
  const core = new wasm.Core();

  // The scene buffer's layout, from the side that writes it.
  //
  // These were `const stride = 10` and a pair of flag literals in this file,
  // which is exactly the drift `crates/core/src/model.rs` exports them to
  // prevent: adding one field to the record and bumping `ITEM_STRIDE` would
  // have left this decoder reading quad coordinates from the wrong offsets for
  // every shape past the first, with no type error and nothing thrown. Three
  // crossings per store, none per frame.
  countCrossing();
  const stride = wasm.itemStride();
  countCrossing();
  const flagSelected = wasm.flagSelected();
  countCrossing();
  const flagProjected = wasm.flagProjected();
  // Flags are the record's last slot: `scene_buffer` pushes them after the
  // eight quad coordinates, so this follows the stride instead of being a
  // second literal that has to be remembered separately.
  const flagsAt = stride - 1;

  // Installing the logger is a crossing only when there is a logger to install.
  if (host.log) {
    countCrossing();
    core.setHostLogger((m: unknown) => host.log!(String(m)));
  }

  const listeners = new Set<() => void>();
  const effectHandlers = new Set<EffectHandler>();

  // The row cache, and the reason a frame costs exactly one crossing.
  //
  // The obvious implementation reads `core.tableVersion` every frame to decide
  // whether the cache is stale. That is a crossing per frame spent asking a
  // question whose answer is almost always no. Instead Rust tells us when
  // anything changed at all, and only then do we pay to ask whether the change
  // touched the table. A pure view change dispatches nothing, so it never marks
  // the cache dirty and `scene()` is the only hop.
  //
  // What counts as touching the table is `affects_shape_table` in
  // `crates/core/src/model.rs`, and it covers every `ShapeRow` field rather
  // than just the shape count. It has to: `rot` and `anchored` are rows, and
  // gating on the count alone left this cache serving stale values through
  // every `setRot` and `setAnchored`.
  let tableDirty = true;
  let cachedTableVersion = -1;
  let rows: readonly ShapeRow[] = [];
  /**
   * handle -> row, rebuilt with the table.
   *
   * The first version looked this up with `rows.find(...)` inside the per-item
   * decode loop, which made `scene()` O(n^2). It was invisible at demo sizes and
   * catastrophic at 10,000 shapes: 184x slower than the all-TypeScript build,
   * for a frame that still crossed the boundary exactly once. Crossing count is
   * not cost.
   */
  let rowByHandle = new Map<number, ShapeRow>();

  /**
   * The version, mirrored on this side.
   *
   * `useSyncExternalStore` calls `getSnapshot` several times per render, and a
   * getter on a wasm-bindgen struct is a boundary crossing every time. Reading
   * it live cost six crossings per re-render for a value that only changes when
   * we already know it changed. So the mirror is refreshed in `notify()` and
   * `getVersion()` is free.
   */
  countCrossing();
  let cachedVersion = core.version;

  /**
   * Invalidation happens HERE, after `dispatch` has returned, not from a
   * callback Rust holds.
   *
   * The first version of this had Rust call a JS change listener from inside
   * `dispatch`. That looked cleaner and was wrong: `dispatch` is `&mut self`,
   * so the Core was mutably borrowed while React synchronously re-rendered and
   * read `core.version`. wasm-bindgen caught the recursive borrow and the
   * damage surfaced later as "attempted to take ownership of Rust value while
   * it was borrowed" on `free()`.
   */
  const notify = () => {
    countCrossing();
    cachedVersion = core.version;
    tableDirty = true;
    listeners.forEach((l) => {
      try {
        l();
      } catch (e) {
        console.error(e);
      }
    });
  };

  const table = (): readonly ShapeRow[] => {
    if (!tableDirty) return rows;
    countCrossing();
    const v = core.tableVersion;
    if (v !== cachedTableVersion) {
      countCrossing();
      rows = core.shapeTable() as ShapeRow[];
      rowByHandle = new Map(rows.map((r) => [r.handle, r]));
      cachedTableVersion = v;
    }
    tableDirty = false;
    return rows;
  };

  const byHandle = (handle: number): ShapeRow | undefined => {
    table();
    return rowByHandle.get(handle);
  };

  return {
    getVersion: () => cachedVersion,

    dispatch(msg) {
      countCrossing();
      const { changed, effects } = core.dispatch(msg) as {
        changed: boolean;
        effects: Effect[];
      };
      if (changed) notify();
      for (const effect of effects) {
        effectHandlers.forEach((h) => {
          try {
            h(effect);
          } catch (e) {
            console.error(e);
          }
        });
      }
    },

    subscribe(listener) {
      listeners.add(listener);
      return () => void listeners.delete(listener);
    },

    onEffect(handler) {
      effectHandlers.add(handler);
      return () => void effectHandlers.delete(handler);
    },

    sceneRaw(view) {
      countCrossing();
      return core.scene(view.zoom, view.rotation);
    },

    scene(view) {
      table();
      countCrossing();
      const buf = core.scene(view.zoom, view.rotation);
      const count = buf[0] ?? 0;
      const items: DisplayItem[] = [];
      for (let i = 0; i < count; i++) {
        const o = 1 + i * stride;
        const handle = buf[o]!;
        const flags = buf[o + flagsAt]!;
        const row = rowByHandle.get(handle);
        items.push({
          handle,
          id: row?.id ?? `s${handle}`,
          quad: [
            { x: buf[o + 1]!, y: buf[o + 2]! },
            { x: buf[o + 3]!, y: buf[o + 4]! },
            { x: buf[o + 5]!, y: buf[o + 6]! },
            { x: buf[o + 7]!, y: buf[o + 8]! },
          ],
          color: row?.color ?? '#888',
          selected: (flags & flagSelected) !== 0,
          projected: (flags & flagProjected) !== 0,
        });
      }
      return { items };
    },

    hitTest(at, view) {
      countCrossing();
      const handle = core.hitTest(at.x, at.y, view.zoom, view.rotation);
      return handle < 0 ? null : (byHandle(handle)?.id ?? `s${handle}`);
    },

    shapeCount() {
      countCrossing();
      return core.shapeCount;
    },

    seq() {
      countCrossing();
      return core.seq;
    },

    selectedId() {
      countCrossing();
      const h = core.selectedHandle;
      return h === undefined ? null : (byHandle(h)?.id ?? `s${h}`);
    },

    selectedRow() {
      countCrossing();
      const h = core.selectedHandle;
      return h === undefined ? null : (byHandle(h) ?? null);
    },

    isDragging() {
      countCrossing();
      return core.isDragging;
    },

    rows: table,

    seed(count, anchored) {
      countCrossing();
      core.seedShapes(count, anchored);
      notify();
      host.log?.(`seeded ${count} shapes, anchored=${anchored}`);
    },

    destroy() {
      listeners.clear();
      effectHandlers.clear();
      // Before free(), and not only for tidiness: the logger closure captures
      // the caller's `host`, which in the React demo captures an unmounted
      // component's setState.
      countCrossing();
      core.clearHostLogger();
      core.free();
    },
  };
}
