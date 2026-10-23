# epdf-plugin-render

`packages/plugin/render` in Rust, plus `engine-core`'s render-policy snaps (`snapFullPageViewport`,
`snapAppearanceScale`, `snapTileScale`, `CONTINUOUS_RENDER_POLICY`), which the plugin's arithmetic
rests on. All 1,470 cases in six schema-2 vector files replay identically (`cargo test -p
epdf-plugin-render`), with no member-order allowances and no outcome exceptions:

| File | Kind | Cases | Covers |
|---|---|---|---|
| `vectors/plugins/render-tiles.json` | function | 701 | `tiles.ts`: grids at 11 scales × 3 tile sizes × 6 page shapes, want sets, paint rects, bleed, coverage, the prefetch ring |
| `vectors/plugins/render-strategy.json` | function | 475 | `resolveRenderOptions`, `resolveStrategy` over 6 policies × 22 option sets, `baseAskWidth` |
| `vectors/plugins/render-policy.json` | function | 180 | the three snaps, 31 of them throwing `InvalidArg` |
| `vectors/plugins/render-ledger.json` | function | 32 | `initialRenderState`, `invalidatePages` and `pixelChangeOf` |
| `vectors/plugins/render-capability.json` | scenario | 39 | both render doors, keys, conformance, the raster store (singleflight, refcounted aborts, LRU by cost), batches, epochs, the three events, permission refusals, close |
| `vectors/plugins/render-tile-plane.json` | scenario | 43 | the tile manager behind `createViewDemand`: engagement, levels, retention, settle timers, backpressure, views, handle refcounts |

`tests/api.rs` covers the typed surface the vectors can't reach: `ViewDemand` identity, the store and
manager accessors, handle costs, M25 and R7. `tests/behavior.rs` drives the capability through the
kernel and the fake engine: what each invalidation scope repaints and which facts change nothing, a
tile answer that lands after its tile left the want set, tile work and renders in flight at close,
the settings, the page-layer rights and the appearance pictures. `tests/conformance.rs` also pins the
31 refusals of `render-policy.json` and their four reasons.

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
are the settings (`get_settings`, `update_settings`, `reset_settings`, `on_settings_changed`),
`can_render`, `render_page`, `render_thumbnail`, `render_pages`, `get_render_policy`,
`get_render_epoch`, `invalidate` and `on_invalidated`. The host ones are `render_source`,
`get_source_key`, `conform_viewport`, `get_paint_settings`, `get_layer_rights`,
`create_view_demand`, `render_appearances`, `kept_appearances`, `get_appearance_scale`,
`on_render_completed` and `on_render_failed`. The public doors take a page as `PageArg`, a
`PageRef` or a zero-based index, as the TypeScript's `PageRef | number`.

The config is the plugin's registered settings, merged over `render_defaults()` by the kernel.
`update_settings` changes every document and re-plans every view's pages at once. Every picture
names what it draws (`PageLayerOptions`): an option left out draws what the session may read, and
an explicit `true` the session may not read is refused by the doors and counted as left out by the
reads. The ledger keeps content, annotation and form-field versions per page, and a picture's
epoch is the sum of the parts it draws (`a1f0` in a key draws annotations and no form fields).

`create_view_demand(view_id)` returns a `ViewDemand`. `set_demand` schedules and re-plans;
`get_plan` is pure and hands back the same `Rc<TilePaintPlan>` until something changes, so a layer
that compared plans with `Object.is` compares them with `Rc::ptr_eq`. Handles are counted per view
id and `dispose` drops one count. The controller holds each handle weakly, because a handle holds the
controller: drop every clone and the next `create_view_demand` builds a new handle over the same
count and demands. The pure planning API is public too, for `plugin-stage` and hosts: `tiles`,
`strategy`, `paint_plan`, `policy`, `RasterStore` and `TileManager` (with `TileManagerDeps` to run
one outside the controller).

Every render door is an `async` function, so it returns a `Promise` that rejects: `permission-denied`
naming the operation (`render.renderPage requires doc.render`), `not-found` for a page the document
lacks, or the engine's refusal mapped through `toPluginError`. `get_source_key` and
`conform_viewport` stay synchronous and return `Err(not-found)`. `invalidate` and
`ViewDemand::set_demand` return nothing: the TypeScript's `fetchTile` is `async` now, so an engine
that refuses a listed page fails the tile instead of throwing out of the plan
(`plan-throws-when-the-engine-refuses-the-page`, whose name predates that).

## The host seam

