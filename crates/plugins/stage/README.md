# epdf-plugin-stage

`packages/plugin/stage` in Rust: the viewport lens. Camera, zoom, scroll, spreads, per-document
layout, motion over a host frame seam, responsive settings, PDF destinations, the six change hooks,
and the pan handler for the interaction hub. It follows the v3 contract: `create()` builds the
controller, `connect` registers the navigation executors, pages are addressed by `PageRef`, and the
verbs carry the v3 names. All 190 cases in six vector files replay identically (`cargo test -p
epdf-plugin-stage`):

| File | Kind | Cases | Covers |
|---|---|---|---|
| `vectors/plugins/stage-destination.json` | function | 29 | `destinationToReveal` for every `/XYZ` and `/Fit*` kind, null and absent members, offset crops, rotation, `userUnit` (0 throws, absent is NaN) |
| `vectors/plugins/stage-pure.json` | function | 87 | `settingsEqual`, the registry constants, the motion laws, `boxOf`, `matchesQuery`, `mergeSettings`, `eqSetting`, `resolveResponsive`, `initialStageState`, `stageReducer` |
| `vectors/plugins/stage-capability.json` | scenario | 48 | placement and initial-view providers (withdrawn ones included), every read, the setters by effect class, zoom intents, `zoomTo` with a bare level, `zoomBy`, fit-all, view rotation, wrapped grids, paged flow, spreads, RTL, navigation by index and by `PageRef`, bare and positioned reveal, `revealRect`, `resetSettings`, the scroller, gestures, the double-tap ladder, responsive rules, two lenses, the page-less document |
| `vectors/plugins/stage-motion.json` | scenario | 15 | the rest detector, the navigation and zoom-anchored tweens, caught tweens, fling glide and bounce, the elastic release spring, all driven by `advance` |
| `vectors/plugins/stage-feeds.json` | scenario | 8 | the page-registry refit, the `goto` and `named` executors through the real actions plugin, the page-state feed (a cursor past the last page included), and the pan handler through the real interaction hub |
| `vectors/plugins/stage-events.json` | scenario | 3 | the six change hooks behind the diffing dispatch, `onMotionEnded` for tweens, flings and `stopMotion`, and the camera hook's object identity on an unbounded landing |

`tests/api.rs` covers what the vectors can't: lens identity (`same`), frames that fire after their
document closed, and the `RuleWhen` and `PdfDestination` helpers.

The generators (`tooling/plugin-conformance/src/suites/stage-pure.ts` and `stage-plugin.ts`) reach
3,536 of the package's 3,542 statements and 774 of its 804 branches, counted over the stage vector
files alone. What they can't reach is listed under Coverage.

## The public API

`stage_plugin(StagePluginOptions)` builds the plugin. `STAGE` is the main lens's token and
`StageCapability` its capability, with the TypeScript's members in snake case (`get_camera`,
`list_visible_pages`, `get_page_frame`, `go_to_page`, `go_to_page_index`, `reveal`, `reveal_index`,
`zoom_to`, `update_settings`, `on_page_changed`, ...). The TypeScript types the same object twice,
as `StageCapability` and the wider `StageHostCapability`; Rust has one type, and the host-only
members (`set_viewport_size`, `begin_gesture`, `fling`, `zoom_around`, `get_scroll_metrics`,
`provide_initial_view`, `refit`, `get_lens_id`, ...) sit on it with the rest. A second lens is
`StagePluginOptions { id, token, .. }`, as `stagePlugin({id, token})` is. `list_visible_pages()`
returns an `Rc<Vec<VisiblePage>>` that stays the same `Rc` until the camera, scene, viewport, ratio
or rest state changes, so a layer that compared it with `Object.is` can use `Rc::ptr_eq`.

`PageInfo` is the kernel's registry row as JSON (`serde_json::Value`), because the kernel keeps the
page registry that way. `get_current_page`, `list_current_item_pages` and the page hook hand those
rows back. `zoom_to` takes a `ZoomTarget`, the TypeScript's `number | ZoomSpec`. The hooks return
the kernel's `Unsubscribe`, and so does `provide_initial_view`.

