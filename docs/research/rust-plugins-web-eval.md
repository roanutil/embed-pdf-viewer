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
| Stamps | works | stamp, annotation | | Placing "Approved" gives a 164 x 50 pt stamp in both. |
| Images | works | annotation | | Clicking with the Image tool opens the picker (`accept="image/png,image/jpeg"`); a 120 x 60 PNG becomes a 120 x 60 `stamp`. |
| Form fill | works | form, actions | | `action_form_fixture.pdf`: text, checkbox, radio, dropdown, Hide/Show Field, Reset All and Next Page behave identically. |
| Validate and calculate scripts | works | actions, acrojs | | An invalid email raises the same `alert` text; `calcsum` follows `calc1 + calc2` (1284.5). |
| Format scripts | works | actions, acrojs | | Neither viewer reformats anything in the fixtures available. See "Form scripts". |
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
matched the TypeScript. Format scripts are unproven. `README.md` in the eval app says typing
`1234.5` into Venue shows `1,234.50`. Neither viewer does that: `calc1` keeps `1234.5`. A search of
every fixture's decompressed streams finds no `AFNumber_Format`, so the README describes a script
`action_form_fixture.pdf` does not have. The only format script in the fixtures is
`AFDate_FormatEx("yyyy-mm-dd")` in `packages/engine/main/test/fixtures/annot_javascript.pdf`, and
typing `3/14/2026` there leaves the value unformatted in both viewers. A fixture with a working
format script is needed before this row can say more than "same as TypeScript".

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

## Commits made during the pass

- `86ebbb78c` Read a KeyboardEvent's stroke members before matchStroke crosses to Rust
- `a4a6f8ed8` Report kernel errors to the console as live Errors, as the TypeScript does