The only browser object the package touches is the console, and it comes in through `RenderHost`
(stderr by default). Frames and the settle timer are the kernel's clock (`ctx.clock`), which the
instance owns, so both are cancelled at close. The raster store
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
that table for printable ASCII, the only characters a tile key holds. `toFixed` is
`epdf_geometry::js::to_fixed`, which takes at most 20 digits; the plugin's two callers pass 1 and 2.

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

Closing a document cancels the tile manager's settle timer and its frame wake-up, because both are
on the instance clock. Tiles in flight reject `instance-closed` through the guarded handle and free
their slots. The plan after close still reads the last ledger and paints the cached tiles, and a
level change there arms a timer the closed clock never runs (`close-with-tiles-in-flight`, and
`closing_cancels_tile_work_the_settle_timer_and_renders_in_flight` in `tests/behavior.rs`). A base
render in flight at close rejects `instance-closed` at close rather than resolving on the late
answer (`close-with-a-render-in-flight`).

`renderPages` sorts its results by each entry's own input index, so a page listed twice keeps both
places (`render-pages-batch`).

Every render asks the engine with `priority: 'high'` and, when the caller names one, the view it is
for, as `ctx.doc.with({priority, view})` does: `DocumentHandle::call_with` with `CallFacts`
(Ruling13.16). The host door passes `RenderSourceOptions.view`, tiles pass their view, and the
public door names none. The facts change queue order, never an answer, so the vectors' fake ignores
them as the TypeScript fake's `with` does; `every_render_carries_high_priority_and_its_view` checks
they reach the handle. Forwarding them in the web and native handles is Task 13.33's.

Not ported: inputs outside the TypeScript's types, which crash it. A handle that isn't an object, an
invalidate scope other than `content`, `annotations` or `fields`, and a render policy this port
can't parse (it reads as `continuous`).

## Rust-only divergences

Each is on a path no vector reaches, and each is registered in
`docs/research/rust-plugins-branch-review.md`, Status.

- M25: a `fetch_tile` that answers `Err` at once (only a Rust embedder of `TileManager` can) fails
  that tile and frees its slot. A synchronous throw in the TypeScript would strand both
  (`a_synchronous_tile_refusal_frees_its_slot`).
- M26: an empty width ladder or tile pyramid degrades to an `undefined` width (a NaN viewport width
  in `conform_viewport`) or a NaN level, where `list[len - 1]` would panic.
- R7: a zero tile span yields no tiles, where the TypeScript's loop runs out of memory
  (`a_zero_tile_span_yields_no_tiles_instead_of_looping`).
- An event missing or mistyping a member `pixelChangeOf` reads names no page for it: an annotation
  or flatten fact with no `page`, a result with no `page`, `widgets`, `results` or
  `meta.changedWidgets` that is not an array, and an `objectNumber` that is not a non-negative
  integer within `u32`. The TypeScript throws a `TypeError` in the event listener or keys the ledger
  with the value as given.
- A settings value this port cannot decode (a member of the wrong type, written past the typed API
  through `Kernel::settings_of`) reads as `render_defaults()` in all three places that read it: the
  resolved options (`Core::resolved`), `get_settings`, and the `on_settings_changed` payload. The
  first such read warns once through `RenderHost::warn`. The TypeScript resolves whatever is there
  (`a_malformed_setting_reads_as_the_defaults_and_warns_once`).

## Migrating from the pre-13.16 API

For annotation (13.17), form (13.18), page-edit (13.19) and viewer-host and epdf-web (13.33).

