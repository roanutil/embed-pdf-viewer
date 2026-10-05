# Rust kernel: verified findings from the manual test pass

Two of the five differences that `MANUAL-TESTING-RUST-KERNEL.md` first reported are Rust kernel
bugs: the Comments panel and the active search hit. Two are real but not Rust-specific. The fifth
was stale browser storage.

All of them were re-checked on 2026-10-05 against three builds side by side in one Chrome, at
1400×717:

| Port | Build                                            | Kernel                       | Chrome code    |
| ---- | ------------------------------------------------ | ---------------------------- | -------------- |
| 5240 | `main` `2516e2786`, `examples/viewer-react`      | TypeScript                   | `main`         |
| 5241 | plugins `b450a9909`, `examples/viewer-react`     | TypeScript                   | plugins branch |
| 5250 | plugins `b450a9909`, `examples/rust-kernel-eval` | Rust (`@embedpdf/core-wasm`) | plugins branch |

5241 is the control. It runs the same branch's chrome on the TypeScript kernel, so a behavior that
shows on 5250 and not on 5241 belongs to the Rust kernel, not to chrome changes made on the branch.

Status at `adda0bbea`: findings 1, 2 and 4 are fixed on
`research/multi-platform-rust-core-impl-plugins`. Each fix was re-checked in the eval app on a
cold Vite cache in a fresh browser profile, and `@embedpdf/core-wasm`'s `test:wasm` (59 tests, plus
10 GC tests) and `@embedpdf/plugin-search`'s suite (9 tests) pass. Finding 3 is still open.

| #   | Fix                                                                                    | Re-check                                                                                                                         |
| --- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| 1   | `fce880492` "Give the annotation capability its comments and links objects"            | Panel shows "Page 1 · 5 comments"; a reply saves as `/Text` with one `/IRT`; Search and Style open afterwards; no console errors |
| 2   | `ca6d809a7` "Answer the active search hit as its entry in listHits on the Rust kernel" | Active hit `rgb(255, 150, 50)` with the panel open and closed, and it moves when another result is picked                        |
| 4   | `adda0bbea` "Pre-bundle the JSX dev runtime and QuickJS in the eval dev server"        | No reload on first load or on the first form script; Seats `3` → Total `45.00` with the uploaded tab kept                        |

The sections below describe the bugs as found at `b450a9909`.

## 1. Comments panel never renders (Rust kernel bug)

