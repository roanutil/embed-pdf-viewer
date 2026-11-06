# Pinned latest-upstream core/runtime catch-up scope and proposed plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. This document authorizes no implementation.

**Goal:** advance both private research branches from the reviewed `f28ed4ea4aa46c8e69676558993b103d68b19e08` baseline to a reviewed integration containing canonical viewer `4ad8986099314b9587df423c802db103a21d665d`, with matching Rust behavior and rebuilt runtime consumers at a declared immutable runtime target.

**Architecture:** merge canonical TypeScript without replacing private additions, merge the upstream runtime into the fork before rebuilding, then port changed behavior at its current owning layer. Both branch names currently identify the same tree; one integration workstream and a shared final closure commit avoid recreating an unrequested split.

**Tech stack:** TypeScript engine/core oracle, Rust/PDFium, existing C ABI/UniFFI/WASM adapters and content-addressed runtime/Apple payloads. Final plugin/native details are in plugin-binding-inventory.md/json; scope-and-plan.md consolidates the dependency jobs and supersedes provisional estimates.

**Spec:** coordinator916's new latest-main scope, frozen in `pinned-targets.json`; `core-runtime-inventory.json` contains exact feature commits, owners, proof paths and evidence hashes. Original-target C5h remains complete and is the baseline, not reopened work.

## Global constraints

All repository Git/API/source operations run inside approved image `sha256:edfb3af3c66b0c9fa51322b39e524d3996d30da7e25d61edab8e9a13508e4703`. Preserve the user checkout, all original source/evidence/releases and existing intentional divergences. Planning here performed no source edits, merges/probes, tests, builds, remote changes or publications. Future implementation uses new owned worktrees/targets and the existing heavy-build/FIFO policy. No global credentials/configuration changes. `.github` changes are upstream integration only. One changeset per directly changed package is the repository rule. No implicit new NuGet publication or additional validated RIDs.

## Fixed identities and recommended decisions

| Identity | Frozen value |
|---|---|
| Canonical viewer | `https://github.com/embedpdf/embed-pdf-viewer.git`, default `main`, `4ad8986099314b9587df423c802db103a21d665d` |
| Viewer commit time / observation | `2026-11-04T15:23:43+02:00` / `2026-11-04T13:25:23Z` |
| Viewer-selected runtime | `f1ba46f0dc3dc5f9b055a7311dda0c13ecc0f185`, `2026-11-04T12:24:24+02:00` |
| Canonical runtime | `https://github.com/embedpdf/runtime.git`, default **`embedpdf/main`**, `eeac787c02fac7b40c74c91bbff0ffb5f59ff97e` |
| Baseline private branches | Both `research/multi-platform-rust-core-impl` and `research/multi-platform-rust-core-impl-plugins` freshly verified `f28ed4ea4aa46c8e69676558993b103d68b19e08` at `13:26:32Z` |
| Baseline runtime | `3c55b7a1c0bf956231392a573e87f28dd6d6667d` |

The canonical repos were verified non-forks through their existing origin remotes, repository API metadata and viewer `.gitmodules`. Fork `main` is not the upstream target. Do not refresh these pins during implementation without changing the approved scope.

Runtime choice is explicit. Exact-viewer option **A** merges `f1ba46f0d`; it reproduces the viewer's declared association but omits an already identified rich-comment reader-default correction. Recommended option **B** merges pinned runtime `eeac787c`, which is exactly two history commits / one nonmerge functional fix beyond `f1ba`: `8bdd5c097be2c29f72a70dd13e0a1e98ce28f65c` makes a markup comment ignore drawing `/DA` and use reader defaults, preserving FreeText/widget behavior. It changes five files and no exported signatures. Treat B as a named supplemental runtime fix, not an automatic upgrade to an unspecified future runtime HEAD. Root/user plan review chooses the fixed option before implementation.

Recommended branch option: build one reviewed integration from now-identical f28ed, merge pinned main once, port all required Rust/plugin/binding deltas once, then advance both private branch names to the same closure commit. Alternative: intentionally recreate engine-only impl and a plugin child, requiring a new written layering rule and separate acceptance. The existing work-distribution plan does not impose permanent divergence; the user just completed the shared tree. Replacing endpoint files or cherry-picking only feature subjects is unsuitable: it can drop private policy/tests and fail to include target ancestry. Public plugin mirroring, if desired, names the exact remote in the execution scope; this plan does not create a public impl branch.