The `/destination` entry is `destination_to_reveal(&PdfDestination, &PageLayout, Option<Rect>)`,
returning a `DestinationReveal { page_index, options }` for `reveal_index`. It returns
`Err(NonInvertible)` where the TypeScript throws, for a layout whose `userUnit` is 0. The contract
types link and search need are exported from the crate root: `RevealOptions`, `RevealAnchor`,
`RevealZoom`, `ScrollBehaviorKind`, `StageState`, `StageAction`, `StageSettings`,
`PartialSettings`, `StageViewState`, `Viewpoint`, `VisiblePage`. `create_scroll_handler(stage,
hub, options)` returns the `InteractionHandler` the surface binding registers, scoped to
`stage.get_lens_id()`.

Where the TypeScript throws, the verb returns `Err`. The only throw the capability can reach is a
`TypeError` (`Cannot read properties of undefined (reading 'x')`) from every verb that clamps
against the paged slice of a document with no pages: `set_camera`, `pan_by`, `zoom_around`,
`zoom_in`, `zoom_out`, `zoom_by`, `fling`, `end_gesture` after an elastic gesture, and
`update_settings` plus every setter built on it when the change re-clamps. The cost is a `Result`
on those methods for one degenerate document. The page-less case in `stage-capability.json` pins
seven of those throws.

## Host inputs

`services/scheduler.ts` reads two globals: `globalThis.requestAnimationFrame` and
`globalThis.cancelAnimationFrame`. They are the `Scheduler` trait here (`raf(callback) -> handle`,
`caf(handle)`), passed as `StageConfig::scheduler`. Rust has no ambient frame clock, so without one
the lens is frameless, which is what the TypeScript is under Node: navigation jumps, `fling` only
snaps into bounds, and the rest detector never runs. A handle is never 0; the capability reads 0 as
"nothing scheduled".

The viewport size and device pixel ratio were already inputs in the TypeScript (`setViewportSize`,
`setDevicePixelRatio`) and stay that way.

## Harness shims

`src/harness.rs` mirrors four shims in `stage-plugin.ts`. A config with `frames: true` gets
`FrameScheduler`: 16 ms timers on the runtime clock, stamped with `rt::now()`, handles from 1, the
same as the generator's scheduler over `@sinonjs/fake-timers`. `lens: name` registers the plugin
under that id with a token of that name. A responsive rule's `when` may be a `$fn` spec, a scripted
predicate that logs the box it is asked about. The `stage-scroll` factory is the surface binding's
half of interaction: a document plugin that registers the scroll handler with the hub, as the React
and Angular stages do. In `stage-pure.ts`, `resolveResponsive` rules may carry `when: {"$returns":
v}` for the predicate `() => v`, and `stageReducer` reports `{state, same}` so the vectors pin the
reference identity the store notifies on (`None` in Rust).

A `VisiblePage` prints its transform's six methods as `$ref`s, registered in member order, so the
ref numbering matches the TypeScript's. The cases were authored against the v2 names, and
`stageStep` in `stage-plugin.ts` emits them against the v3 surface (`goToPage` with an index becomes
`goToPageIndex`, a per-setting read becomes `getSettings`, `pages` reads the kernel's
`documents.listPages`), so the vectors only ever call v3 members. A registry row is printed with its
`undefined` members dropped, as JavaScript reads it.

## Where the port differs in shape

The TypeScript controller is a composition root over areas (`services/`, `read/`, `camera/`,
`navigation/`, `settings/settings.ts`, `view/`), each a closure that returns an `api` slice for
`composeApi`. The areas call each other in a cycle: settings changes re-place through arrivals,
arrivals read the scene, the view lifecycle drives both. Here each area is an `impl
StageCapability` block in a file of the same name over one shared `Inner`, so the cycle needs no
late binding. `settings/settings.ts` is `settings/area.rs`, because `settings::settings` would trip
clippy's `module_inception`.

`services/events.ts` wraps `ctx.dispatch` so every write compares the state before and after and
fires the hooks. `StageCapability::dispatch` does the same. The camera hook fires on
`before.camera !== after.camera`, an object comparison, and the port keeps that: a CAMERA write
always carries a new camera except `setCam(cam())` on an unbounded stage, where `clampCamera`
returns its input. `set_cam_current` marks that write, and no camera event fires for it. The
`an-unbounded-landing-keeps-the-camera` case pins it. The viewport hook fires on every VP write for
the same reason.