Clicking Comment highlights the button and `viewer.resolve('panel:comment').active` turns `true`,
but no panel appears. The console logs `Uncaught (in promise)`, with a stack through
`useCommentThreads` (`packages/framework/react/src/annotation.tsx:1136`) and `CommentsPanel`
(`packages/viewer/chrome/src/ui/comments-panel.tsx:53`). It happens on the first click after a
fresh load. On 5241 the same click opens the panel ("No comments yet. Use the note tool to start a
thread.").

The capability has no `comments` member. The TypeScript contract requires it
(`comments: CommentsApi`, `packages/plugin/annotation/src/contract.ts:708`), and the hook calls
`c.comments.listThreads()`. On 5250:

```js
const { AnnotationToken } =
  await import('/@fs/<worktree>/packages/core-wasm/src/redirect/plugin-annotation.ts');
const cap = document
  .querySelector('embedpdf-viewer')
  .viewer.get(AnnotationToken);
typeof cap.comments; // "undefined"
cap.comments.listThreads(); // TypeError: Cannot read properties of undefined (reading 'listThreads')
```

The Rust side has the code. `CommentsLens` is exported from `crates/plugins/annotation/src/lib.rs:55`
and implements `listThreads` (`comments/comments.rs:103`), but the harness only reaches it through
the test-only `annotation-lens` plugin, which "exposes the `comments` and `links` verbs flat"
(`crates/plugins/annotation/src/harness/mod.rs:11`, registered at `:1019`). The production
annotation capability never gets a nested `comments` object. `links` is missing the same way; no
chrome code calls `.links.`, so nothing visibly breaks from that today.

The failure spreads. After Comments has failed once, closing it and opening Style or Search leaves
the right-hand panel area empty, even though both commands report `active: true`. The left
sidebar still works. On a fresh load, Style works until Comments is opened. Only a reload
recovers. On 5241, Search opens normally after Comments.

`docs/research/rust-plugins-web-eval.md` has no row for the Comments panel, so its pass never
reached this.

## 2. Active search hit isn't highlighted (Rust kernel bug)

Search `pain` on `ebook.pdf`, click the Page 4 result. The hit should be painted in the active
colour, `#ff9632`. On 5241 its computed background is `rgb(255, 150, 50)`. On 5250 it's
`rgb(255, 213, 0)`, the ordinary-hit colour `#ffd500`, whether the panel is open or closed. In the
first pass it was only noticed after closing the panel, but it's wrong from the moment the result
is clicked.

The index is right. On 5250, `getActiveHitIndex()` returns `1` and `getActiveHit()` has the same
value as the matching entry of `listHits({ page })`. They're different objects, though:

```js
const s = viewer.get(SearchToken); // SearchToken from core-wasm's plugin-search redirect
const active = s.getActiveHit();
const same = s
  .listHits({ page: active.page })
  .find((h) => h.charStart === active.charStart);
same === active; // false
JSON.stringify(same) === JSON.stringify(active); // true
s.getActiveHit() === s.getActiveHit(); // true
```

`SearchLayer` picks the colour with `hit === active` (`packages/framework/react/src/search.tsx:51`),
so on the Rust kernel no hit ever matches. Each getter is reference-stable on its own, but the two
hand back separate copies of the same hit. The branch tip, `b450a9909`, is "Share search hits
instead of copying them per page of results". That commit made the per-page arrays shared. This
check suggests `getActiveHit` still returns its own copy. I haven't traced it in the Rust code.

Either side could fix it: the shim could return the shared hit object from `getActiveHit`, or the
layer could compare by `page` and `charStart`. The first keeps the TypeScript contract's identity
guarantee, which other `Object.is` selectors may rely on too.

## 3. Closing a tab logs `stage` errors (all kernels, not Rust-specific)

Closing the selected tab logs `[kernel] Error: Capability "stage" unavailable: document "<id>" is
closing.` many times on every build. Under identical steps (fresh load, upload `ebook.pdf`, click
its × at once) the counts were:

| Build                    | Copies |
| ------------------------ | ------ |
| 5240 `main`              | 25     |
| 5241 plugins, TypeScript | 25     |
| 5250 plugins, Rust       | 16     |

The first pass reported 22 for `main` against 16 for Rust. The 22 came from a session with
different view state, so the count depends on what's mounted. It isn't a fixed per-kernel number.

On `main` the stack is `closeDocument` → `closeSession` → `unpublishSlot` → `setCore` → a store
listener (`store.ts:69`) that resolves `stage` for the document being closed. The store catches
each throw and hands it to the kernel's default reporter, `console.error('[kernel]', …)`
(`packages/core/main/src/kernel.ts:204`). On Rust the same message comes from inside
`epdf_web_bg.wasm`, reached from `tab-bar.tsx:86`. The bug is in the shared close path: listeners
run while the closing document's capabilities are being torn down. The Rust kernel just notifies
fewer of them.

`rust-plugins-web-eval.md` covers the related kernel-destroy reports (16 on Rust, 17 on
TypeScript, "Stage reports at destroy"). That's the destroy path, not this one.

## 4. First form script reloads the page (dev server, already documented)

On a cold Vite cache, the first form script makes Vite optimize
`@jitl/quickjs-singlefile-browser-release-sync` and reload the page. Uploaded documents are lost,
so the action looks like it failed (Seats snapped back to `2`). Reproduced after deleting
`examples/rust-kernel-eval/node_modules/.vite`. On the second attempt, with the cache warm, the
same steps gave Total `45.00` with no reload. A fresh cache also reloads once on first page load,
for `react/jsx-dev-runtime`.

This isn't the Rust kernel. `rust-plugins-web-eval.md:125` records the same QuickJS reload in the
TypeScript dev server, and the eval README lists first-load reloads under "Known behaviors". The
sandbox reaches QuickJS through the `#quickjs-variant` subpath import
(`packages/core/js-sandbox/package.json:15`), and Vite's dependency scan doesn't pick it up until
it's first imported. Adding the package to `optimizeDeps.include` in the dev configs would
probably stop the reload. I didn't try it.

## 5. Stamps panel "All stamps" filter (not a bug)

The first pass saw an "All stamps" library filter and a signature ("Eval Tester") among the stamps
on 5250, and nothing like it on `main`. The panel shows that filter "once there are two"
libraries, and custom libraries persist in IndexedDB (`packages/viewer/chrome/CHANGELOG.md:29`,
`stamps-panel.tsx:6`). The Chrome profile used for testing had 8 stamp libraries in
`localhost:5250`'s `embedpdf-stamps` database, left from earlier sessions.

In a clean profile (an isolated browser context) the Rust kernel shows one "STANDARD STAMPS"
library and no filter, the same as the TypeScript control. That's still true after creating a
signature and after a reload.