## History is evidence, not the work estimate

Viewer merge base is prior actual upstream target `a40d63a199f2d483fd27c9ad439f0d5efbc195aa`; baseline has 1,890 private-only history commits and pinned upstream has 82, of which 51 are nonmerges. Upstream changed 794 paths, 151 under core/engine. Runtime merge base is `28b8b808f52d544b17a973199410cda0cc43530f`; fork has 11 history commits to preserve and selected upstream f1ba has 38, of which 25 are nonmerges. `git cherry` finds no exact patch-equivalent upstream nonmerge commits against either baseline; that does not determine cross-language behavior.

Already completed original-target work stays completed. Existing rich-text DTOs/native functions, raw font facts, owner-thread lifecycle, transactions and current native ABI are foundations to reuse. The existing C5f 340 passed / 2 skipped / 0 failed result is baseline evidence, not a C7 pass. New behavior is grouped below instead of counting every merge, version or follow-up as a port.

## Functional inventory and owners

| Change | Confirmed baseline gap / required behavior | Owning source and eventual proof |
|---|---|---|
| Annotation sibling-kind conversion | `write/patch.rs:81` currently refuses all subtype changes. Target supports highlight/underline/squiggly/strikeout and square/circle sibling sets, preserves ref/nm and refuses fields the destination cannot hold. Runtime adds `EPDFAnnot_SetSubtype`. | TS `annotation/siblings.ts`, resolvers, `AnnotationMutator.ts`; Rust `annotation/write/{patch,prepare,update}.rs`, `pdfium/src/annot_write.rs`; target sibling tests plus Rust update/transaction vectors. |
| Null stroke and dash | `DraftFields.color: Option<Rgb>` refuses explicit null; create defaults red. Target writes null as `/C []` through new `EPDFAnnot_SetNoColor`, omitted color retains defaults. Existing dash writer accepts `[]`; target refuses `[]`, accepts null and nonnegative nonempty arrays **including `[0]`**, and writes omitted dashed default `[3,3]`. Imported empty dash reads solid. | TS `base.schema.ts`, `resolve/dash.ts`, writer/readStyle; Rust `annotation/write/{prepare,create,update}.rs`, `read/style.rs`, pdfium writer; raw dictionary/readback and no-stroke raster cases. |
| Ink curves | New shared Bezier/ink fitting and matching runtime appearance fitter. Existing cloudy Bezier code is not a port of the new algorithm. | TS `core/annotation/src/{bezier,ink-curves}.ts`, shapes/points and new tests; Rust annotation-core scene/geometry, coordinated with plugin worker; native `cpdf_ink_curves*` / `epdf_ink_appearance_embeddertest.cpp`. |
| Markup rich-text comments | Current rich-text foundation describes FreeText. New markup `/RC` read/write source, reader body defaults, contents/plain projection agreement and tri-state updates must be carried. Comment writes preserve drawing appearance. | TS `resolve/comment.ts`, `dto/RichText.schema.ts`, `richTextWire.ts`, writer registry; Rust `annotation/rich_text.rs`, readers and write prepare/patch/create; newline/mismatch/null/source and no-redraw cases. |
| Final `/XYZ` reading | Current Rust uses `raw.location()` and preserves literal zero. Target follow-up `01f97b459` reads raw params/null bits, keeps truncated axes, maps zoom `0` to null. Earlier `3fd007402` is superseded, not a second feature or an already-completed final port. | TS `readDestination.ts`, `destination-xyz.test.ts`; Rust `features/actions/destination.rs`, `pdfium/src/action.rs`; zero/null/positive and short-array outline/link/open-action cases. |
| Standard fallback and font identity | Native changes make missing Latin draw in the standard stand-in and registered `/BaseFont` use PostScript identity. New TS final DTO mapping appends the standard display hash unless already primary. Raw Rust font-fact reader is reusable; trace shared consumer adapter before inventing a duplicate mapper—no final `fontResolutionOf` mapper was found in crates. | TS `fonts/fontList.ts`, `annotation/fontFaces.ts`, `standard-fallback.test.ts`; Rust raw `fonts/document.rs`, pdfium document facts, existing font vectors; plugin worker owns UI cache/picker integration. |

