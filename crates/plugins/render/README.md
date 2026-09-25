# epdf-plugin-render

`packages/plugin/render` in Rust, plus `engine-core`'s render-policy snaps (`snapFullPageViewport`,
`snapAppearanceScale`, `snapTileScale`, `CONTINUOUS_RENDER_POLICY`), which the plugin's arithmetic
rests on. All 1,653 cases in six vector files replay identically (`cargo test -p
epdf-plugin-render`):

| File | Kind | Cases | Covers |
|---|---|---|---|
| `vectors/plugins/render-tiles.json` | function | 914 | `tiles.ts`: grids at 11 scales × 3 tile sizes × 6 page shapes, want sets, paint and engine rects, bleed, coverage, the prefetch ring |
| `vectors/plugins/render-strategy.json` | function | 457 | `resolveRenderOptions`, `resolveStrategy` over 6 policies × 20 option sets, `baseAskWidth` |
| `vectors/plugins/render-policy.json` | function | 180 | the three snaps, 31 of them throwing `InvalidArg` |
| `vectors/plugins/render-ledger.json` | function | 33 | `renderReducer` and `annotatedPons` |
| `vectors/plugins/render-capability.json` | scenario | 30 | keys, conformance, the raster store (singleflight, refcounted aborts, LRU by cost), epochs, events, permission refusals, close |
| `vectors/plugins/render-tile-plane.json` | scenario | 39 | the tile manager behind `tilesFor`: engagement, levels, retention, settle timers, backpressure, views |

`tests/api.rs` covers the typed surface the vectors can't reach: `ViewTiles` identity, the store and
manager accessors, and handle costs.

## The public API

`render_plugin(options)` and `render_plugin_with_host(options, host)` build the plugin; `RENDER` is
its token and `RenderCapability` its capability, with the TypeScript's methods in snake case
(`render_page`, `render_source_key`, `conform_viewport`, `tiles_for`, ...). `tiles_for(view)`
returns a `ViewTiles` whose `plan` hands back an `Rc<TilePaintPlan>`: the same `Rc` until something
changes, so a layer that compared plans with `Object.is` compares them with `Rc::ptr_eq`. The pure
planning API is public too, for `plugin-stage` and hosts: `tiles`, `strategy`, `paint_plan`,
`policy`, `RasterStore` and `TileManager` (with `TileManagerDeps` to run one outside the capability).

Where the TypeScript throws synchronously, the Rust returns `Err` rather than a rejected promise:
`render_page` for a page the registry doesn't know (`render: unknown page object number 99`) or one
the engine refuses, and `ViewTiles::plan` for the same refusal. A refusal the plugin makes itself
(`PermissionDenied` with context `render.renderPage` or `render.tile`) still rejects, as it does in
the TypeScript.

## The host seam

The only browser objects the package touches are `requestAnimationFrame` and the console. Both come
in through `RenderHost`. Its default is what the TypeScript does under Node: `setTimeout(cb, 16)` on
the kernel's clock for frames, and stderr for `console.warn` and `console.debug`. The raster store
never touches an `ImageBitmap` or object URL. It costs a handle from its `source`, `width` and
`height` and returns the engine's JSON as `PageImageHandle`; `objectUrl()` is the engine's business.

## Harness shims

Three things in `tooling/plugin-conformance/src/suites/render.ts` exist only for the vectors, and
`src/harness.rs` mirrors each. A `renderPage` option `signal: {"$abortSignal": name}` is the signal
of a controller the `signals` test plugin aborts. The plugin's warnings and debug lines log as
callbacks named `console.warn` and `console.debug`, so their text and their one-shot flags are
compared. Every `page.render.image` call defers under its own label, `r0`, `r1`, ..., so a step
settles exactly the call it means.

## Where the port differs in shape

`localeCompare` orders a paint list's keys under Node's ICU root collation, which puts `|` before
digits: `3,1|a1` sorts before `3,10|a1`, the opposite of byte order. `js::locale_compare` carries
that table for printable ASCII, the only characters a tile key holds. `js::to_fixed` rounds ties
away from zero as `Number#toFixed` does (`0.125` to `0.13`); Rust's `{:.2}` rounds them to even.

