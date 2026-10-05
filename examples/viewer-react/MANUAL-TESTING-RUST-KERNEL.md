# Manual test script: full viewer on the Rust kernel

This is the `research/multi-platform-rust-core-impl-plugins` version of `MANUAL-TESTING.md`. That
branch moves the kernel and the plugins into Rust (`@embedpdf/core-wasm`) and runs the same viewer
chrome on them through `examples/rust-kernel-eval`, not `examples/viewer-react`. The engine
underneath is the Rust core from `research/multi-platform-rust-core-impl`.

Every expected result below was observed live on 2026-10-01 at `b450a9909` ("Share search hits
instead of copying them per page of results"), in Chrome with a 1400×717 CSS-pixel viewport. Where
the Rust kernel differs from `main` (`2516e2786`), the step says so. Section numbers match
`MANUAL-TESTING.md`, so the two runs line up side by side.

> [!NOTE]
> Since `adda0bbea`, three of the differences below are fixed (`RUST-KERNEL-FINDINGS.md`). From
> that commit on, expect `main`'s results instead: the Comments panel opens and section 10 of
> `MANUAL-TESTING.md` passes as written, so section 19's failure no longer applies and you can run
> Comments in order. The active search hit is orange (section 8), and the first form script
> doesn't reload the page (section 13). Rebuild the kernel (`pnpm eval` does) after pulling.

A full pass takes about 45 minutes, plus about 3 minutes of builds the first time.

## Setup

Use a fresh worktree so your own checkout stays untouched.

```sh
# 1. Worktree
git worktree add --detach ../epdf-manual-kernel research/multi-platform-rust-core-impl-plugins
cd ../epdf-manual-kernel
git log -1 --oneline                            # note the commit you're testing

# 2. Dependencies, about a minute
pnpm install --frozen-lockfile
```

### 3. The wasm runtime

This is the same as the Rust core branch, because this branch pins the same PDFium fork commit
(`7c7b600`). `fetch:payload` has nothing published for it, and the `wasm32` build it suggests can't
link the Rust core, so you build `wasm32-eh` from a fork archive.

3a. Emscripten 4.0.0. `rust-toolchain.toml` pins Rust 1.98.0 with the `wasm32-unknown-emscripten`
and `wasm32-unknown-unknown` targets, and `rustup` installs them on the first `cargo` command.

```sh
git clone --depth 1 https://github.com/emscripten-core/emsdk.git ~/emsdk
(cd ~/emsdk && ./emsdk install 4.0.0 && ./emsdk activate 4.0.0)
source ~/emsdk/emsdk_env.sh
emcc --version | head -1                        # ... 4.0.0 (97c7c2ad...)
```

3b. The `wasm32-eh` fork archive for the submodule commit:

```sh
FORK_SHA=$(git ls-tree HEAD packages/engine/runtime/runtime-src | awk '{print $3}')
echo "$FORK_SHA"                                # 7c7b600b1... at b450a9909
```

Reuse an archive if one exists. The perf harness on `research/viewer-perf-compare` leaves one at
`perf-compare/.work/fork/$FORK_SHA/artifacts/libembedpdf-runtime-wasm32-eh-<sha9>.tar.gz`:

```sh
ARCHIVE=$(ls /path/to/perf-compare/.work/fork/$FORK_SHA/artifacts/*wasm32-eh*.tar.gz)
```

If not, build it from source with the commands in `MANUAL-TESTING.md` step 3b. They follow
`buildFork()` in `perf-compare/scripts/build-arm.mjs`, they weren't rerun for these scripts, and
the synced checkout is 6.7 GB.

3c. Build the runtime from the archive, then its JS wrapper. The eval app's `pnpm bootstrap` checks
for both `npm/wasm32/lib/embedpdf.wasm` and `dist/index.browser.js`, and fails without them.

```sh
jq --arg u "file://$ARCHIVE" --arg s "$(shasum -a 256 "$ARCHIVE" | cut -d' ' -f1)" \
  '.artifacts["wasm32-eh"] = {url: $u, sha256: $s}' \
  packages/engine/runtime/engine-runtime-build.json > /tmp/epdf-pin-eh.json

source ~/emsdk/emsdk_env.sh
PDF_RUNTIME_BUILD_FILE=/tmp/epdf-pin-eh.json \
  pnpm --filter @embedpdf/engine-runtime build:target wasm32-eh     # about a minute
pnpm --filter @embedpdf/engine-runtime build
```

Expected last line of the first build: `…/npm/wasm32/lib/embedpdf.wasm: all 918 bound functions
match their bindings`.

### 4. The eval app

The Rust kernel needs `wasm-bindgen-cli` at exactly 0.2.127, because `crates/epdf-web/Cargo.toml`
pins `wasm-bindgen =0.2.127`. A different CLI fails the build with a version mismatch.

```sh
wasm-bindgen --version || cargo install wasm-bindgen-cli --version 0.2.127
# expected: wasm-bindgen 0.2.127

cd examples/rust-kernel-eval
pnpm bootstrap          # about 20 s; ends "✓ setup complete: packages/engine/main/workers/ ..."

# Optional: confirm the engine payload carries the Rust core
(cd ../.. && node packages/engine/runtime/scripts/verify-rust-core.mjs)
# expected:   ok      the WASM payload carries the Rust engine core

# Builds the Rust kernel (about 60 s the first time), then serves http://localhost:5250
pnpm eval
```

There's no turbo build step: the eval app runs the viewer from source.

Open `http://localhost:5250` in Chrome and open DevTools (Cmd+Option+J).

### Console helpers

Paste the `window.t` block from `MANUAL-TESTING.md` into the console once per page load. It works
unchanged on the Rust kernel: `documents.save()` returns the PDF bytes through the shim, and
`resolve()` reports command state.

The app opens one document, `/ebook.pdf` (from `packages/viewer/main/public/`). Its baseline is the
same as on `main`: `await t.annots()` → `{ "/Subtype/Square": 1, "/Subtype/Circle": 1,
"/Subtype/PolyLine": 1, "/Subtype/Polygon": 1, "/Subtype/Ink": 1 }`.

### Things that differ from the viewer-react demo

- The page has a 36 px bar above the viewer: an orange "Rust kernel" badge, Zoom in and Zoom out
  buttons, and a label listing every open document and its status. The bar takes height from the
  viewer, so fit-to-page zoom values come out lower than on `main`.
- Document ids are the source path or a timestamp: `/ebook.pdf`, and `testlab.pdf-1790891126314`
  for an opened file. The tab names are `ebook` and `testlab.pdf`.
- `testlab.pdf` isn't served, so you open it from disk with the `+` button. It's at
  `examples/viewer-react/public/testlab.pdf` in this worktree.

## 1. Load and confirm the kernel

1. Load the page.
   - Expected: one tab, "ebook", selected, showing the EmbedPDF cover with its ink, polygon,
     square and ellipse. Pill reads "Page 1 of 4", zoom `100`. The top bar reads "/ebook.pdf
     (ready)".
   - Expected console: the same as `main`, one `[engine-local] open() called without scope`
     warning and a 404 for `/favicon.ico`.
   - With one tab open there's no × on it.
2. Confirm it's the Rust kernel, not the TypeScript one:
   - `t.v.documents.list.toString()` → `(...args) => this.call(id, member, args)`, the shim's wire
     call (the README's own check, `examples/rust-kernel-eval/README.md`).
   - `__epdfCounts()` returns the kernel's live tables, e.g. `{ refs: 645, hosts: 38,
documents: 8, wasmBytes: 9437184, … }`. On `main` it's `undefined`.

## 2. Open a file

1. Click `+` and pick `examples/viewer-react/public/testlab.pdf`.
   - Expected: a second tab "testlab.pdf", selected, "Page 1 of 3". The top bar lists both
     documents as ready.
   - Verify: `t.v.documents.list()` has `{ name: "testlab.pdf", status: "ready", pageCount: 3 }`.
2. Click the ebook tab. Steps 3 to 12 use it.

## 3. Zoom

Same as `main` except the fit values.

1. Zoom in: `100` → `120`.
2. Fit to Page: `51` at 1400×717 (`54` on `main`; the top bar costs the difference).
   `t.cmd('zoom:fit-page').active` → `true`.
3. Fit to Width: `166`. `t.cmd('zoom:fit-width').active` → `true`, fit-page → `false`.
4. Cmd+0: back to `51`.
5. Type `75`, Enter: the largest page `<img>` in the shadow root is 612 px wide, the same as
   `main`. `t.cmd('zoom:fit-page').shortcuts` → `["Mod+0"]`.

## 4. Sidebar

Same as `main`. Thumbnail 3 jumps to page 3 ("Page 3 of 4") and gets a blue outline. Outline is a
static label and does nothing.

## 5. View controls

Same as `main`.

1. Two Page: `t.cmd('spread:odd').active` → `true`, pages paired. Back to Single Page.
2. Rotate Clockwise: pages turn to landscape (792 × 612 px at 75%). Revision stays `0`, Undo stays
   disabled. Rotate back.
3. Horizontal: pages in a row at the same `top`. Back to Vertical.
4. Full Screen (from Menu): `document.fullscreenElement.tagName` → `"HTML"`. Esc to leave.

## 6. Pan

Same as `main`: the cursor is `grab`, a 200 px upward drag scrolls the document 200 px, and
`t.cmd('pan:toggle').active` → `true`. Switch back to Pointer.

## 7. Text selection and copy

Same as `main`. Clear the clipboard first so a leftover from another run can't pass the check:

```sh
printf cleared | pbcopy
```

Drag across "PDF Viewers Are a" on page 1, click Copy, then `pbpaste` → `PDF Viewers Are a`.

## 8. Search

Same counts and navigation as `main`: `embedpdf` → "8 results found" (pages 1, 3, 4); Case
sensitive → "No results"; `EmbedPDF` → 8; regex `pain|sanity` → "3 results found"; the Page 4
result → "Page 4 of 4".

Differs from `main` (Rust kernel bug, `RUST-KERNEL-FINDINGS.md` §2): the active hit is never
painted in the active colour. After clicking the Page 4 result, "pain" is yellow (`#ffd500`), not
orange (`#ff9632`), with the panel open or closed. `getActiveHitIndex()` is still right.

## 9. Annotate

Same as `main`. Undo and Redo are disabled here too.

1. Highlight across "Visit our GitHub repository today to see": `"/Subtype/Highlight": 1`.
2. Trash icon in the strip: the highlight is gone. Delete and Backspace do nothing.
3. Draw: `/Subtype/Ink` 1 → 2.
4. Text: type `Hello` at human speed → "Hello", `"/Subtype/FreeText": 1`. Scripted typing with no
   delay drops characters here too ("Manual test note" became "Mnul et oe").

## 10. Comments panel

Moved to the end of the script (section 19). On this branch, opening it leaves the right-hand panel
area broken until you reload, which would block sections 11 to 16.

## 11. Shapes and style

Same as `main`. Rectangle: `/Subtype/Square` 1 → 2. Style opens the panel with "1 SELECTED",
Stroke, Fill, Opacity 100%, Stroke width 6px, Border and FLAGS (/F). The green Fill swatch gives
`"/IC[ 0 .80000001 .40000001]"`.

If the Style panel doesn't appear (the button highlights but no panel shows), you've opened
Comments since the last reload. Reload and redo this section.

## 12. Insert

1. Rubber Stamp. Same as `main` in a clean browser profile: one "STANDARD STAMPS" library
   (APPROVED, NOT APPROVED, DRAFT, FINAL, COMPLETED, CONFIDENTIAL and more) and Import PDF….
   Custom stamp libraries persist in IndexedDB per origin, and with two or more the panel adds an
   "All stamps" filter. If you see one, `localhost:5250` has libraries left from earlier sessions
   (`RUST-KERNEL-FINDINGS.md` §5).
2. Click APPROVED, then click an empty part of the page: `"/Subtype/Stamp": 1`. Clicking on an
   existing annotation selects that annotation instead of placing the stamp, same as `main`.
3. Signature → "New signature…" → Name `Test User`, Type tab, `Test User`, Save. Expected: a "Test
   User" card. Saved signatures persist in browser storage for `localhost:5250`, so the panel may
   already list ones from earlier sessions (this run showed an "Eval Tester" card left over from
   one).
4. Click the card, click the page: italic "Test User" placed, `/Subtype/Stamp` → 2,
   `(await t.pdf()).includes('/FT/Sig')` → `false`. Per the eval README, no signer is wired in,
   so the mark is never sealed.

## 13. Form filling

Switch to the testlab.pdf tab and click View.

The first form script after starting `pnpm eval` makes Vite pre-bundle QuickJS and reload the page.
The `pnpm eval` terminal logs `new dependencies optimized:
@jitl/quickjs-singlefile-browser-release-sync`, the opened tab disappears, and the step you were on
looks like it failed: in this run, Seats snapped back to `2`. Reopen testlab.pdf with `+`, paste the
helpers again, and redo the step. In this run it happened once and not again, and the README lists
the same reload under "Known behaviors".

Once past that, everything matched `main`:

1. Seats `3`, Tab → Total `45.00`, focus on Price.
2. Email `not-an-email`, Tab → alert "Please enter a valid email address.", value reverts to
   `hello@embedpdf.com`.
3. Hide field removes the hidden-message box; Show field brings it back.
4. Engine, Search, March, iOS → `t.field()` gives `"/Engine"`, `"/Yes"`, `"(March)"`, `"(iOS)"`.
5. Reset form → Seats `2`, Total `30.00`, Display name empty, and `mode` and `featureAnnot` both
   `"/Off"`.
6. "Next: actions" → "Page 2 of 3", zoom Fit to Page (`51` here).
7. Focus and blur → "Focus (Fo) fired.", then "Blur (Bl) fired."
8. "Run app.alert()" → alert "Hello from EmbedPDF Interactive Test Lab!", then "JavaScript action
   executed."

## 14. Form authoring

Same as `main`. Text Field drawn on page 3 → `["/T(text_1)"]`, and the page check from
`MANUAL-TESTING.md` returns `3`.

## 15. Redaction

Use the testlab.pdf tab. There's only one, so you'll open a fresh copy for the comparison in step 4.

1. Redact → Mark for Redaction, drag across the title: purple outline, `/Redact` count `1`.
2. Redactions panel: "Text mark · p. 1".
3. Apply Redactions → the "Apply redactions?" dialog → Apply: black box, panel reads "No pending
   redactions.", `/Redact` count `0`.
4. Search `Test Lab` on this tab → "No results". Open testlab.pdf again with `+` and search the new
   tab → "1 results found".

## 16. Measure

Same as `main`. Do it on the fresh testlab.pdf tab from step 15.

1. Fit to Page reads `46` in Measure mode, because the Measure toolbar row takes more height. A
   144 pt Distance (88 px at 46%) reads "0.05 m". Before the final click
   `t.cmd('annotation:cancel-creation').enabled` → `true`, after it `false`, and the saved PDF has
   `/Contents(0.05 m)`.
2. Calibrate scale, drag the same length: the "Calibrate scale" dialog, the same fields as `main`.
3. `10`, `in`, Apply scale: "Scale: 1 in = 5.055147 in" (it depends on your line), and the distance
   becomes `/Contents(10 in)`.

## 17. Download and print

Same as `main`. Download saves `document.pdf`, or `document (1).pdf` if one is already there, with
your edits (`grep -ac '/Contents(10 in)'` → `1`). `t.v.execute('document:print')` calls
`window.print()` once.

## 18. Closing tabs

1. Select a tab with edits and click its ×. Expected: it closes with no prompt, and another tab
   becomes active.
   - Expected console: repeated copies of
     `[kernel] Error: Capability "stage" unavailable: document "<id>" is closing.`
     Every build logs them; the bug is in the shared close path, not the Rust kernel. Uploading
     `ebook.pdf` and closing it at once gave 16 here and 25 on both `main` and the plugins
     branch's TypeScript build. The count depends on view state, so don't treat a different
     number as a regression (`RUST-KERNEL-FINDINGS.md` §3).
2. With one tab left there's no ×, same as `main`.

## 19. Comments panel (expected to fail on this branch)

Run this last. Afterwards, reload before doing anything else.

1. Click Comment (speech bubble, top right).
   - Expected on this branch: the button highlights and `t.cmd('panel:comment').active` → `true`,
     but no panel appears. The console logs `Uncaught (in promise)`, stack through
     `useCommentThreads` (`packages/framework/react/src/annotation.tsx:1136`) and
     `comments-panel.tsx:53`. The Rust annotation capability has no `comments` member, so
     `c.comments.listThreads()` throws a `TypeError` (`RUST-KERNEL-FINDINGS.md` §1). It happens
     even as the first action after a fresh load.
   - On `main` this opens the Comments panel and the reply step works. The reply check (`/Text`
     with `/IRT`) can't run here.
2. After this, every right-hand panel (Style and Search both checked) stays empty until you
   reload, even after you close Comments. The left sidebar still works.

## Differences from `main`, in one place

Each is verified, with its cause, in `RUST-KERNEL-FINDINGS.md`.

Rust kernel bugs:

- The Comments panel never renders, and it blocks every right-hand panel until reload (section
  19). The capability lacks `comments`.
- The active search hit is never painted in the active colour (section 8). `getActiveHit()`
  returns a copy, and the layer compares by identity.

Not the Rust kernel:

- The first form script on a cold Vite cache reloads the page while Vite pre-bundles QuickJS
  (section 13). Dev-server tooling; the TypeScript dev server does the same.
- Closing a tab logs `stage` errors on every build (section 18).
- Fit values are lower (`51` against `54`) because of the eval app's top bar. That's layout.

Everything else in sections 1 to 18 matched `main`, including all the known `main` behavior: Undo
and Redo disabled, no Delete-key binding, a static Outline label, character drops under scripted
typing, and `document.pdf` as the download name.
