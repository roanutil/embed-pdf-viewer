# epdf-plugin-render

`packages/plugin/render` in Rust, plus `engine-core`'s render-policy snaps (`snapFullPageViewport`,
`snapAppearanceScale`, `snapTileScale`, `CONTINUOUS_RENDER_POLICY`), which the plugin's arithmetic
rests on. All 1,450 cases in six vector files replay identically (`cargo test -p
epdf-plugin-render`):

| File | Kind | Cases | Covers |
|---|---|---|---|
| `vectors/plugins/render-tiles.json` | function | 701 | `tiles.ts`: grids at 11 scales × 3 tile sizes × 6 page shapes, want sets, paint rects, bleed, coverage, the prefetch ring |
| `vectors/plugins/render-strategy.json` | function | 457 | `resolveRenderOptions`, `resolveStrategy` over 6 policies × 20 option sets, `baseAskWidth` |
| `vectors/plugins/render-policy.json` | function | 180 | the three snaps, 31 of them throwing `InvalidArg` |
| `vectors/plugins/render-ledger.json` | function | 33 | `reduceRender` and `annotatedPons` |
| `vectors/plugins/render-capability.json` | scenario | 38 | both render doors, keys, conformance, the raster store (singleflight, refcounted aborts, LRU by cost), batches, epochs, the three events, permission refusals, close |
| `vectors/plugins/render-tile-plane.json` | scenario | 41 | the tile manager behind `createViewDemand`: engagement, levels, retention, settle timers, backpressure, views, handle refcounts |

`tests/api.rs` covers the typed surface the vectors can't reach: `ViewDemand` identity, the store and
manager accessors, and handle costs.

`toEngineRect` and `tileEngineRect` left `tiles.ts` in v3, and their 171 cases with them. The tile
manager now hands its owner a page-space rect, and the controller converts it with
`ctx.geometry.forPage(page).pageRectToPdf(rect)`. That conversion is the kernel's
(`PageSpace::page_rect_to_pdf`), and every tile fetch in `render-tile-plane.json` records its
result as the engine call's `target.rect`, so the vectors still pin it. The Rust counterparts and
`PdfRect` are gone.

## The public API

`render_plugin(config)` and `render_plugin_with_host(config, host)` build the plugin; `RENDER` is its
token. The TypeScript has two lenses on one token: `RenderCapability` for applications and
`RenderHostCapability` (from `contract/host`) for view layers. Rust can't widen a token's type, so
`RenderCapability` here carries both, with the TypeScript's methods in snake case. The public ones
are `can_render`, `render_page`, `render_thumbnail`, `render_pages`, `get_render_policy`,
`get_render_epoch`, `invalidate` and `on_invalidated`. The host ones are `render_source`,
`get_source_key`, `conform_viewport`, `get_paint_settings`, `create_view_demand`,
`on_render_completed` and `on_render_failed`.

`create_view_demand(view_id)` returns a `ViewDemand`. `set_demand` schedules and re-plans;
`get_plan` is pure and hands back the same `Rc<TilePaintPlan>` until something changes, so a layer
that compared plans with `Object.is` compares them with `Rc::ptr_eq`. Handles are counted per view
id and `dispose` drops one count. The controller holds each handle weakly, because a handle holds the
controller: drop every clone and the next `create_view_demand` builds a new handle over the same
count and demands. The pure planning API is public too, for `plugin-stage` and hosts: `tiles`,
`strategy`, `paint_plan`, `policy`, `RasterStore` and `TileManager` (with `TileManagerDeps` to run
one outside the controller).

Every render door is an `async` function in v3, so it returns a `Promise` that rejects:
`permission-denied` naming the operation (`render.renderPage requires doc.render`), `not-found` for
a page the document lacks, or the engine's refusal mapped through `toPluginError`. `get_source_key`
and `conform_viewport` stay synchronous and return `Err(not-found)`. `invalidate` and
`ViewDemand::set_demand` return `Result` because re-planning can throw where the engine refuses a
page the registry lists (`plan-throws-when-the-engine-refuses-the-page`).

