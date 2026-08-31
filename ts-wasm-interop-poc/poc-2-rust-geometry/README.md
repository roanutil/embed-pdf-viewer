# poc-2-rust-geometry

poc-1 with one change: `@poc/geometry` is a Rust crate compiled to wasm. The brain, the store, the
React adapter and the demo are untouched.

This is the port the plan in `docs/research/rust-core-port.md` argues against, built so the argument
has numbers behind it.

```
crates/geometry/src/core.rs   the math, no wasm-bindgen, so cargo test runs it natively
crates/geometry/src/lib.rs    the wasm surface: a flat f64 API and a serde object API side by side
packages/geometry             the TS wrapper: same signatures, plus initGeometry()
```

Run it:

```
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.127
pnpm install
pnpm build          # cargo -> wasm-bindgen -> packages/geometry/wasm, then the demo
pnpm dev            # http://localhost:5172
pnpm test           # 87 TypeScript tests
cargo test          # 3 Rust tests, against ../vectors/anchor-vectors.json
```

## What it costs

Seven crossings per anchored shape per frame, asserted exactly in
`packages/shapes/src/shapes.test.ts`:

> rectQuad, `anchorFactors` calling `normalizeDeg`, `quadBounds`, `quadScaleAbout`, `normalizeDeg(-r)`,
> `quadRotateAbout`, and `anchorFactors` again for the `projected` flag.

Two of those seven are `normalizeDeg`, which is three arithmetic operations. An unanchored shape
costs one crossing, because `anchorFactors` short-circuits on `!anchored` before it reaches wasm.

Measured cost of a single crossing, from the demo's `measure` button (20,000 calls each):

* `1.015 µs` through a flat `Float64Array`.
* `2.735 µs` through `serde-wasm-bindgen` objects, so the nicer signature costs about 2.7x.
* `0.225 µs` for a Rust to TypeScript callback.

`crates/geometry/src/lib.rs` keeps both surfaces so this stays measurable rather than assumed.

## The good surprise

The identity short-circuit survives. `projectQuad` still returns the caller's own object when the
projection is the identity, because `anchorFactors` decides in TypeScript and the identity path
never calls into wasm. The plan expected to lose this. What you actually lose is the decision being
free: `anchorFactors` calls `normalizeDeg`, so deciding "nothing to do" now costs a crossing.

Inside geometry itself the short-circuit is gone for good, and
`packages/geometry/src/geometry.test.ts` asserts that directly:

```ts
const same = quadScaleAbout(q, ORIGIN, 1);
expect(same).not.toBe(q);   // in poc-1 this was toBe(q)
expect(same).toEqual(q);
```

## The taxes nobody asks for

Async init spreads upward. `@poc/geometry` was synchronous with `sideEffects: false`; now nothing
works until `initGeometry()` resolves, and that reaches `@poc/shapes`, `@poc/store`, the React tree,
and every test file. `apps/demo/src/App.tsx` grew an init gate component that exists for no other
reason.

It also breaks at collection time, not test time. A `const q = rectQuad(...)` in a `describe` body
runs during collection, before `beforeAll`, so it throws before a single test starts and reports as
a confusing suite error. `packages/shapes/src/shapes.test.ts` had to make that fixture lazy.

> Note: `initGeometry` re-registers the host callback on every call, not just the first. Doing it
> only once is silently broken under React `StrictMode`: the first effect run registers a closure,
> the cleanup invalidates it, the second run's callback is dropped because init is already done, and
> the host logger goes dead with no error anywhere.

## Two-way interop

Rust holds a JS function and calls it. `setHostLogger` hands the crate a logger, `greetHost` calls it
from inside wasm, and the demo shows `poc-geometry 0.1.0 ready` under "Rust → TypeScript".
`benchHostCalls(n)` exists so the reverse direction can be measured rather than guessed at.