| Old | New |
|---|---|
| `RenderAction`, `reduce_render`, `define::<RenderState, RenderAction>` | removed. `model::invalidate_pages(state, pons, scope) -> Option<RenderState>`, applied with `update_state` |
| `RenderState.annotated_epochs`, `RenderState.paint_versions` | `annotation_epochs` and `field_epochs`; plan wake-ups are `ctx.notify()`, not a state bump |
| `invalidation::annotated_pons(event, all)` | `pixel_change_of(event, all) -> Option<PixelChange { pages, scope }>`, current event names (`annotations.*`, `forms.*`, `redaction.applied`, `pages.flattened`, `annotations.flattened`, `signatures.completed`, `stream.desynced`) |
| `InvalidateScope::{Content, Annotations}` | adds `Fields` |
| `RenderInvalidatedEvent.origin` as a mapped `ChangeOrigin` | the event's raw `origin`, `Value::Null` for the verb or an event without one |
| `include_annotations: Option<bool>` on `RenderPageOptions`, `RenderThumbnailOptions`, `RenderSourceOptions` | `layers: PageLayerOptions { include_annotations, include_form_fields }`; `RenderSourceOptions` adds `view` |
| `get_render_epoch(page, include_annotations: bool)` | `get_render_epoch(impl Into<PageArg>, PageLayerOptions)` |
| `get_source_key(page, scale, Option<bool>)` | `get_source_key(page, scale, PageLayerOptions)` |
| `render_page`, `render_thumbnail` take `PageRef`; `render_pages` takes `Vec<PageRef>` | `impl Into<PageArg>` and `Vec<PageArg>`; `PageRenderFailure.page` and `RenderBatchResult.skipped` are `PageArg` |
| `invalidate(..) -> Result<(), Error>`, `InvalidateOptions.pages: Option<Vec<PageRef>>` | `invalidate(..)`, `pages: Option<Vec<PageArg>>` |
| `ViewDemand::set_demand(page, demand, Option<bool>) -> Result<(), Error>` | `set_demand(page, demand, PageLayerOptions)` |
| `TileManager::plan(view, pon, demand, bool) -> Result<Rc<TilePaintPlan>, Error>` | `plan(view, pon, demand, PageRenderLayers) -> Rc<TilePaintPlan>` |
| `TileManagerDeps { options, fetch_tile(pon, ..), get_epoch(pon, bool), .. }` | `get_options`, `after` (the clock), `fetch_tile(view, pon, rect, scale, layers, signal)`, `get_epoch(pon, PageRenderLayers)` |
| `RenderHost.request_frame` | removed: frames are `ctx.clock().next_frame` |
| `PaintSettings.fade_ms`, `TilesOptions.fade_ms`, `ResolvedTiles.fade_ms` | removed; `get_paint_settings()` is `Rc<PaintSettings { tiles }>` |
| `TilesSetting` in `paint_plan` | in `contract`, and serializes (`false` or the options) |
| `create_render_controller(cx, &config, host)` | `create_render_controller(cx, host)`; the config is the registered settings |
| tile request `target.rect` as PDF edges via `pageRectToPdf` | page space `{x, y, width, height}`, as `render.image` takes it |
| `DocumentHandle::call` for renders | `DocumentHandle::call_with(&CallFacts { priority: High, view }, ..)`, defaulting to `call` (kernel, Ruling13.16) |
| new | `render_defaults`, `RenderSettingsChanged`, `get_layer_rights`, `render_appearances`, `kept_appearances`, `get_appearance_scale`, `AppearanceStore`, `RecentPictures`, `same_page_view_demand`, `resolve_page_layers`, `layers_key_of`, `render_epoch_of` |
| ffi `RenderPaintSettings.fade_ms` | removed. The ffi doors add an `include_form_fields` argument beside `include_annotations`, and `RenderInvalidateScope` adds `Fields`. `src/ffi.rs` and `tests/ffi.rs` were migrated by hand and are not compiled until `epdf-ffi` builds (13.33) |

Known call sites: `crates/epdf-viewer-host/src/session.rs` builds `RenderHost { release_image,
..RenderHost::default() }`, which still compiles, but its comment about the default 16 ms frame
timer (`session.rs:68-70`) is stale now that frames are the kernel clock's. Its
`render_plugin_with_host(render.into(), host)` keeps its shape and waits on the ffi build (13.33).
`crates/epdf-viewer-host/src/lib.rs` and `api.rs` use the ffi handle and config, `crates/epdf-manual-bridge/src/render.rs` (`start_render_page`), and
`crates/epdf-web/src/adapters.rs` (the harness, whose live `host` drops `requestFrame`).
`crates/plugins/annotation/src/read/render.rs` uses only `policy`, which is unchanged. Appearance
paths are `page.annotations.renderAppearances` and `page.forms.renderAppearances`; a native or web
handle must route them (13.33).

Annotation (13.17): the TypeScript annotation plugin draws its pictures through render
(`packages/plugin/annotation/src/read/render.ts:257` `renderAppearances`, `:271`
`keptAppearances`). Port that onto `RenderCapability::render_appearances`, `kept_appearances` and
`get_appearance_scale` (family `Annotations`), in place of its own
`page.annotations.renderAppearanceImages` call and its local `snap_appearance_scale`.

Form (13.18): the TypeScript form plugin's widget pictures come from render too
(`packages/plugin/form/src/services/pictures.ts:56` `renderer.renderAppearances`). Draw them through
`render_appearances`, `kept_appearances` and `get_appearance_scale` with family `Widgets`, which
needs `doc.forms.read`.

## Coverage

Stale since Task 13.16 (the counts and lines below are the pre-13.16 port's); Task 14 regenerates
them.

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

Stale since Task 13.16: the lines below name the pre-13.16 code. Task 14 reruns it.

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
