# epdf-plugin-stage

`packages/plugin/stage` in Rust: the viewport lens. Camera, zoom, scroll, spreads, per-document
layout, motion on the kernel's host clock, responsive settings, page-space destinations, the six
events, and the pan handler for the interaction hub. It follows the current TypeScript contract
(upstream `b456ee4fd` plus the Task 12 migration): `create()` builds the controller, `connect` wires
the refit, the working set, the page-state feed and the navigation executors, and state changes go
through pure transitions (`model.rs`). All 198 cases in six vector files replay with no member-order
allowance and no outcome exception (`cargo test -p epdf-plugin-stage`):

| File | Kind | Cases | Covers |
|---|---|---|---|
| `vectors/plugins/stage-destination.json` | function | 29 | `revealOfDestination` for every page-space `xyz` and `fit*` kind, null and absent members |
| `vectors/plugins/stage-pure.json` | function | 87 | `settingsEqual`, the registry constants, the motion laws, `boxOf`, `matchesQuery`, `mergeSettings`, `eqSetting`, `resolveResponsive`, `initialStageState`, the named transitions with `{state, same}` |
| `vectors/plugins/stage-capability.json` | scenario | 54 | placement and initial-view providers, every read, the setters by effect class, zoom intents, fit-all, view rotation, wrapped grids, paged flow, spreads, RTL, navigation by index and by `PageRef`, bare and positioned reveal, the scroller, gestures, the double-tap ladder, responsive rules, two lenses, the page-less document |
| `vectors/plugins/stage-motion.json` | scenario | 15 | the rest detector, the navigation and zoom-anchored tweens, caught tweens, fling glide and bounce, the elastic release spring, on the scenario clock's 16 ms frames |
| `vectors/plugins/stage-feeds.json` | scenario | 10 | the page-registry refit, the `goto` and `named` executors through the real actions plugin, the page-state feed, and the pan handler through the real interaction hub |
| `vectors/plugins/stage-events.json` | scenario | 3 | the six events, `onMotionEnded` for tweens, flings and `stopMotion`, and the camera event's object identity on an unbounded landing |

`tests/api.rs` covers what the vectors can't: lens identity, motion on frames and its cancellation
when the document closes, `stop_motion`, the current destination and a destination asked for before
placement, the order of a viewport report's events, the paged-empty camera refusal, M27 (the empty
scene for a NaN column count, and `Err` rather than a panic for non-finite camera input on a grid),
R5, a non-invertible page transform, page arguments, the working set and its engine calls, and what
a close releases.

The statement and branch coverage figures that stood here were measured against the pre-catch-up
package and are stale; Task 14 re-measures them.

## The public API

`stage_plugin(StagePluginOptions)` builds the plugin. `STAGE` is the main lens's token and
`StageCapability` its capability, with the TypeScript's members in snake case (`get_camera`,
`list_visible_pages`, `get_page_frame`, `go_to_page`, `go_to_destination`, `reveal`,
`get_current_destination`, `zoom_to`, `update_settings`, `on_page_changed`, ...). The TypeScript
types the same object twice, as `StageCapability` and the wider `StageHostCapability`; Rust has one
type, and the host-only members (`set_viewport_size`, `begin_gesture`, `fling`, `zoom_around`,
`get_scroll_metrics`, `provide_initial_view`, `refit`, `get_lens_id`, ...) sit on it with the rest.
A second lens is `StagePluginOptions { id, token, .. }`, as `stagePlugin({id, token})` is.
`list_visible_pages()` returns an `Rc<Vec<VisiblePage>>` that stays the same `Rc` until a memo input
moves, so a layer that compared it with `Object.is` can use `Rc::ptr_eq`.

A page argument is `impl Into<PageTarget>`: a `PageRef`, or a `usize` display index, the
TypeScript's `PageRef | number`. `PageTarget::Index` takes any `f64`, and only a whole one names a
page. `get_current_page` and the page event answer a `PageRef`; `list_current_item_pages` answers
the kernel's registry rows as JSON (`PageInfo`). `PageDestination` is the core page-space
destination, declared here because the kernel crate has none and `epdf-engine`'s would pull pdfium
into `epdf-web`. `reveal_of_destination(&PageDestination, Size)` is `destination.ts`.
`stage_state(Option<&StageCapability>)` is `stageState`. `create_scroll_handler(stage, hub,
options)` returns the `InteractionHandler` the surface binding registers, scoped to
`stage.get_lens_id()`.