## The host seam

The only browser objects the package touches are `requestAnimationFrame` and the console. Both come
in through `RenderHost`. Its default is what the TypeScript does under Node: `setTimeout(cb, 16)` on
the kernel's clock for frames, and stderr for `console.warn` and `console.debug`. The raster store
never touches an `ImageBitmap` or object URL. It costs a handle from its `source`, `width` and
`height` and returns the engine's JSON as `PageImageHandle`; `objectUrl()` is the engine's business.

## Harness shims

Three things in `tooling/plugin-conformance/src/suites/render.ts` exist only for the vectors, and
`src/harness.rs` mirrors each. A `renderSource`, `renderPage` or `renderThumbnail` option `signal:
{"$abortSignal": name}` is the signal of a controller the `signals` test plugin aborts.
`renderPages` is not wrapped, so its cases carry no signal. The plugin's warnings and debug
lines log as callbacks named `console.warn` and `console.debug`, so their text and their one-shot
flags are compared. Every `page.render.image` call defers under its own label, `r0`, `r1`, ..., so a step
settles exactly the call it means.

## Where the port differs in shape

`localeCompare` orders a paint list's keys under Node's ICU root collation, which puts `|` before
digits: `3,1|a1` sorts before `3,10|a1`, the opposite of byte order. `js::locale_compare` carries
that table for printable ASCII, the only characters a tile key holds. `js::to_fixed` rounds ties
away from zero as `Number#toFixed` does (`0.125` to `0.13`); Rust's `{:.2}` rounds them to even.

The strategy memos compare the policy by value, where the TypeScript compares by reference. A
document's policy is fixed at open, so the two recompute at the same moments, and the one-time format
warning is guarded by its own flag either way.

`reasonOf` hands the engine an abort reason only when it is a string. A Rust `AbortSignal` carries
an `Error`, never a string, so the port always aborts the engine call with no reason. The vectors
agree: every signal that reaches `reasonOf` in them was aborted with an `AbortError` or none.

## Pinned TypeScript behaviour

A signal that is already aborted still starts an engine fetch. `RasterStore.acquire` runs `fetch`
for a new key before `attach` checks the consumer's signal, so the consumer rejects
`operation-cancelled` at once while the fetch runs on unowned, and a later signal-less ask joins
it. v3 kept this through both doors (`signal-already-aborted`).

Closing a document still does not stop its tile manager. An armed settle timer fires after close
and asks the engine for its tiles through the guarded handle, which aborts each call at once with
the lifetime's `CancelledError` and rejects it `instance-closed`. The `paintAdvanced` that follows
is dropped and reported once by the kernel. What changed in v3 is the plan after close: the ledger
lease answers the last state, so the plan reads it and paints the cached tiles where v2 threw
`TypeError: Cannot read properties of undefined (reading 'contentEpochs')`
(`close-with-tiles-in-flight`). A base render in flight at close rejects `instance-closed` at close
rather than resolving on the late answer (`close-with-a-render-in-flight`).

`renderPages` sorts its results by each page's index in a `Map` built from the input, so a page
listed twice sorts both entries at its last index (`render-pages-batch`).

Not ported: inputs outside the TypeScript's types, which crash it. A handle that isn't an object, an
event missing the member its type names, an invalidate scope other than `content` or `annotations`,
and a render policy this port can't parse (it reads as `continuous`).

## Coverage

The generators reach 2,283 of 2,293 statements and 469 of 483 branches of the package. None of the
rest is reachable from the public surface, or from this port's:

- `controller.ts:54`, a string abort reason. The TypeScript can reach it with a caller's
  `abort('reason')`; a Rust abort reason is an `Error`, so the arm has no Rust counterpart.
- `controller.ts:117` and `:155`, the `ctx.document()` null arms. A document-scoped context falls
  back to the staged meta, which outlives close.
- `controller.ts:206` and `:377`, a store or tile signal already aborted when the fetch starts. The
  store and the tile manager each hand the fetch a fresh controller.