The settings registry is the `SettingKey` enum in `SETTING_KEYS` order, and `for_each_setting!`
expands one arm per field wherever the TypeScript loops over the keys (merge, diff, effect lookup,
equality). A setting added to `StageSettings` without a row fails to compile, which is the
TypeScript's `Record<keyof StageSettings, ...>` guarantee. `eqSetting`'s one level of own-key
equality is `PartialEq` on the typed values, `NaN` unequal to itself included.

The two watchers in `sync/watchers.ts` are written against `subscribe` rather than
`EffectCtx::watch`, because the page-state feed's selector resolves the capability and throws while
the document closes. The TypeScript reports that throw and leaves the watch's previous value alone,
and so does the port: `navigation-executors` in `stage-feeds.json` records the report on its close
step.

Engine JSON reaches the stage carrying the kernel's `{"$undefined": true}` marker (a page without
`userUnit`, say). `without_undefined` drops those members before the page registry and destinations
deserialize, as a JavaScript read sees them.

A frame scheduled before its document closed still fires. Under v3 the lease answers the last state
after close, so the frame reads it, as the TypeScript's does, and its dispatches are dropped with
one `instance-closed` report. The pre-v3 port returned early instead.

## Not ported

Page indexes are integers. `goToPageIndex` and `revealIndex` take any number in the TypeScript, and
a fractional index lands as a fractional cursor. Here they take `isize` and the cursor is `usize`,
so a restored view with a negative cursor does not decode.

`undefined` and `null` settings are absent. `stagePlugin({padding: undefined})` spreads the
`undefined` over the default and the state loses its padding; the typed config keeps the default.
`mergeSettings(s, {zoom: null})` lands a null zoom, and `settingsEqual` and `eqSetting` compare
nulls; none of those values is a setting a viewer can hold, so the cases were dropped from the
suite. For the same reason a zoom spec carrying two keys (`{level, mode}`) decodes as its first,
and `eqSetting` on nested objects is not ported.

`RevealOptions.anchor: null` reads as absent (a bare reveal); the TypeScript counts a present null
as positioned. `GoToOptions.arrivalAlign: {y: undefined}` reads as absent; the TypeScript spreads
the `undefined` and places at NaN. A goto node whose destination has an unknown `kind` fails the
executor here, where the TypeScript's switch falls through to a bare smooth reveal.

## Coverage

Six statements and 30 branches are unreached. Each is unreachable from the public surface or
doesn't port:

- `services/scheduler.ts:18` (and branch `17:43`), the `globalThis.requestAnimationFrame` default.
  Node has none; Rust takes the `Scheduler` explicitly.
- The `ctx.document()` absent arms: `services/scene.ts:34`, `:36`, `:105`, `:180`,
  `read/pages.ts:157`, `:193`, `navigation/arrive.ts:99`, `:180`, `:181`, `view/lifecycle.ts:60`,
  `sync/watchers.ts:31`. The plugin is document-scoped, and the kernel keeps serving the staged meta
  while the document closes.
- `services/scene.ts:167`, the `JSON.stringify` arm of the scene key: every object-valued scene
  setting has its own key function.
- A laid-out box without a registry row, or a row without its box: `read/pages.ts:30`, `:36-38`,
  `:49`, `:162`, `services/scene.ts:218`, `:233`. The scene is built from the same registry and
  grouping, so each target is in its item and each box has its row.
- An empty scene after a non-empty page count: `read/pages.ts:160`, `navigation/reveal.ts:94`,
  `:128`, `:162`, `:171`, `navigation/zoom.ts:65`.
- `sync/watchers.ts:64`, a null snapshot in the feed handler: `placed` is a one-way latch.
- `stage.plugin.ts:87` and `:102`, the executors' node-type guards, and `:104`, a missing stage in
  the named executor. The dispatcher calls an executor only for its own type, and the stage lives as
  long as its document.
- `model.ts:124-125`, the reducer's `default` arm. `StageAction` is an enum here.

Two TypeScript behaviours are pinned that look unintended. `applyViewState` writes the view's
cursor without clamping it, so a view saved on a longer document leaves the cursor past the last
page: `getCurrentPage` answers null, the page hook reports `page: null`, and the feed reports a null
current page (`a-cursor-past-the-last-page`, `change-hooks`). And the camera hook's object identity
means a bounded re-clamp that moves nothing still fires a camera event, while the same re-clamp on an
unbounded stage fires none.

## Mutation testing

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
