# Rust plugins in the web viewer: manual eval pass

This records a manual pass over `examples/rust-kernel-eval`, the viewer chrome running on the Rust
kernel and plugins compiled to WASM, against the TypeScript viewer (`packages/viewer/main` dev
server) on the same PDFs. The pass started at `66927472f` on
`research/multi-platform-rust-core-impl-plugins` and ended at `a4a6f8ed8`. Both dev servers ran
side by side in one Chrome (ports 5250 and 5230), driven through the Chrome DevTools MCP tools: real
clicks, typing and key presses where the tools allow them, and synthetic `PointerEvent`s or direct
capability calls where they do not (see "How gestures were driven"). Almost every row compares the
two viewers' state after the same input, read from the capabilities, not just by eye. Nothing was
timed.

| Feature | Status | Plugin | Cause | Notes or repro |
|---|---|---|---|---|
| Open and close tabs | works | kernel (documents) | | Upload through Open File, switch, close, and close a failed document. Each tab keeps its own scroll and sidebar state. |
| Scroll | works | stage | | Six wheel events of `deltaY: 500` on `ebook.pdf` give the same `scrollTop` sequence (759 to 3259) and the same "Page n of 4" in both. |
| Zoom: buttons, menu, input | works | stage | | `zoom:in`, `zoom:out` and the menu levels give the same zoom and scroll metrics. |
| Fit page, fit width, automatic | works | stage | | Same results once the harness difference is allowed for: see "Viewport height". |
| Spreads, horizontal scroll, view rotation | works | stage | | Two Page, Cover Page, Horizontal and both view rotations give identical page rects and rotations. |
| Thumbnails | works | render, shell | | Clicking a thumbnail scrolls to the page. The Outline tab does nothing in either viewer. |
| Text selection and copy | works | selection | | A drag over the title selects "PDF Viewers Are a" with identical quads; `Meta+C` writes that text to the clipboard. |
| Search | works | search | | "EmbedPDF" finds 8 results with the same excerpts and on-page highlights. |
| Keyboard shortcuts | works | commands | binding | Threw on every keydown before `86ebbb78c`. See "Keyboard shortcuts". |
| Markup, ink, free text, note, caret and replace tools | works | annotation | | The same synthetic gestures create the same annotations with the same bounds in both viewers. |
| Shape, callout and link tools | works | annotation | | Same results from the same `createPointer` sequences, props included. The UI gestures for polygon and callout were not driven successfully in either viewer. |
| Annotation edit and delete | works | annotation | | Moving a note by 60 x 30 px gives the same new bounds (`x` 12.5 to 57.5). Color from the Style panel and Delete from the strip both apply. |
| Stamps | works | stamp, annotation | | Placing "Approved" gives a 164 x 50 pt stamp in both. "Import PDF…" works since `f303fafb5`, which reads a `File` argument to bytes before it crosses; before that it failed on the Rust kernel. |
| Images | works | annotation | | Clicking with the Image tool opens the picker (`accept="image/png,image/jpeg"`); a 120 x 60 PNG becomes a 120 x 60 `stamp`. |
| Form fill | works | form, actions | | `action_form_fixture.pdf`: text, checkbox, radio, dropdown, Hide/Show Field, Reset All and Next Page behave identically. |
| Validate and calculate scripts | works | actions, acrojs | | An invalid email raises the same `alert` text; `calcsum` follows `calc1 + calc2` (1284.5). |
| Format scripts | works | actions, acrojs | | `action_form_fixture.pdf`: typing `1234.5` into Venue (`calc1`) and tabbing out gives `1,234.50` in both viewers, matching the README. See "Form scripts". |
| Document open action | works | actions | | `action_doc_events.pdf` writes "Open" to `eventLog` in both. |
| Links | works | link, actions | | `annots_action_handling.pdf`: GoTo lands on `scrollTop` 1161.33 in both, and the URI link opens a tab. `link_javascript.pdf` raises `alert('embedpdf')`. |
| Redact and apply | works | redaction | | Same mark bounds, same dialog ("2 other annotations will also be removed"), same annotations left afterwards. |
| Measurement | works | measurement, annotation | | Distance, perimeter and polygon from `createPointer` match field for field (distance caption "0.03 m"), timestamps and `nm` aside. The UI gestures were not driven. |
| Page rotate, delete, move | works | page-edit | eval scope | Driven through `PageEditToken`, since the chrome has no page-organize UI (`packages/viewer/chrome/src/config/commands.ts:385`). Layouts and rendered pages match. |
| Signatures without a signer | differs | signature | eval scope | The TS dev harness has a signer and offers "Sign this field"; the eval app has none. A TS viewer started without `signatures` behaves exactly like the Rust one. |
| Locale switch | works | i18n | | `setLocale('es')` relabels the chrome identically, and "Open File" stays untranslated in both. |
| Toolbar overflow at 520 px | works | shell | | Same mode menu, same tool row and "More" overflow. |
| Reports at kernel destroy | differs | stage, epdf-kernel | port | Both viewers log `[kernel] capability("stage") on a destroying kernel`: 16 times on Rust, 17 on TS, from a fresh `ebook.pdf` load. See "Stage reports at destroy". |