The strategy memos compare the policy by value, where the TypeScript compares by reference. A
document's policy is fixed at open, so the two recompute at the same moments, and the one-time format
warning is guarded by its own flag either way.

After a document closes, the TypeScript tile manager keeps running: an armed settle timer still
fires and fetches through the retained handle, and a late answer still wakes the layers. The port
keeps that, and the vectors pin it. A plan after close reads the removed slice and throws `TypeError:
Cannot read properties of undefined (reading 'contentEpochs')`; `render_epoch` and `plan` return
that error.

Not ported: inputs outside the TypeScript's types, which crash it. A handle that isn't an object, an
event missing the member its type names, an invalidate scope other than `content` or `annotations`,
and a render policy this port can't parse (it reads as `continuous`).

## Coverage

The generators reach 2,003 of 2,013 statements and 421 of 438 branches of the package. None of the
rest is reachable from the public surface:

- `capability.ts:38`, `:154`, `:180` and `effects.ts:66`, the `ctx.doc` null arms. The plugin is
  document-scoped, so its context always carries a handle.
- `capability.ts:48`, `:80`, `:239` and `effects.ts:67`, the `ctx.document()` null arms. A
  document-scoped context falls back to the staged meta, which outlives close.
- `capability.ts:116`, a non-width viewport in `baseKey`. `conformViewport` only returns width.
- `capability.ts:126-127`, `requestAnimationFrame`. DOM-only; this is `RenderHost::request_frame`.
- `capability.ts:166` and `:196`, a store signal already aborted when the fetch starts. The store
  hands every fetch a fresh controller.
- `raster-store.ts:82-88`, the `size` and `costUsed` getters, used only by the package's own tests.
  Ported and covered by `tests/api.rs`.
- `raster-store.ts:106`, an abort after settlement. The listener is removed when the fetch settles.
- `raster-store.ts:178`, an aborted signal without a reason. `abort()` always sets one.
- `tile-manager.ts:524`, a resolution for a superseded entry. Every path that removes an unresolved
  entry aborts its consumer first, so it rejects.
- `tile-manager.ts:573`, `wantWidth` null with `wantScale` set. They are set together.
- `tile-manager.ts:586`, no `lastVisible` in the release rule. It's set before any level is.
- `tile-manager.ts:606-610`, `stats()`, test-only. Ported and covered by `tests/api.rs`.

The engine-core snaps live outside `coverage.json`'s ported set (`packages/engine/core`), so they
have no entry there.

## Mutation testing

`cargo mutants -p epdf-plugin-render --test-package epdf-plugin-render` tests 575 mutants: 500
caught, 6 timeouts, 54 unviable, 15 missed. Every miss is equivalent:

- `capability.rs:78` (`==` to `!=` on the policy-parse memo) and the three guard and `==` mutants
  each at `capability.rs:91` and `tile_manager.rs:174` (the strategy memos). The policy doesn't
  change within a document, so recomputing or never recomputing gives the same strategy, and the
  format warning has its own once-flag.
- `tile_manager.rs:353` and `:678`, `bleed > 0` to `>= 0`. Bleeding by zero points returns the same
  rect, because a tile's paint rect is already clamped to the page.
- `tile_manager.rs:464`, the fresh-state epoch sentinel `-1` to `1`. A first plan at epoch 1 skips a
  reset that would have cleared an empty state.
- `tile_manager.rs:610`, both `&&` to `||` in the settle-cancel test. The mutated condition differs
  only when no timer is armed, where cancelling clears `pending_level`, which only an armed timer
  reads.
- `tile_manager.rs:851`, `same_value_zero`'s NaN arm. A NaN level never reaches the paint list: its
  rects are NaN and fail the visibility test.
- `js.rs:68`, `+` to `*` in the non-ASCII collation rank. Both keep non-ASCII after ASCII and
  ordered by code point.
- `reducer.rs:8`, `initial_render_state()` to `Default::default()`, the same value.