- `controller.ts:248` and `:470`, a non-width viewport in a source key. `conformViewport` only
  returns width.
- `controller.ts:306`, the `?? 0` index fallback in `renderPages`. Every page it sorts came from
  the input.
- `controller.ts:336-337`, `requestAnimationFrame`. DOM-only; this is `RenderHost::request_frame`.
- `raster-store.ts:82-88`, the `size` and `costUsed` getters, used only by the package's own tests.
  Ported and covered by `tests/api.rs`.
- `raster-store.ts:106`, an abort after settlement. The listener is removed when the fetch settles.
- `raster-store.ts:178`, an aborted signal without a reason. `abort()` always sets one.
- `tile-manager.ts:527`, a resolution for a superseded entry. Every path that removes an unresolved
  entry aborts its consumer first, so it rejects.
- `tile-manager.ts:576`, `wantWidth` null with `wantScale` set. They are set together.
- `tile-manager.ts:589`, no `lastVisible` in the release rule. It's set before any level is.
- `tile-manager.ts:611-615`, `stats()`, test-only. Ported and covered by `tests/api.rs`.

The engine-core snaps live outside `coverage.json`'s ported set (`packages/engine/core`), so they
have no entry there.

## Mutation testing

`scripts/mutants.sh -p epdf-plugin-render --test-package epdf-plugin-render --file <file>`, file by
file over every module but `harness.rs`, catches 527 of 543 viable mutants (521 caught, 6
timeouts; 98 unviable). Five of those are killed by new cases, checked by applying each by hand.
Three origins on `form.imported` in `events-reach-listeners`, the falsy `''` and `0` and the truthy
`7`, kill four of the five `origin_of_event` guard mutants. `signal-reaches-a-thumbnail` kills deleting
`signal` from `render_thumbnail`'s options, which needed the harness shim to wrap `renderThumbnail`
on both sides.

The 16 survivors are equivalent or out of a vector's reach.

`src/controller.rs`:

- `:71`, the empty-string guard replaced with `true`. The mutant sends every string origin to the
  system origin. Only a non-empty string tells them apart, and for one the TypeScript's `originOf`
  reads `origin.sub` as `String.prototype.sub`, a function, which a Rust `Value` cannot hold.
- `:174`, `==` to `!=` in the policy memo, and the three guard and `==` mutants at `:187`, the
  strategy memo. The kernel reads the policy once at bring-up and a document-scoped instance never
  sees another, so recomputing gives the same value and never recomputing is never stale. The
  one-shot flag keeps the format warning from repeating.

`src/tile_manager.rs`:

- `:175`, the same three strategy-memo mutants in the tile manager, for the same reason.
- `:353` and `:678`, `bleed > 0` to `>=`. Bleeding a tile's paint rect by zero points returns the
  same rect, because the rect is already clamped to the page; `(x + w) - x` can differ from `w` by
  one rounding, far under the vectors' 1e-12 tolerance.
- `:464`, the fresh-state epoch sentinel `-1` to `1`. A first plan at epoch 1 skips a reset that
  would have cleared an already empty state.
- `:610`, both `&&` to `||` in the settle-cancel test. `abort_all` clears the timer whenever the
  level is cleared, and a timer is only armed when `settleMs > 0`, so a timer never coexists with a
  first engagement or with a level change at this line. The mutated condition therefore differs
  only when no timer is armed, where it clears `pending_level`, which only an armed timer reads.
- `:845`, `same_value_zero`'s `&&` to `||`. Both sides are scales of paint sources. A NaN scale
  makes `tiles_in_rect` yield no coordinates, so no entry and no paint source ever carries one.

`src/js.rs`:

- `:68`, `+` to `*` in the non-ASCII collation rank. Both keep non-ASCII after ASCII and ordered by
  code point, and the plugin's keys are ASCII.

`src/model.rs`:

- `:34`, `initial_render_state()` replaced with `Default::default()`. Its body is
  `RenderState::default()`.
