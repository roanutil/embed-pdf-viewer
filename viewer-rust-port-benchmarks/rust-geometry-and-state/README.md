# rust-geometry-and-state

Geometry, the brain and the store all live in Rust. TypeScript holds a handle, a version number and
an id cache. This is the shape the port plan recommends, and the numbers are the reason.

```
crates/core/src/geom.rs    120 lines. The math.
crates/core/src/model.rs   377 lines. The brain: Msg, Effect, update, scene_buffer, hit_test.
crates/core/src/lib.rs     205 lines. The wasm surface, and that is the whole API.
packages/core              The TS wrapper: initCore + createCoreStore.
packages/react             useSyncExternalStore over the Rust-owned store.
```

The Rust replaces 472 lines of TypeScript (`geometry` 99 plus `shapes` 276 plus `store` 97) with 702
lines of Rust. Some of that is `#[wasm_bindgen]` glue that a Swift binding would not need, and some
is the explicitness Rust asks for. Do not read it as a porting ratio; a 1.5x line count on a toy
says nothing about 16,000 lines of engine services.

Run it:

```
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.127
pnpm install
pnpm build          # http://localhost:5173 after pnpm dev
pnpm test           # 18 TypeScript tests, including a jsdom render
cargo test          # 18 Rust tests, 2 of them against ../vectors/anchor-vectors.json
```

## One crossing per frame

`packages/core/src/store.test.ts` asserts it exactly, at both sizes:

```ts
store.seed(200, true);
store.scene(V(3, 90));       // warm the id cache
resetCrossings();
store.scene(V(2, 90));
expect(crossings()).toBe(1);
```

rust-geometry-only cost 7 crossings per anchored shape, so 200 shapes there would be 1,400 hops. Here the count
does not depend on the shape count at all, because the loop over shapes runs in Rust and comes back
as one `Float64Array`.

The full picture, all asserted: 1 crossing in the steady state, 2 on the frame after any change,
3 on the frame after the shape SET changes. The 3 is the id and colour table being re-read; the 2 is
learning that the set did *not* change.

`packages/react/src/binding.test.tsx` takes it further and mounts the thing in jsdom: a re-render
with the same store, view and version costs **zero** crossings, and 200 shapes render to 200 SVG
paths for one hop.

## The two ideas that make it work

The React snapshot is a number. `useSyncExternalStore` needs a `getSnapshot` that is stable under
`===`, and the model lives in linear memory where there is no object to compare. Returning a decoded
model would allocate every render and spin React forever. Rust's monotonic `version` counter
satisfies the contract by construction. That one substitution is worth more than any interop
cleverness, and it is why the store's public shape is `getVersion()` rather than `getModel()`.

The display list is split by change frequency. Quads change every frame; ids, colours, `rot` and
`anchored` change only when a shape is edited. So `scene()` returns a flat buffer of
`[handle, 8 coords, flags]` per shape and nothing else, and `shapeTable()` returns the slow half,
re-read only when Rust's `tableVersion` moves. Zooming never touches it. `affects_shape_table` in
`crates/core/src/model.rs` is the list of messages that do, and it has to cover every `ShapeRow`
field: gating on the shape count alone left `setRot` and `setAnchored` invisible to the cache, so the
demo's rot readout and anchored checkbox rendered stale values until an unrelated add came along.

## Two bugs worth stealing

**A `&mut self` export must not call back into JS.** The first version had Rust hold a change listener
and call it from inside `dispatch`. `dispatch` is `&mut self`, so wasm-bindgen held a mutable borrow
for its duration, React synchronously re-rendered inside the callback and read `core.version`, and
the recursive borrow surfaced much later as:

```
attempted to take ownership of Rust value while it was borrowed
```

thrown from `free()`, nowhere near the cause. `dispatch` now returns `{ changed, effects }` and the
wrapper notifies after the call returns. `setHostLogger` takes `&self` on purpose, and the doc
comment says out loud that its callback may read but must not dispatch.

**`getSnapshot` runs more than you think.** React calls it several times per render, and a
wasm-bindgen getter is a crossing every time. Reading `core.version` live cost six crossings per
re-render for a value we already knew had changed. The wrapper mirrors it in a `cachedVersion` that
`notify()` refreshes, so `getVersion()` is free.

Both were found by the jsdom render test, not by the unit tests. Neither would have shown up without
a React tree actually rendering.

## What moved, and what that cost

Every brain test moved from `packages/shapes/src/shapes.test.ts` into `crates/core/tests/brain.rs`.
That is the trade: the logic is now shareable with a Swift or Kotlin binding, and it is no longer
testable from TypeScript without going through wasm. `crates/core/tests/brain.rs` also pins the
serde tagging in both directions, because a `Msg` shape mismatch is a runtime `bad message` string
rather than a type error:

```rust
r##"{"t":"pointerDown","at":{"x":1,"y":2},"view":{"zoom":2,"rotation":90}}"##
```

`packages/core/src/types.ts` and `packages/core/wasm-types.d.ts` duplicate the Rust definitions by
hand, and nothing checks the pair. That is left visible on purpose: it is the drift the port plan
says to fix with `ts-rs` or `tsify` before wave 2 gets going, and 300 lines of toy is enough to feel
why.