`core-runtime-inventory.json` gives exact full commit SHAs for every row. The original rich-text/plain-text helpers are reusable foundations, not proof that new markup semantics already work. Standard fallback needs both native effect and consumer display-chain validation; neither a new wire kind nor an invented Rust mapper is justified from the current evidence.

The public C header comparison is **1,066 → 1,068 exported functions**, exactly `EPDFAnnot_SetNoColor` and `EPDFAnnot_SetSubtype`, zero removals, zero signature changes. `worker-protocol.ts` is unchanged across endpoints: existing wire kinds acquire new payload/read/write semantics. The generated TS function table adds the same two names. Regenerate bindings against actual merged fork headers and verify exported symbol association; header presence alone is not a built-library pass.

Runtime also carries linearized-file layer opening, color-space/image handling fixes, and DEPS rolls for Skia, V8, ICU, fast_float, gtest, llvm-libc, zlib, tooling. `.gn` disables ICU4X ECMA-402 support for this PDFium configuration. The 11 fork-history commits include target-args/emsdk pins/goldens, layer-local tiling/Type3 ownership and prior upstream merges; retain them rather than replacing the fork pointer with raw canonical upstream.

## Merge and compatibility risks

The upstream/private path intersection since merge base is only `.github/workflows/release.yml`, runtime gitlink and `pnpm-lock.yaml`; runtime changed-path intersection is empty. This is a path-level risk inventory, **not** a simulated merge or a claim of conflict-free compatibility. Runtime PDFium image/font internals can affect fork-layer semantics without touching the same file.

An ancestry-preserving merge must retain private script-print policy/tests (`5462de7` / `253bca6`), gesture-removal regression (`7a7c94f`) and active-search-hit identity assertions (`ca6d809`), as identified by the plugin worker. Do not replace canonical endpoint files over private files. Carry upstream CloudPDF tenant-feature/module-manifest/day-usage/licensing/Postgres idle-error/migration/mocks code as TypeScript; this does not create a new Rust CloudPDF port. Its boundary-specific upstream tests still need integration coverage.

## Review focus

1. Absent/null/value distinction survives DTO/schema/prediction/native dictionary and readback, especially color, dash and rich-text comments.
2. Kind conversion preserves identity and authority; refused conversion changes nothing and batch/transaction rollback remains coherent.
3. Runtime fork confinement/layer ownership stays intact across native PDFium/DEPS changes; no old library is linked under new header metadata.
4. New ink/fallback rendering matches the pinned TS oracle with the same font bytes/order/corpus, with no silent tolerance or stale font answer.
5. Checkout or fixture changes cannot contaminate acceptance; owned stable sources, matching manifests, generated fixtures and pre/post guards are mandatory.

## Proposed dependency-ordered work

### Task 1: Pin the runtime decision and prepare the reviewed fork merge

**Owns:** runtime fork merge from 3c55 into exact option A/B; `public/epdf_annot.h`, native annotation/font/image changes, DEPS/.gn, retained `scripts/embedpdf-runtime/{target-args.sh,tests}`. **Produces:** a new fork commit, full export inventory, matching Linux/WASM libraries and immutable build provenance for later tasks.

- [ ] In a new owned runtime worktree, first retain the fork-target/emsdk/goldens and layer-seam tests as regression acceptance; add any missing comment-default regression fixture for chosen option B.
- [ ] Merge the chosen immutable runtime SHA, inspect actual conflicts and preserve fork-only behavior; do not alter prior tags/artifacts or user refs.
- [ ] Run target-args tests, relevant native embedder tests (kind/no-color/ink/rich-text/fonts/linearized layers) and existing held-layer regression against the **new** library; compare exports for +2/0 removed/0 signature changes.
- [ ] Build initial Linux ARM64 and WASM with source/build-tool provenance and cold DEPS sync where required. Other consumer targets follow only from the same accepted source. Commit/independently review this runtime result before dependent acceptance.