## Keyboard shortcuts

`useCommandShortcuts` hands `matchStroke` the raw `KeyboardEvent`
(`packages/framework/react/src/commands.ts:68`). The shim encodes arguments by walking own
properties, and a `KeyboardEvent`'s `key`, `ctrlKey` and the rest are prototype getters, so the
Rust adapter got `{"isTrusted": true}` and threw
`[commands harness] matchStroke: a stroke and options` (`crates/plugins/commands/src/harness.rs:422-431`)
on every keydown outside an editable field. It is a binding bug. `86ebbb78c` adds a `PREPARE`
entry in `packages/core-wasm/src/rehydrate.ts` that reads the `KeyStroke` members off the event
before it is encoded. The regression test in `packages/core-wasm/test/kernel.test.ts` passes an
object whose stroke members are getters. `Meta+=` now zooms to 120% in both viewers.

## Stage reports at destroy

This is not a binding bug, and the Rust port copies it faithfully. The stage's page-state feed
watches a selector that calls `ctx.get(token)` (`packages/plugin/stage/src/sync/watchers.ts:39-47`,
ported at `crates/plugins/stage/src/sync/watchers.rs:78-93`). During `destroy`, closing each session
changes the store while the kernel status is `destroying`, the selector runs, and the kernel guard
throws (`packages/core/main/src/kernel.ts:240`, `crates/epdf-kernel/src/kernel.rs:298-304`). The
store catches each throw per listener and reports it. Fixing it means changing the TS watcher (or
the guard) and the Rust mirror together, and `packages/plugin` is off limits for this pass.

The two viewers differ in the count by one (16 against 17). That points at one fewer store
notification somewhere in the Rust close path. I did not chase it. Task 8 saw five reports; the
count grows with the number of open documents and pages.

The format of the report did differ, and that part was a binding bug. `WasmKernel` printed the
canonical JSON as one string, `[kernel] {"$error":{...}}`, where the TypeScript passes
`console.error` a live `Error`. `a4a6f8ed8` adds `runtime::error_to_js` and reports through it. The
test `a_reported_error_is_a_live_error_with_its_members` is in `crates/epdf-web/tests/runtime.rs`.

## Form scripts

