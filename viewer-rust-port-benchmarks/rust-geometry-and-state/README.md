# Rust geometry and state

This variant keeps geometry, state updates, and the model in Rust. TypeScript
holds the Rust object, a cached version number, and a table of shape metadata.

| Path | Responsibility |
| --- | --- |
| `crates/core/src/geom.rs` | Geometry calculations |
| `crates/core/src/model.rs` | Messages, effects, state updates, scenes, and hit-testing |
| `crates/core/src/lib.rs` | Wasm exports |
| `packages/core` | TypeScript initialization, subscriptions, and result conversion |
| `packages/react` | React bindings and SVG rendering |

Run from this directory:

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.127
pnpm install
pnpm build
pnpm test
```

Then use `pnpm dev` for the demo on port 5173, or `cargo test` for native Rust
tests, including the shared expected geometry cases.

## Calls to generate a display list

With the metadata cache current, `store.scene(view)` calls Wasm once regardless
of shape count. If a state change marks the cache dirty, it first checks
`tableVersion`. If that version changed, it also reads `shapeTable()`.

| Cache state at the call | Wasm calls |
| --- | --- |
| Current | 1 |
| Needs checking, but metadata unchanged | 2 |
| Needs refreshing, including the initial scene | 3 |

Metadata changes include rotation and anchored status, not just adding or
removing shapes. Calling `rows()` first can refresh the cache before `scene()`.
These counts exclude the earlier dispatch or setup calls. See
[`store.ts`](packages/core/src/store.ts) and [its tests](packages/core/src/store.test.ts).

`sceneRaw()` always makes one call and returns the numeric buffer without
creating display objects. Its layout is a shape count followed by records of
`[handle, eight coordinates, flags]`. The wrapper reads the record size and
flag values from Rust during store creation.

## React integration

The wrapper caches the Rust version number in TypeScript. `getVersion()` reads
that cache without calling Wasm, giving React a stable snapshot between state
changes. The React binding also reuses a display list when store, view, and
version have not changed; see
[`binding.test.tsx`](packages/react/src/binding.test.tsx).

State-change notifications run after Rust's `dispatch` returns. A callback
that reenters the same Rust object during a mutable borrow can fail at runtime.
Deferring notification avoids that path. A host logger called from an immutable
borrow may read the object but must not dispatch a mutation.

## Type definitions

[`types.ts`](packages/core/src/types.ts) and
[`wasm-types.d.ts`](packages/core/wasm-types.d.ts) are maintained by hand alongside
the Rust types. Tests exercise messages and serialization, but there is no
generator that guarantees the TypeScript declarations match every Rust field.
Changing an API requires reviewing both sides and their tests.

One Wasm call is not a performance guarantee. Converting the returned numeric
buffer to JavaScript objects still takes time; the
[historical benchmark tables](../bench/RESULTS.md) compare both forms.