Estimate: **3–6 engineering hours**, **2–6 machine hours** for initial cached/cold target builds; other architectures/Apple may add **4–10 machine hours**. Missing native hosts are explicit remaining gates, not emulated passes.

### Task 2: Merge pinned viewer main once and preserve private additions

**Owns:** new integration worktree from f28ed; canonical4ad merge, gitlink to Task1's new fork commit, lockfile/workflow conflict resolutions. **Produces:** one integrated TypeScript oracle and preserved private tests for both branch names.

- [ ] Record and independently compare the existing print-policy, gesture and search-identity tests/gates before merge; merge exact4ad, then confirm their surviving behavior alongside upstream changes.
- [ ] Resolve actual runtime gitlink/lockfile/release-workflow conflicts without substituting raw upstream runtime or adding unrelated CI policy changes.
- [ ] Build/typecheck affected TS packages and run focused new upstream tests (siblings/comment/dash/no-stroke/standard fallback/XYZ), plus inherited CloudPDF tests appropriate to its changed boundaries. Regenerate/check API contracts through existing build tooling where required.
- [ ] Run engine vector generators against this exact merged TS + Task1 runtime into owned outputs; retain each stale/throw/incompatible case as the port worklist instead of restoring older vectors to make Rust green.

Estimate: **1–3 engineering hours**, **20–60 machine minutes** for initial merge/oracle checks, excluding native builds. No text-conflict count is treated as a parity estimate.

### Task 3: Port the concrete engine/core semantic gaps

**Owns:** Rust files named in the functional table plus engine conformance scenarios/vectors. **Produces:** exact final target semantics through existing wire operations and raw/native calls; no duplicated facade business rules.

- [ ] First add failing invariant cases for sibling/non-sibling/held-field conversion; omitted/null stroke; dash `[]` refusal versus `[0]` acceptance/default `[3,3]`; markup contents/richText agreement and null/source behavior; final XYZ zero/null/short arrays. Use target TS tests as oracle inputs, not commit subjects.
- [ ] Implement at the existing owners: pdfium wrapper exposes the two actual C signatures; write prepare/patch/create/update carry the new fields/ordering; rich-text comment helpers share the existing DTO/parser; actions reader uses raw view params. Keep lifecycle, authorization and mutation ownership at their current layers.
- [ ] Replay affected engine suites, including `annotation-update:check`, `annotation-create:check`, `annotation-write:check`, `annotation:check`, `annotation-appearance:check`, `actions:check`, `fonts:check`, `schema:check`, `wire:check`, `operations:check`; enumerate all `:check` scripts from the merged package rather than freezing the old count.
- [ ] Run corresponding Rust binaries `annotation_update`, `annotation_create`, `annotation_write`, `annotation_reader`, `write_transactions`, `fonts_wire_corpus`, `document_fonts_wire`, `font_registry`, plus the actual action corpus; then workspace tests/clippy/changed-file formatting against matching Task1 native library. A positive mutation/control must show the new invariant tests fail when its guard is removed.

Estimate: **4–8 engineering hours**, **30–90 machine minutes** after usable native/TS inputs. Geometry/font rendering work overlaps Task4 ownership and should have one owner per algorithm.

### Task 4: Integrate the separate plugin/binding plan without duplicating policy

**Consumes:** Task2 oracle + Task3 engine payload semantics; plugin worker's exact inventory. **Produces:** snapshot, caret/annotation/ink/font/stage updates and truthful C#/Swift/Kotlin/WASM contracts at each existing native boundary.

- [ ] Integrate the plugin worker's dependency-ordered scope (snapshot lifecycle/render rights, caret/selection style, kind/ink/font transitions and stage page turns/pager); keep core geometry/prediction and engine mutation policies in one owner.
- [ ] Generate/replay affected plugin vectors and add missing behavioral scenarios for new functionality, including refusal/disposal/font-staleness paths; do not count a default-valued state field as feature coverage.
- [ ] Regenerate/typecheck/bind the existing adapters with truthful native capability exposure. Keep the .NET facade's current read/render/helper scope distinct from the full plugin viewer; repackage/revalidate the already-supported Linux ARM64 local consumer with new native inputs, without claiming new RIDs or publishing NuGet.
- [ ] Independently review the whole integrated tree and reconcile every inventory row to an implementation+test, an inherited shared implementation+test, or an explicit accepted deferral. Do not advance a branch while mandatory target functionality remains only in a worklist.