Validate, calculate, hide/show, reset and the document open action all ran through QuickJS and
matched the TypeScript. Format scripts do too. `action_form_fixture.pdf` carries
`AFNumber_Format(2, 0, 0, 0, "", true)` and `AFNumber_Keystroke(2, 0, 0, 0, "", true)` on `calc1`
and `calc2`, and `AFSimple_Calculate("SUM", ["calc1", "calc2"])` on `calcsum`. These sit in
uncompressed objects, not inside the page's Flate/ASCII85 content stream, so an earlier pass that
searched only every fixture's decompressed streams missed them; `strings
packages/plugin/form/test/fixtures/action_form_fixture.pdf | grep AFNumber` finds all four. The
README's claim stands.

Typing `1234.5` into Venue (`calc1`) and pressing Tab gives `1,234.50` in both the Rust eval app
and the TS viewer; typing `250` into Catering (`calc2`) gives `250.00`, and the total (`calcsum`)
reads `1484.5` in both.

The MCP `fill` tool sets a field's value directly and does not itself fire the blur that runs
`AFNumber_Format`: filling Venue with `1234.5` leaves it showing `1234.5` and the total at `0`
until something blurs the field, a real `Tab` press or a click elsewhere. Once that blur fires,
the filled value reformats the same as typed input does. Reading a filled field before any blur,
or never blurring it, is what produces a false "doesn't reformat" result, and is the likely cause
of the earlier negative finding here.

`AFDate_FormatEx("yyyy-mm-dd")` is also on a fixture,
`packages/engine/main/test/fixtures/annot_javascript.pdf`; typing `3/14/2026` there was reported
leaving the value unformatted in both viewers. That finding was not retested here.

## How gestures were driven

The MCP tools click and drag by element, not by page coordinate. Drawing gestures therefore used
synthetic `PointerEvent`s dispatched on the page surface, with the same coordinates in both viewers.
Where those failed (polygon closing, the two-step distance line, callouts, form-field authoring),
they failed identically in both, so I also called `createPointer` on the annotation capability with
the same sequences the tool handlers make (`packages/plugin/annotation/src/tools/handlers.ts:420-470`).
That covers the Rust annotation core but not the Rust interaction path for those gestures. A pass
with real mouse input (Playwright or a person) should repeat the polygon, callout, distance and
form-authoring tools.

## Things that looked like bugs and were not

The TS dev harness styles the tab bar through `::part(tab)` in `packages/viewer/main/index.html`,
which makes its viewport 4 px taller (586 against 582). That alone moves `zoom:fit-page` from
0.50568 (Rust) to 0.50947 (TS) and shifts centered `scrollTop` values by 2.

Clicking or focusing a widget that is off screen makes Chrome scroll the chrome's `overflow: hidden`
viewport natively (`scrollTop` 1070 on Rust, 817.5 on TS in the repro). The stage does not know, and
the pages render offset from where the stage thinks they are. It happens in both viewers and is a
chrome issue, not a kernel one. In this pass the MCP click's scroll-into-view triggered it; keyboard
focus could do the same for a user.

Escape does not close the Menu or View Controls popovers in either viewer. Undo and Redo stay
disabled after edits in both.

The first form interaction in the TS dev server made Vite optimize `@jitl/quickjs-singlefile-browser-release-sync`
and reload the page. The eval app README already notes this kind of reload.

## Performance, as observed

Nothing was measured. Loading `ebook.pdf`, the Rust viewer's first screenshot showed page 1 with
white bands and no annotations where the TS was already complete; a second screenshot a moment
later was complete. Opening an uploaded PDF also looked slower on Rust: a snapshot straight after
the upload showed the new tab on TS but not yet on Rust. Scrolling and zooming felt the same.

## Known gaps

These are known and left open. None of them blocked a row in the table above, and all of them would
need fixing before this build did anything but manual evaluation.

JS plugins are init-only. `createKernel` refuses a JS plugin with `initialState`, `reduce`,
`capability`, `create` or `effects` (`UNSUPPORTED` in `packages/core-wasm/src/kernel.ts`), and the
`ctx` its `init` gets (`runInit`, same file) has `get`, `tryGet`, `forDocument`, `tryForDocument`,
`core` and `cleanup` but no `subscribe`, `watch`, `getState` or `onAction`. The chrome's
`demoToolsPlugin` needs nothing more; a plugin that watches the store does.

Feedback haptics are silent. `feedbackPlugin` in `packages/core-wasm/src/plugins.ts` hands the
adapter only `provider: true`, which selects its logging provider, and a live kernel does not log.
`stampPlugin` passes only `previewWidth` and `dynamic` and drops `assetEngine`, because the stamp
adapter only knows its scripted asset engines. `formPlugin` passes its config through, but the form
adapter ignores it (`_config` in `crates/plugins/form/src/harness.rs:473`).

Cancellation is partial. A `$signal` argument decodes to a fresh `AbortController`'s signal,
aborted only if it already was (`packages/core-wasm/src/proxy.ts`); nothing aborts it later. In the
other direction, `abort()` on a promise from the shim calls `abort_promise`, which reaches
`rt::Promise::abort` (`crates/epdf-kernel/src/rt.rs:459-467`), and that does nothing for a promise
with no abort handle. A promise made with `map` (`rt.rs:476-482`) has none unless the plugin
carries it over with `abort_handle`.

Session memory is bounded for scrolling and rendering, measured at `3d4199d10`.
`packages/core-wasm/test/soak.test.ts` pans `ebook.pdf` 300 times over the Node engine and checks
each counter against its own rule between iterations 50 and 300: `promises`, `aborts`, `described`,
`documents` and `proxies` must not grow at all; `refs`, `wireRefs`, `fns` and `heldFns` may grow by
up to 64 (`LAGGING_SLACK`), since their release can lag a sample; `hosts` and `allHosts` are
bounded, not flat; and `wasmBytes` may grow by up to 2 MiB. Before the release work
(`cc1266800`) the harness's `refs` went from 1271 to 5915 and its `promises` from 190 to 964 over
that span, and the wasm module's linear memory (`wasmBytes`) from 16.3 MB to 66.8 MB at a 96 px
render width, or from 232 MB to 1088 MB at 400 px. Now `refs` stays at 130, `promises` at 0 and
`wasmBytes` at 12,582,912 bytes at both samples. The engine bridge's host table is capped by the
raster store's 128 MiB budget instead: 380 entries at iteration 50, 222 at 300.

The browser agrees. Five round trips from the top of `ebook.pdf` to the bottom and back at 300%
zoom left `wasmBytes` at 9,437,184 bytes before and after, `refs` at 654 then 650, and `promises`
at 0. The JS heap snapshot grew 15.6 MiB in the eval app (78.9 to 94.5 MiB) and 13.3 MiB in the
TypeScript viewer (133.5 to 146.7 MiB). Most of each delta is `ArrayBuffer` data, the encoded tiles
each raster store keeps: 9.5 MiB over 546 more buffers in the eval app, 7.3 MiB over 521 in the
TypeScript. The eval app's host table went from 60 entries to 1152, and three more round trips
added 20, 10 and 10. That is the store filling toward its budget, which this document does not
reach at 300% in either viewer, so nothing evicted in the browser. A heap snapshot is not a
reading of wasm memory, which is why `wasmBytes` is read separately.

These still grow, or can:

- Stamp previews no longer grow. The stamp plugin calls `page.render.image` on the engine itself
  (`crates/plugins/stamp/src/services/asset_engine.rs:210`, `ghosts.rs:39`,
  `write/placement.rs:218`), so those images never enter the raster store. All three go through
  `image_to_preview` (`asset_engine.rs:96`), which now copies the bytes out and then hands the
  render back through `epdf_kernel::bytes::release_host`. The wasm build installs that hook once
  per instance (`set_host_release`, called from `initWasm()`) over the bridge's module-level
  `releaseHost`, which frees both entries a macrotask later, like `bridge.release`. A render whose
  bytes did not resolve is kept, and a native build installs no hook. Before, importing
  `packages/default-stamps/en/stamps.pdf` (17 stamps) added 34 host entries holding 1,634,402
  bytes of BMP over the Node engine, and each further import added the same again. Now the Node
  test in `packages/core-wasm/test/stamp-preview.test.ts` reads `allHosts` at 0 before the import
  and 0 after, where the same test read 34 without the stamp change. In the eval app, an import
  of the same file through the stamp capability left `allHosts` at 59 before and after, with 17
  previews of 10,643 bytes or more. Only the import path (`render_thumbnail`) has a test; the
  ghost and dynamic placement paths release through the same function but are untested.
- The bridge's document table (`documents` in `createEngineBridge`,
  `packages/core-wasm/src/engine-bridge.ts`) keeps every `DocumentHandle` it opened, closed ones
  included, until `dispose()`. That is one small handle per open. A stamp library import opens one
  too (the Node run above goes from 1 to 2 to 3 over two imports), and the eval app reads
  `documents: 2` with one tab.
- `aborts` keeps its entry for a promise that never settles. It read 0 in the soak and in the
  browser.
- Annotation's `BEHAVIORS` thread_local is shared by every kernel in one wasm instance, so a
  destroyed kernel's behaviors, and the harness they hold, stay alive until the next annotation
  build.

A release queued while the kernel is idle waits for the next outermost call into it (the module
doc of `crates/epdf-web/src/exports.rs`), so the callbacks it covers outlive their last use by that
long. `abort_promise` does nothing for a live caller, because no outer promise is wired with an
abort handle (see "Cancellation is partial" above). Releasing an evicted tile has no automated
test: the soak evicts full-page renders, and the browser pass never reaches the budget, so the
tile path was checked only by hand against a build with `DEFAULT_MAX_COST` set to 4 MiB. In the
live build only, a handler that calls its own `off()` twice around `onDown` does not get `owner`
cleared on the second call: `unregister_ref` (`crates/plugins/interaction/src/harness.rs`) drops
the `Unregister` the first time it runs, to free the handler's callbacks once JavaScript is done
with it. `dispatch_pointer` sets `owner` to that handler only after `on_down` returns true, so the
second `off()` arrives too late to find anything to call, and the removed handler stays `owner`
until the next `up` or `cancel`, receiving `move` events for the rest of the gesture. Found by
tracing the code, not reproduced in the browser.

Closing a document drops its session's capabilities only because `epdf-web` sets
`KernelOptions.release_closed_sessions` (`crates/epdf-kernel/src/kernel.rs`). The flag defaults
to `false`. Native callers and the conformance vectors rely on a closed session's deferred work
going quiet through its `Weak` guards, not on the lists being emptied, and the default keeps their
behavior exactly as it was.

Destroy logs 16 `[kernel] capability("stage") on a destroying kernel` reports from a fresh
`ebook.pdf` load, where the TypeScript logs 17. That is a faithful port of a TypeScript bug; see
"Stage reports at destroy".

Test coverage has two holes. The browser hosts have no automated test: the vitest suite runs
under Node with `vi.fn()` stand-ins through `setHosts` (`packages/core-wasm/test/kernel.test.ts`),
so `examples/rust-kernel-eval/src/hosts.ts` over real `requestAnimationFrame` and
`crypto.randomUUID`, and a `Blob` returned from a `callJs` callback (`readBlobs` in
`packages/core-wasm/src/proxy.ts`), were only exercised by hand. And the `LiveSandbox` in
`crates/plugins/actions/src/harness.rs` has no error-path tests: a sandbox member that is not a
function, one that answers a promise, and a `boot` or `run` that throws are all unexercised.

The rehydrated transform's drift check (`withPageTransform` in
`packages/core-wasm/src/rehydrate.ts`) follows the same fail-loudly rule as the release paths
above: it throws rather than hand back a transform that differs from the kernel's, even by the
rebuild's last bit. A real PDF whose page size or scale lands on that last bit would break
`listVisiblePages` outright, and only one fixture (`packages/core-wasm/test/rehydrate.test.ts`)
pins the rebuild against the kernel's own math.

## Commits made during the pass

- `86ebbb78c` Read a KeyboardEvent's stroke members before matchStroke crosses to Rust
- `a4a6f8ed8` Report kernel errors to the console as live Errors, as the TypeScript does
