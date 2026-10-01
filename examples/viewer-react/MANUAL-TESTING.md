# Manual test script: full viewer on `main`

This script drives `examples/viewer-react`, the full `<PDFViewer>` demo, in Chrome. Every expected
result below was observed live against `main` at `2516e2786` (Merge #832) on 2026-10-01, in Chrome
with a 1400×717 CSS-pixel viewport. Zoom percentages that depend on window size are marked as such.

It covers the major features, not every edge. A full pass takes about 45 minutes.

## Setup

Use a fresh worktree so your own checkout stays untouched. Steps 2 and 3 take about a minute each.

```sh
# 1. Worktree on main
git worktree add ../epdf-manual main
cd ../epdf-manual

# 2. Dependencies
pnpm install --frozen-lockfile

# 3. The wasm runtime payload. Without this, step 4 fails with
#    'Could not resolve "@embedpdf/engine-runtime-wasm32"'.
pnpm --filter @embedpdf/engine-runtime fetch:payload

# 4. Build everything the demo depends on (37 tasks)
pnpm exec turbo run build --filter='@embedpdf/example-viewer-react^...'

# 5. Start the demo on http://localhost:5240 (strictPort, so free that port first)
pnpm --filter @embedpdf/example-viewer-react dev
```

Open `http://localhost:5240` in Chrome and open DevTools (Cmd+Option+J).

### Console helpers

Several checks read state that isn't visible. Paste this into the DevTools console once per page
load. It defines `window.t`.

```js
window.t = (() => {
  const v = document.querySelector('embedpdf-viewer').viewer;
  // documents.save() returns the PDF bytes. It does not trigger a browser download.
  const pdf = async (id = v.documents.getActiveId()) =>
    new TextDecoder('latin1').decode(await v.documents.save(id));
  return {
    v,
    // Annotation subtypes in the saved PDF, e.g. { "/Subtype/Ink": 1 }
    annots: async (id) =>
      ((await pdf(id)).match(/\/Subtype ?\/\w+/g) || [])
        .filter((s) => !/Form|Type|Widget/.test(s))
        .reduce((m, s) => ((m[s] = (m[s] || 0) + 1), m), {}),
    // Current /V of a form field in the saved PDF (last incremental revision wins)
    field: async (name, id) => {
      const objs = (await pdf(id))
        .split(/\bendobj\b/)
        .filter((o) => new RegExp(`/T ?\\(${name}\\)`).test(o));
      return objs.at(-1)?.match(/\/V ?(\([^)]*\)|\/\w+)/)?.[1];
    },
    // Raw saved text, for one-off greps
    pdf,
    // Command state: { enabled, active, shortcuts, ... }
    cmd: (id) => v.resolve(id),
  };
})();
```

The demo loads two tabs, Proposal and Contract. Both are `public/testlab.pdf`, a 3-page form
fixture with no annotations. Step 2 opens `public/ebook.pdf`, a 4-page document whose baseline is
`await t.annots()` → `{ "/Subtype/Square": 1, "/Subtype/Circle": 1, "/Subtype/PolyLine": 1,
"/Subtype/Polygon": 1, "/Subtype/Ink": 1 }`.

## 1. Load and tabs

1. Load the page.
   - Expected: two tabs, Proposal (selected) and Contract. Proposal shows "Interactive PDF Test
     Lab" and the pill reads "Page 1 of 3". Zoom box reads `100`.
   - Expected console: one `[engine-local] open() called without scope` warning and a 404 for
     `/favicon.ico`. Nothing else at error level.
2. Click the Contract tab.
   - Verify: `t.v.documents.getActiveId()` → `"contract"`.
   - The × close button only appears on the selected tab.

## 2. Open a file

1. Click the `+` after the tabs and pick `examples/viewer-react/public/ebook.pdf`.
   - Expected: a third tab "ebook.pdf", selected. Page 1 shows a blue ink stroke, a red polygon,
     square and ellipse over the cover. Pill reads "Page 1 of 4".
   - Verify: `t.v.documents.list()` has a third entry with `name: "ebook.pdf"`, `status: "ready"`,
     `pageCount: 4`.

Steps 3 to 12 use the ebook.pdf tab. Opened files don't survive a reload, so reopen it if you
reload.

## 3. Zoom

1. Click Zoom in (⊕). Expected: zoom box `100` → `120`.
2. Open the zoom dropdown (chevron next to the percentage) and pick Fit to Page.
   - Expected: the whole page fits vertically; zoom box reads `54` at a 1400×717 viewport.
   - Reopen the dropdown: Fit to Page now shows a check mark.
3. Pick Fit to Width. Expected: page fills the width; `166` at 1400 px wide.
   - Verify: `t.cmd('zoom:fit-width').active` → `true`, `t.cmd('zoom:fit-page').active` → `false`.
4. Press Cmd+0 (Ctrl+0 elsewhere). Expected: back to Fit to Page.
5. Click in the zoom box, type `75`, press Enter. Expected: box reads `75`.
   - Verify the scale numerically: ebook.pdf is US Letter (612 × 792 pt) and 100% renders 1 pt as
     4/3 CSS px, so at 75% a page is `612 × 4/3 × 0.75 = 612` px wide. In the Elements panel, hover
     the largest `<img>` inside `embedpdf-viewer` → `#shadow-root`; it should measure 612 wide.
   - `t.cmd('zoom:in').shortcuts` lists `Mod+=` and `Mod+NumpadAdd`. Only Cmd+0 was exercised for
     this script.

## 4. Sidebar

1. Click Sidebar (the panel icon, second from left).
   - Expected: a left panel with Thumbnails and Outline headings and page thumbnails.
2. Click thumbnail 3. Expected: the main view jumps to page 3, thumbnail 3 gets a blue outline,
   pill reads "Page 3 of 4".
3. Click Outline. Expected on `main`: nothing happens. It's a static label
   (`packages/viewer/chrome/src/ui/panels.tsx:39`), not a tab.
4. Click Sidebar again to close it.

## 5. View controls

Open View Controls (the page-with-gear icon, third from left).

1. Pick Two Page. Expected: pages sit side by side (3 and 4 together when you're on page 3).
   - Verify: `t.cmd('spread:odd').active` → `true`. Switch back to Single Page.
2. Pick Rotate Clockwise. Expected: every page in the main view rotates 90°; thumbnails don't.
   - Verify it's a view rotation, not an edit: `t.v.documents.getRevision(t.v.documents.getActiveId())`
     → `0`, and `t.cmd('history:undo').enabled` → `false`. Rotate Counter-Clockwise to restore.
3. Pick Horizontal under Scrolling. Expected: pages laid out in a row, scrolled sideways. Switch
   back to Vertical.
4. Pick Full Screen. Expected: the browser goes full screen.
   - Verify: `document.fullscreenElement.tagName` → `"HTML"`. The whole host page goes full
     screen, not just the viewer element. Press Esc to leave.

## 6. Pan

1. Click Pan (hand icon). Drag the page upward by about 200 px.
   - Expected: the cursor is a grab hand and the document scrolls down by the same distance.
   - Verify: `t.cmd('pan:toggle').active` → `true`.
2. Click Pointer (arrow icon) to return to the default tool.

## 7. Text selection and copy

1. With Pointer active, scroll to page 1 and drag across "PDF Viewers Are a".
   - Expected: the text highlights in blue with a handle at each end, and a small floating strip
     with a Copy button appears.
2. Click Copy. Verify in a terminal: `pbpaste` prints `PDF Viewers Are a`.

## 8. Search

1. Click Search (magnifier, top right). Type `embedpdf`, press Enter.
   - Expected: "8 results found", grouped under Page 1, Page 3 and Page 4, each with a snippet.
2. Tick Case sensitive. Expected: "No results". Change the query to `EmbedPDF`, press Enter:
   "8 results found". Untick Case sensitive.
3. Tick Regular expression, search `pain|sanity`. Expected: "3 results found" (pages 1, 3, 4).
4. Click the Page 4 result. Expected: the view jumps to page 4, "pain" gets an orange highlight,
   pill reads "Page 4 of 4".
5. Close the search panel. On `main` the orange highlight on "pain" stays until the next search.

## 9. Annotate

Click Annotate in the mode bar. A second toolbar appears with Comment, Link, Highlight, Strikeout,
Underline, Squiggly, Draw, Ink Highlighter, Text, Insert Text, Replace Text, Callout, Style, Undo and
Redo.

Undo and Redo are always disabled on `main`: both commands are stubs (`enabled: () => false`,
`packages/viewer/chrome/src/config/commands.ts:813`). Expect that throughout.

1. Highlight. Pick Highlight, drag across "Visit our GitHub repository today to see" on page 4.
   - Expected: yellow highlight, selected, with a strip of Comment, Style, Make stamp, Link, Delete.
   - Verify: `await t.annots()` now includes `"/Subtype/Highlight": 1`.
2. Delete. With the highlight still selected, click the trash icon in its strip.
   - Verify: `/Subtype/Highlight` is gone from `await t.annots()`.
   - The Delete and Backspace keys do nothing on `main`; there's no key binding for it.
3. Draw. Pick Draw, drag a squiggle in empty space below the text.
   - Expected: red freehand stroke, selected, with a rotation handle above it.
   - Verify: `/Subtype/Ink` goes from 1 to 2.
4. Text box. Pick Text, click empty space. A box appears with a blinking caret. Type `Hello` at
   normal speed, then click elsewhere on the page.
   - Expected: red text "Hello" in a red-bordered box.
   - Verify: `"/Subtype/FreeText": 1`.
   - Typing has to be human-speed. Scripted input with no delay between keys
     (`chrome-devtools-mcp` `type_text`) dropped about every other character: "Manual test note"
     became "Mnalts oe". Human-speed key presses landed in full.

## 10. Comments panel

1. Click Comment (speech bubble, top right).
   - Expected: a Comments panel listing every annotation, grouped by page ("Page 1 · 5 comments",
     "Page 4 · …"), each with author "Unknown", a date, a Status dropdown and an "Add a comment…"
     box.
2. Under the first entry, type `Reply from manual test` in "Add a comment…" and press Enter.
   - Expected: the reply appears under that entry with today's date; the input becomes "Reply…".
   - Verify: `(await t.pdf()).includes('Reply from manual test')` → `true`, and `/Subtype/Text`
     appears in `await t.annots()`. The reply is a Text annotation with an `/IRT` pointer to its
     parent: `(await t.pdf()).match(/\/IRT/g).length` → `1`.
3. Close the panel.

## 11. Shapes and style

1. Click Shapes. Pick Rectangle, drag a box in empty space.
   - Expected: red-outlined rectangle, selected with eight handles.
   - Verify: `/Subtype/Square` goes from 1 to 2.
2. With it selected, click Style (palette icon) in the strip.
   - Expected: a Style panel with "1 SELECTED", Stroke and Fill swatches, Opacity 100%, Stroke
     width 6px, Border, and a FLAGS (/F) list.
3. Click the green Fill swatch (`#00cc66`).
   - Expected: the rectangle fills green.
   - Verify: `(await t.pdf()).match(/\/IC ?\[[^\]]*\]/g)` includes `"/IC[ 0 .80000001 .40000001]"`.

## 12. Insert

1. Click Insert, then Rubber Stamp. A Stamps panel lists APPROVED, NOT APPROVED, DRAFT, FINAL,
   COMPLETED, CONFIDENTIAL.
2. Click APPROVED, then click the page. Expected: an APPROVED stamp at that spot, selected.
   - Verify: `"/Subtype/Stamp": 1`.
3. Click Signature, then "New signature…". In the dialog, enter `Test User` as Name, pick the Type
   tab, type `Test User`, click Save.
   - Expected: the Signatures panel shows a "Test User" card. "IN THIS DOCUMENT" reads "This
     document has no signature fields."
4. Click the card, then click the page. Expected: "Test User" in italic at that spot, selected.
   - Verify: `/Subtype/Stamp` goes to 2. The placed mark is a stamp annotation, not a signature
     field: `(await t.pdf()).includes('/FT/Sig')` → `false`.

## 13. Form filling

Switch to the Proposal tab and click View. Paste the helpers again if you reloaded.

1. Calculation. Select the Seats value (`2`), type `3`, press Tab.
   - Expected: Total reads `45.00` and focus moves to Price.
2. Validation. Select all of the Email field, type `not-an-email`, press Tab.
   - Expected: a native browser alert, "Please enter a valid email address." After OK, the field
     reverts to its previous value.
3. Visibility. Click Hide field. Expected: the "This field can be shown or hidden." box disappears.
   Click Show field: it comes back.
4. Choices. Click Engine (radio), tick Search (checkbox), pick March in the list box and iOS in the
   Dropdown.
   - Verify each in the saved PDF:
     `await t.field('mode')` → `"/Engine"`, `await t.field('featureSearch')` → `"/Yes"`,
     `await t.field('monthList')` → `"(March)"`, `await t.field('transport')` → `"(iOS)"`.
5. Reset. Click Reset form.
   - Expected: Seats `2`, Total `30.00`, Display name empty.
   - Also expected: no radio is selected and Annotations is unticked, even though Viewer and
     Annotations were on at load. `testlab.pdf` gives `mode` and `featureAnnot` no `/DV` default,
     so reset sets them to Off, which is what the PDF spec says.
6. GoTo action. Click "Next: actions".
   - Expected: the view jumps to page 2 ("Navigation & Action Types") and zoom changes to Fit to
     Page (54% at 1400×717), because the destination is a `/Fit` view.
7. Focus and blur. Scroll to page 3. Click "Click here, then click away".
   - Expected: Event status reads "Focus (Fo) fired." Click elsewhere: "Blur (Bl) fired."
8. JavaScript. Click "Run app.alert()".
   - Expected: native alert "Hello from EmbedPDF Interactive Test Lab!". After OK, Event status
     reads "JavaScript action executed."

## 14. Form authoring

1. Still on Proposal, click Form. Toolbar: Text Field, Checkbox, Radio Button, Dropdown, List Box,
   Signature Field.
2. Pick Text Field and drag a rectangle in empty space on page 3.
   - Expected: a new empty field with a grey border appears where you dragged.
   - Verify the name: `(await t.pdf()).match(/\/T ?\(text_\d+\)/g)` → `["/T(text_1)"]`.
   - Verify the page. The field must land on the page you drew on, not the page the pill names:

     ```js
     (async () => {
       const s = await t.pdf();
       const last = {};
       for (const [, n, b] of s.matchAll(/\b(\d+) 0 obj([\s\S]*?)endobj/g))
         last[n] = b;
       const f = Object.values(last).find((b) => /\/T ?\(text_1\)/.test(b));
       const kid = f.match(/\/Kids ?\[ ?(\d+) 0 R/)[1];
       const pages = Object.values(last)
         .find((b) => /\/Type ?\/Pages/.test(b))
         .match(/\/Kids ?\[([^\]]*)\]/)[1]
         .match(/\d+(?= 0 R)/g);
       return (
         1 +
         pages.findIndex((p) => {
           let a =
             (last[p].match(/\/Annots ?(\[[^\]]*\]|\d+ 0 R)/) || [])[1] || '';
           const ref = a.match(/^(\d+) 0 R$/);
           if (ref) a = last[ref[1]];
           return new RegExp(`\\b${kid} 0 R`).test(a);
         })
       );
     })();
     ```

     Expected: `3`.

## 15. Redaction

Switch to the Contract tab.

1. Click Redact, then Mark for Redaction. Drag across the title "Interactive PDF Test Lab".
   - Expected: a purple outline around the title.
   - Verify: `(await t.pdf()).match(/\/Subtype ?\/Redact/g).length` → `1`.
2. Click Redactions (the second icon). Expected: the panel lists "Text mark · p. 1".
3. Click Apply Redactions. Expected: an in-app dialog, "Apply redactions? Permanently remove the
   content under 1 redaction marks. This cannot be undone." Click Apply.
   - Expected: the title becomes a solid black box. The panel reads "No pending redactions."
   - Verify the mark is consumed: the `/Redact` count above is now `0`.
4. Verify the text is gone. Open Search on Contract and search `Test Lab`: "No results". Switch to
   Proposal and run the same search: "1 results found". Search state is per document.

## 16. Measure

Back on Proposal, click Measure. Toolbar: Distance, Perimeter, Area, Calibrate scale, a
"Scale: 1:1" button, Style. Perimeter and Area aren't covered here.

1. Set zoom to Fit to Page so you know the scale. Pick Distance. Press at a point in empty space,
   drag right about 2 inches of page (144 pt), release, then click once more at the end point.
   - Expected: a red double-arrow line labelled about "0.05 m" (2 in = 0.0508 m; default unit is
     metres).
   - Before the final click the line is still a draft: `t.cmd('annotation:cancel-creation').enabled`
     → `true`, and it isn't in the saved PDF. After the click that's `false`.
   - Verify: `(await t.pdf()).match(/\/Contents ?\([^)]*\)/g)` includes `"/Contents(0.05 m)"`, and
     the Comments panel lists it under its page.
2. Click Calibrate scale. Drag a line of the same length and release.
   - Expected: a "Calibrate scale" dialog with Known length, Unit, "Apply to all pages",
     "Recalculate existing measurements" (ticked).
3. Enter `10`, choose `in`, click Apply scale.
   - Expected: the toolbar button reads "Scale: 1 in = 5.0282803 in" (or close, depending on your
     line), and the distance from step 1 now reads "10 in".
   - Verify: `/Contents(10 in)` in `await t.pdf()`.

When you drag near a selected annotation, watch its floating strip: one stray click on its trash
icon deletes it, and with Undo stubbed there's no way back.

## 17. Download and print

1. Open Menu (≡, far left). Items: Download, Print, Full Screen.
2. Click Download.
   - Expected: Chrome saves `document.pdf` to `~/Downloads`. The name is always `document.pdf`,
     not the tab name.
   - Verify it carries your edits: `grep -ac '/Contents(10 in)' ~/Downloads/document.pdf` → `1`.
3. Click Print (or press Cmd+P). Expected: Chrome's print dialog for the whole host page. To check
   without the dialog: `window.print = () => console.log('print called')`, then
   `t.v.execute('document:print')` logs once.

## 18. Closing tabs

1. Open ebook.pdf again (step 2) so it's the selected tab, then click its ×.
   - Expected: the tab closes and a remaining tab becomes active. There's no "unsaved changes"
     prompt, even with edits.
   - Expected console on `main`: `[kernel] Error: Capability "stage" unavailable: document
"ebook.pdf-…" is closing.`, repeated 22 times. Closing a tab that isn't selected
     (`await t.v.documents.close('contract')` from the console) logs nothing.
2. With one tab left, there's no × on it, so the last document can't be closed from the UI.

## Known behavior on `main`

These were observed while writing the script. They're expected results today, not tester error.

- Undo and Redo are always disabled (stubs, `commands.ts:813`).
- No keyboard shortcut deletes a selected annotation.
- The Outline sidebar heading does nothing.
- Closing the selected tab logs 22 `[kernel] Capability "stage" unavailable` errors.
- Fast scripted typing into a FreeText box drops characters.
- Download always names the file `document.pdf`.
- The search hit highlight stays after closing the Search panel.

Two things were seen once and didn't reproduce from a clean reload. If you hit either, note the
steps that led there:

- After a long session of mode, zoom and tab switches on Proposal, the page pill stayed on "Page 1"
  while page 2 filled the view, and a Text Field drawn on page 2 landed on page 1. Step 14 checks
  for the placement half of this.
- Right after switching Scrolling from Horizontal back to Vertical, the pill showed "Page 3 of 4"
  with page 2 in view until the next scroll.