Estimate: FINAL plugin inventory gives **29–52 engineering hours** for P1–P5, excluding lead core/runtime. This supersedes the provisional allocation; do not add it twice. Native queues are listed there. No implementation or new platform gate ran in C7 planning.

### Task 5: Release/pin and close both branches at the fixed target

**Consumes:** accepted full source/tree/runtime from Tasks1–4. **Produces:** matching saved-byte runtime payloads, new Apple pin if shipping that consumer, reproducible local .NET package, reviewed final branch SHAs and immutable handoff.

- [ ] Compute content-addressed payload/Apple hashes from the **finished** link graph; build matching required WASM/Linux/Apple runtime slices (and configured supported Android/native consumers under their existing contracts), then inspect ABI/symbols/manifests/font bytes/notices and archive relocation. Never overwrite old publications.
- [ ] Save/review exact release bytes/tag targets and complete publication/pin only under the execution authorization/policy applicable then. New NuGet publication remains a separately named deliverable; a local nupkg must not be described as a NuGet release.
- [ ] Run stable-source default-consumer acceptance with overrides unset, provision canonical generated fixtures, guard exact source/manifests/artifact association, and record real pass/skip/fail counts. Run the real packed-viewer/.NET/native raster gate with same supplied controlled fonts; external deployment fonts and unvalidated RIDs stay explicit limitations.
- [ ] After independent final review, ordinarily advance both private branch refs to the same reviewed closure commit and verify fresh remote refs/ancestry, or execute the explicitly selected layered alternative. Preserve resources/evidence until their scoped cleanup is authorized.

Estimate: **1–3 engineering hours**, **2–6 machine hours** after builds; fresh native targets or uncached DEPS can extend machine time. Combined planning envelope is approximately **40–75 engineering hours**, using core/runtime/merge8–17, plugin/native/package29–52 and distinct release-close1–3 as component estimates rounded for coordination/review uncertainty. Shared P1 geometry and P5 acceptance are not added again. Allow **6–18 machine hours** plus unavailable native/device queues, with cache/platform availability and discovered oracle drift the main uncertainty. These are proposals, not measured runtimes or promised completion dates.

## Definition of done at the pinned target

Both private branches contain canonical viewer4ad in ancestry and the accepted full Rust/plugin/binding closure; the runtime gitlink identifies a reviewed fork merge containing3c55 and the declared A/B target, preserving fork-only guarantees. Every functional inventory row has a concrete oracle/regression and matching implementation; inherited TS changes and preserved private policies pass their relevant checks. No stale vector shims, feature defaults, source overlays or local runtime overrides substitute for default-consumer proof. Fresh runtime/Apple releases and pins, if part of the agreed execution closure, match saved bytes and exact target/source/link graph; the .NET result is named local package unless actual publication was separately completed. Unsupported native platforms/external deployment fonts remain labeled unvalidated. Final exact remote SHAs, commands/exits/hashes, source guards, counts/skips and remaining limitations are sealed.

“Latest” means frozen4ad and declared runtime A/B for this job. Further upstream movement becomes a later scope, not a reason to silently stretch this plan.

## Retained evidence and current status

`pinned-targets.json`, `inventory-read-result.json`, `core-detail-read-result.json`, `export-and-rust-result.json`, `rust-owning-snippets.json` and `dash-schema-proof.json` record exact read commands/exits/source values. Exploratory missing-path and no-match results are retained and corrected in confirmed owner paths; they are not compile/test failures. No trial merge was run. The user checkout remained HEADf28ed and tracked-clean.

Own bare metadata was exported to HOST `viewer-metadata.git` and `runtime-metadata.git`; `metadata-export-manifest.json` hashes their files. They preserve fetched new objects/refs but use alternates to retained HOST source `.git/objects`; preserve those original repositories. Plugin/binding worker may still need the combined container; root releases that shared reservation after all readers finish. This worker needs no further source reads after sealing.

Next action (under two minutes): review the runtime A/B choice and one-shared-integration branch strategy alongside the separate plugin/binding inventory before authorizing implementation.