Where the TypeScript throws, the verb returns `Err`. The paged slice of a document with no pages has
no item, and every verb that clamps against it (`set_camera`, `pan_by`, `zoom_around`, `zoom_in`,
`zoom_out`, `zoom_by`, `fling`, `end_gesture` after an elastic gesture, and `update_settings` when
the change re-clamps) answers the TypeScript's `TypeError` (`Cannot read properties of undefined
(reading 'x')`) after the same earlier writes. The TypeScript reaches it by dereferencing
`undefined` (`services/scene.ts` `itemRect`); Rust checks for the missing item in `stay_bounds`, so
the error is the same and nothing panics. `a-document-with-no-pages` pins seven of those throws.

## Host inputs

Time comes from the kernel's host clock (`ctx.clock`, `Kernel::new_with_clock`). A clock with frames
gives tweens, flings and the rest detector; without frames, as under Node, navigation jumps, `fling`
only snaps into bounds, and pages always snap. The instance clock cancels every frame and timer when
the document closes, so no motion outlives its document. The viewport size and device pixel ratio
are inputs (`set_viewport_size`, `set_device_pixel_ratio`).

## Harness shims

`src/harness.rs` mirrors the shims in `stage-plugin.ts` and `stage-pure.ts`. A case's `clock:
{frames: true}` builds the kernel over the scenario clock's 16 ms frames; the stage itself has no
frame option. `lens: name` registers the plugin under that id with a token of that name. A responsive
rule's `when` may be a `$fn` spec, a scripted predicate that logs the box it is asked about. The
`stage-scroll` factory registers the scroll handler with the hub at connect, as the React and
Angular stages do. In `stage-pure.ts`, `resolveResponsive` rules may carry `when: {"$returns": v}`
for the predicate `() => v`, and the named transitions report `{state, same}`, `same` being the
`Rc` identity the store notifies on.

A `VisiblePage` prints its transform's six methods (`toPixels` to `viewToPageRect`) as `$ref`s,
registered in member order, then `cssMatrix`. A restored view decodes as a `StageViewPatch`, because
the vectors' views leave out the settings added since they were written, and `mergeSettings` skips
an absent key.

## Where the port differs in shape

The TypeScript controller is a composition root over areas (`services/`, `read/`, `camera/`,
`navigation/`, `settings/settings.ts`, `view/`), each a closure that returns an `api` slice for
`composeApi`. The areas call each other in a cycle: settings changes re-place through arrivals,
arrivals read the scene, the view lifecycle drives both. Here each area is an `impl
StageCapability` block in a file of the same name over one shared `Inner`, so the cycle needs no
late binding. `settings/settings.ts` is `settings/area.rs`, because `settings::settings` would trip
clippy's `module_inception`.

`services/events.ts` derives five events in one state-change listener, and so does
`services/events.rs`. The TypeScript compares two state members by object identity
(`previous.camera !== next.camera`, `previous.viewport !== next.viewport`) and prints each camera in
the member order of the literal that built it (`{x, y, zoom}` or stage-core's `{zoom, x, y}`). The
state therefore carries a hidden camera identity and member order (`CameraObject`), and counts
viewport writes. `writeCamera(camera())` on an unbounded stage stores the same object, so no camera
event fires; `an-unbounded-landing-keeps-the-camera` pins it.

`patchSettings` lands a key when it is not `Object.is` the current value. A primitive setting
compares by value; an object setting (a page frame, an alignment, a zoom spec, a `{px}` gap) counts
as new, because every patch the stage writes carries a new object. The TypeScript can pass back the
very object the state holds (a `getViewpoint()` zoom handed to `goToPage`), and then writes nothing;
Rust writes an equal value and records a state change with no settings event. No vector reaches it.

The settings registry is the `SettingKey` enum in `SETTING_KEYS` order, and `for_each_setting!`
expands one arm per field wherever the TypeScript loops over the keys (merge, diff, effect lookup,
equality). A setting added to `StageSettings` without a row fails to compile, which is the
TypeScript's `Record<keyof StageSettings, ...>` guarantee. `eqSetting`'s one level of own-key
equality is `PartialEq` on the typed values, `NaN` unequal to itself included.

`connect.ts`'s `ctx.watch` calls are the kernel's `ControllerContext::watch`.
The working-set watch (`working-set.ts`) runs as in the TypeScript, because reading the visible pages
on every store change builds the scene when it does there. It tells the engine through
`DocumentHandle::set_working_set(view, pages)`, the pages as JSON, once per change of the set's key.
The kernel's default is a no-op, and so is the conformance fake's, as the TypeScript fake's is
(`fake-engine.ts:625`); the native and web handles forward it to the engine (Task 13.33).
`the_working_set_is_told_at_connect_and_on_each_change` pins the calls with a recording handle.

A page transform whose view scale squares to zero (a huge device ratio over a tiny unbounded zoom)
is not invertible. The TypeScript throws `Mat2D.invert: non-invertible matrix` from the page reads;
no Rust page read returns a `Result`, so the page is left out of `list_visible_pages` and
`get_page_frame` answers `None` (`a_non_invertible_page_transform_leaves_the_page_out`).

Engine JSON reaches the stage carrying the kernel's `{"$undefined": true}` marker (a page without
`userUnit`, say). `without_undefined` drops those members before the page registry and destinations
deserialize, as a JavaScript read sees them.

A pending navigation (`whenPlaced`) and the providers and rule predicates hold the capability; the
instance's cleanup lets go of them at close, which a garbage collector does for the TypeScript.

## Not ported

The cursor is a `usize`, so a restored view with a negative or fractional cursor does not decode.

`undefined` and `null` settings are absent. `stagePlugin({padding: undefined})` spreads the
`undefined` over the default and the state loses its padding; the typed config keeps the default.
`mergeSettings(s, {zoom: null})` lands a null zoom, and `settingsEqual` and `eqSetting` compare
nulls; none of those values is a setting a viewer can hold, so the cases were dropped from the
suite. For the same reason a zoom spec carrying two keys (`{level, mode}`) decodes as its first,
and `eqSetting` on nested objects is not ported.

`RevealOptions.anchor: null` reads as absent (a bare reveal); the TypeScript counts a present null
as positioned. `GoToOptions.arrivalAlign: {y: undefined}` reads as absent; the TypeScript spreads
the `undefined` and places at NaN. A goto node whose destination does not decode as a
`PageDestination` gets a bare reveal of its page, as the TypeScript's switch falls through to one.

## Coverage

The unreached-line list that stood here named pre-catch-up files (`services/scheduler.ts`,
`sync/watchers.ts`, the reducer) and is stale; Task 14 re-measures it with the mutation counts.

Two TypeScript behaviours are pinned that look unintended. `applyViewState` writes the view's
cursor without clamping it, so a view saved on a longer document leaves the cursor past the last
page: `getCurrentPage` answers null, the page hook reports `page: null`, and the feed reports a null
current page (`a-cursor-past-the-last-page`, `change-hooks`). And the camera hook's object identity
means a bounded re-clamp that moves nothing still fires a camera event, while the same re-clamp on an
unbounded stage fires none.

## Mutation testing

Stale: these counts and line numbers are from the pre-catch-up port. Task 14 reruns
`scripts/mutants.sh` over the re-ported modules.

`scripts/mutants.sh -p epdf-plugin-stage --test-package epdf-plugin-stage --file <file>`, file by
file over every module but `harness.rs`, catches 874 of 899 viable mutants (870 caught, 4
timeouts; 84 unviable). Seven of those are killed by new cases, each checked by applying the mutant
by hand:

- `a-rule-moves-a-quantized-setting`: a responsive rule moves a px gap from 1 to 1.4 at zoom 1.
  Both key as 1, so only the cache invalidation in `sync_responsive` rebuilds the scene. It kills
  `||` to `&&` and the first `==` to `!=` at `settings/area.rs:80`.
- `double-tap-rungs-exactly-1.1-apart`: fit-width is 1100/1000 over an automatic of 1, so the
  ladder dedupe meets exact equality (`navigation/zoom.rs:41`).
- `double-tap-at-the-rung-edges`: a zoom of exactly the rung times 1.1, then the rung over 1.1,
  kills both boundary mutants of `near` (`navigation/zoom.rs:45`).
- `pan-handler-gesture-id` listens to `onGestureStarted`: both replacements of the scroll
  handler's `id` (`scroll_handler.rs:37`).

The 25 survivors:

`src/camera/animation.rs`, 3:

- `:102` and `:153`, `item_count > 0` to `>= 0` in the zoom-anchored tween. With an empty scene
  the mutant would panic in `set_cam_anchor_at_screen`, but neither line meets one. The only
  caller, `double_tap_zoom`, returns early on an empty scene, and a registry change refits, which
  cancels the tween before a frame can run.
- `:248`, `> 1e-6` to `>= 1e-6` in the spring's clamp check: exact equality of a float difference
  with `1e-6`.

`src/camera/gestures.rs`, 4:

- `:19` and `:95:43`, `gesture_depth > 0` to `>= 0`. `gesture_elastic` is only true while a
  gesture is open (`end_gesture` clears it), so the depth test never decides.
- `:95:47`, `&&` to `||`. A non-elastic gesture then writes `gesture_raw`, but only an elastic pan
  reads it, and the next `begin_gesture` resets it.
- `:91`, `after.item_count > 0` to `>= 0`. A focal point exists only if the scene before the zoom
  had items, and a zoom cannot empty the scene.

`src/controller.rs`, 3:

- `serde_str` replaced with `String::new()` or `"xyzzy"`, and its `Ok(String(s))` arm deleted.
  The scene key then stops telling layouts, spreads, sizings and directions apart. Every write of
  those settings also invalidates the scene cache (`settings/area.rs:38`, `:81` and `:171`), so
  the key is a second guard. The visible-pages signature also carries the flow, but a flow change
  already flips the scene key between `paged|` and `cont|`.

`src/navigation/arrive.rs`, 2:

- `:190`, `page_count - 1` to `+ 1` or `/ 1` in `go_to_last_page`: `go_to_target` clamps both to
  the last page.

`src/navigation/reveal.rs`, 2:

- `:41`, `>` to `>=` and `<` to `<=` in the nearest reveal. At `rect_extent == hi - lo` the other
  branch computes `rect_pos - p / zoom` by another route, and at `rect_pos == lo` a rect that fits
  already returned. Telling them apart would need a world rect bit-equal to a float difference
  whose two routes round apart, which page geometry cannot be steered to.

`src/read/pages.rs`, 6:

- `:25`, `+` to `*` in `ref_for_index`, and `:30` and `:31` (five), the fallback in
  `unrotated_points`. Both run only for a laid-out box whose page is missing from the registry.
  The scene is built from the registry under the same `page_count.revision` key, so the fallbacks
  are unreachable, as their TypeScript lines are.

`src/services/scene.rs`, 4:

- `:115`, `no_frame` always false: an all-zero frame divides to the same zeros, and the frame key
  only changes spelling while it stays constant.
- `:150`, the `!falsy(px)` guard: a `{px}` gap of 0 gives 0 either way, and a NaN one cannot be
  written into a vector, because the kernel prints an action through `serde_json`, which writes
  NaN as `null` where the TypeScript trace has `{"$number": "NaN"}`.
- `:284`, `groups.len() - 1` to `+ 1` or `/ 1` in the paged slice bound: `item_index_of_page`
  never exceeds `len - 1`.

`src/settings/area.rs`, 1:

- `:80:59`, `strongest == Reflow` to `!= Reflow`. The mutant invalidates on every effect but
  reflow. A reflow is a flow change, which already changes the scene key, and the extra
  invalidations only rebuild an equal scene before anything compares one by reference.
