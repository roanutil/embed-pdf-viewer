# Catch-up execution ledger

Plan: docs/research/upstream-catchup/2026-11-06/scope-and-plan.md

## Authorization and inputs

User authorized implementation after planning commit4289d3f86, allowed subagents, requires version-controlled progress and permits pushes only to their fork. Viewer target8297bad8d388eb9c70324c22cb001c59248f95f4 and runtime4346c5131cd009c4086fdce13bde7fd43fbd4ce0 remain fixed. Public baseline refs remain impl5a2f25f06 and plugins8fa01cc1e until final acceptance.

## Current jobs

| Job | State | Evidence / next operation |
|---|---|---|
| 1 Runtime | Complete for initial Linux ARM64 and WASM inputs | Final fork 1e214deb2 contains fixed upstream 4346c5131 and preserved fork histories. Native 498 passed / 6 skipped; layer gate has zero unexpected failures. Other hosts and TSAN remain platform gates. |
| 2 Viewer integration | Exact upstream source merged | Merge bdebee29c contains fixed viewer 8297bad8 and both starting histories. Matching fork runtime gitlink is f8bd8110a. Full product acceptance remains open. |
| 3 Engine and shared contracts | Initial ports verified; broader migration in progress | Clean text/charmap, XYZ, attachment records, LF/CR, stroke/dash, error vocabulary and ABI fixes committed. See latest checkpoint below. |
| 4 Plugins and bindings | Pending | Remaining contracts, generated bindings and all existing platform package consumers must be updated together. |
| 5 Artifacts and both refs | Pending | Final combined Rust WASM payload, packaged consumers, platform matrix and both research-ref acceptance required. No release publication authorized. |

## Preflight interfaces and rulings

1. Job1→2/3: runtime headers/symbols/build identity must match the fork. Ruling: source integration and dependency provisioning may overlap runtime builds; no dependent behavior acceptance until matching runtime exists. Cost if wrong: duplicated source work, caught by final graph checks.
2. Job2→3/4: merged TS schemas define final keys/tags/nulls and generated vector outputs. Ruling: preserve generated baseline evidence and regenerate with explicit reviewed diffs, never restore stale fixtures to turn Rust green. Cost if wrong: missed feature cases; paired replay/coverage gates catch them.
3. Job3→4/5: engine DTO vocabulary is shared with router/native bindings. One owner changes each vocabulary; platform packages follow the accepted records and artifacts.
4. Ruling: use manual Git worktrees inside Docker because native app worktree tools operate on the host checkout; retain a versioned ledger here per user request instead of relying on disposable skill scratch. The five plan headings remain Jobs rather than rewriting them for skill script parsing.
5. Platform/parity requirements were reaffirmed by the user: see PARITY-AND-PLATFORMS.md. Both are completion gates, not optional follow-up work.

## Stop/resume

Inspect this ledger, git status, git log and agent worktrees before repeating a task. Logs go under .superpowers/sdd/upstream-catchup-2026-11-06; summaries, source identities, tests/results and decisions are committed here. Preserve running builds and all prior artifacts. Do not reset/clean a worktree to resume.

## Source and first parity checkpoint

- Viewer mergebdebee29c contains exact8297bad8 and both starting histories; conventions conflict retained private lifecycle/isolation rules plus upstreamAPI-evolution row. Runtime gitlink196331edea7566e3a77a8f338822b052d339d345 is public in the runtime fork and contains exact4346 plus all preserved fork work. This is source integration, not full acceptance.
- Runtime targetargs/emsdk/CAPI-listing checks pass; native release compile is active with-j2 after successfulDEPS sync. Exact source/args/compiler provenance is in runtime-release-inputs.json under the persistent build logs. Runtime source ledger lives in its dedicated branch.
- Merged pnpm frozen-lockfile install succeeded. Before setup, plugin-boundary check could not load TypeScript. After setup it exposed22factory-type import failures. Ruling: consumer native bridges may import only plugin factory types without loading implementation; regular types still require contract entries. Focused RED→GREEN and full boundary suite8/8; repository boundary check passes. The Rust frame host keeps its own scheduler type, because it is a native host seam rather than an upstream stage option.
- Canonical43 Linux and WASM archives downloaded with checksum checks as provisional oracle inputs. NAPI addon compiled; canonical inputs do not prove the final retained-fork build. New content-addressed fork WASM payload is unpublished, so build locally; no pin downgrade or fake published hash.
- LinuxLP64 typedef resolution misclassified explicituint64_t/int64_t as32-bit numbers. Added executable generator regression, observed number-versus-bigint RED, preserved fixed-width names before resolution, GREEN1/1. Generic unsignedlong/PDF flags remain numbers. This fix is required to regenerate matching platform bindings safely.
- Text commit8f7054241ed6f22521befe75be989604f9a720c3 expands12TS cases and adds normalization/reader wiring. Provisionalcanonical43 charmap30/30passes; native reader hook mutation RED→GREEN. text_corpus4pass/1fail: page_text_agrees_with_the_typescript_engine, embedded-orientations still expects rawCRLF69units versus cleaned65. Existing page-text vectors were not transformed to hide the mismatch; full targetTS regeneration remains pending.
- XYZ has eight deterministic fixtures and semantic RED→GREEN at raw reader; TS32entrypoint vector replay waits for engine worker bootstrap. No full Job3 completion claim.

- Independent review caught that capitalized interface names endingPlugin could bypass the new type-factory guard. Added negative test, observed RED0-versus1violation, narrowed to lowercamel factory export names, final boundary+ABI suites10/10pass. Preserved per-instance Rust scheduler option as well as global host scheduler; the private host interface replaces an absent TS exportedScheduler type without removing existing Rust-only behavior.
- Merged TypeScript engine-core suite1080/1080passes (85files), engine and inlineworkers build successfully. Canonical TS-only WASM linked in /workspaces/catchup-oracle-canonical43;1056/1056boundfunctions matchABI. The provisional package symlink has a canonical-only payload-source marker; it must be replaced by matching retained-fork payload before finaldefault-consumer acceptance.
- XYZ commitcccf2da64be7403d3f7283d0f958ef7561396ce0 has8PDFs/32entrypointcomparisons, identical native+WASM TSvectors and fullRust action_reader15/15pass, all provisionalcanonical43. Native zeronormalization mutation control failed as expected and source restored.

## Checkpoint after first code ports

Commits: viewermergebdebee29c, cleantext8f7054241, XYZcccf2da64, factorybridgedd9fd4a2b and ABI0e3b1e870. None is a fullcatch-up acceptance claim. Runtime source196331ede is available in the user runtimefork; release native compilation is active. Both original publicresearchrefs remain at their recordedbaselines until finalacceptance.

Runtime provisional suite:5passed/1skipped; skipped wasm above2GiB test reports missing fixture/dist prerequisites. TypeScript engine-core1080/1080, boundary+ABI10/10, repository boundarycheckpass; native/WASM XYZ oracle parity and Rustaction15/15pass. Fullpage-text regeneration is underway to resolve named staleCRLF vectors. Exact retained-fork native/WASM reruns remain mandatory.

Committed execution-state.json and execution-evidence/ capture source identities, resource/log paths, tests and remaining gates. Original verification.json describes planning-only proof at its timestamp; it does not certify these implementation tasks. SHA256SUMS is refreshed when this execution checkpoint is finalized.

## Verified retained-fork checkpoint (2026-11-06)

This section supersedes the provisional build/test status above; those entries retain the decision history. Source checkpoint 0a6fa59e23368886d2623cebac10283b0759942e, runtime fork 1e214deb2869d9233e573666d239bc32ce73f186. Machine-readable results: [checkpoint receipt](execution-evidence/checkpoint-results.json), [runtime acceptance](execution-evidence/runtime-final-acceptance-result.json) and [runtime artifacts](execution-evidence/runtime-final-artifacts.json).

1. Runtime: native release and WASM static archives are built, checksummed and repackaged against final source 1e214deb2. Final source differs from prior build only in reviewed layer registry/docs; GN/Ninja no-op and library byte equivalence verified. Native unit/embedder total 498 passed, 6 skipped (five unavailable external encrypted fixtures; one opt-in Acrobat output). Layer gate: 361 passed, one opt-in skip, three unchanged expected failures, zero unexpected failures. Nine checker tests and 49 saved PDFs passed independent validation without recovery.
2. Final matched viewer checks: Rust engine library 509/509 plus 85 selected integrations = 594 passed. Boundary/ABI tests 10/10, attachment coverage guard 3/3, runtime suite 6/6 with zero skips including actual above-2GiB read/render/write. All 13 required checkpoint command groups pass; broader tooling typecheck still reports ten contract migration diagnostics. Earlier TS engine-core suite passed 1080 tests across 85 files.
3. Actual TS oracle vectors were regenerated and checked for cleaned page text/charmaps, attachments (sizeBytes/md5), XYZ, public LF/native PDF CR, and no-stroke/dash scenarios. New PDF/scenario coverage includes joined hyphens, eight XYZ fixtures, 22 stroke/dash cases, and mixed-line-break rich-text agreement. Independent review found contents normalization occurred after agreement checks; commit 8766530d0 fixes shared/create/patch normalization with ten passing regression tests. Three new generator runtime-type inference errors were fixed in 0a6fa59e2; ten broader migration diagnostics remain.
4. ABI generation preserves uint64_t/int64_t and size_t/ptrdiff_t before platform typedef erasure; native Node bindings and matching TS-only WASM oracle were rebuilt, with 1056/1056 exported functions matching. Exact final archives are extracted under /workspaces/catchup-final-runtime-artifacts. Use its runtime-build.local.json via PDF_RUNTIME_BUILD_FILE for local builds. Tracked release/local configuration is unchanged. WASM lib symlink points to /workspaces/catchup-oracle-fork1e214; this is the TS oracle, not the final combined Rust browser payload.
5. All pushes target user forks only. Runtime result is public on catchup/2026-11-06-runtime. Viewer checkpoint is on research/catchup-2026-11-06. User viewer fork Actions are globally disabled and have no repository secrets; no workflow dispatch, release publication or upstream write occurred. Original research refs stay at their recorded baselines until full acceptance.

## Remaining acceptance work

| Owner/job | Required next work | Current evidence/limitation |
|---|---|---|
| Engine/contracts, Job3 | Subtype conversion, markup RC/body defaults, ink geometry, font display and remaining DTO/cache/tolerant-read changes | Broader annotation tests expose old ink bbox, missing richText, absent-C expectations and attachment keys. Do not relax tolerances or restore stale expectations. |
| Oracle tooling, Job3 | Migrate removed mutation changed field, FormField.family, scope PdfBits, security advisory and SignatureDTO APIs; regenerate affected real TS vectors and replay Rust | Ten typecheck diagnostics; focused generators above pass. |
| Plugins/bindings, Job4 | Complete P1–P5; regenerate WASM/UniFFI/C metadata and update all exposed existing Swift/Kotlin/.NET/browser package surfaces | Platform matrix in PARITY-AND-PLATFORMS.md is mandatory. |
| Artifacts/consumers, Job5 | Build final combined Rust WASM, current native packages and supported consumers; record host/device gaps and immutable graph checksums | Initial runtime inputs are ready. Apple hosts, Android NDK and dotnet are unavailable in this Linux image; these checks remain incomplete. |
| Both research refs, Job5 | Final reviewed acceptance, merge correct result into each requested fork branch, push and verify ancestry | Neither research ref has been advanced for catch-up. Preserve distinct branch scope and all private behavior. |

Resume with Job3 contract rows and their TS-oracle generators. Check existing commits and receipts first; do not rebuild completed runtime inputs or repeat the accepted ports. Whole catch-up is incomplete.

## Job 3 checkpoint: conformance typecheck migration (2026-11-06)

The ten generator typecheck diagnostics are fixed on the TypeScript side. `pnpm run typecheck` in `tooling/engine-conformance` now exits 0. Vectors were regenerated with `pnpm run <group>` for annotation-create, annotation-delete, annotation-move, annotation-update, form-write, scope, security and signature-protection.

1. `AnnotationListMutationMeta.changed` became `changedRefs` (upstream wire schema). Generators and the Rust annotation_create replay follow it. Annotation `appearance.changed` is a boolean and keeps its name.
2. `FormField.family` became `kind`. The form-write generator reads `field.kind`; values are unchanged.
3. `PdfBits`, `decodePdfBits` and `pdf-bits` became `PdfPermissionFlags` and `permissionFlagsOf` (upstream a780243ba). Scope vectors carry named flags.
4. The advisory section of the security vectors was removed. Upstream removed `advisoryFromPdfBits` in a780243ba, so no advisory is emitted. The Rust advisory code in `crates/epdf-engine/src/features/security/state.rs` is now a Rust-only divergence to remove.
5. `SignatureDTO` became `Signature`. The signature fixture field `contentsSize` became `contentsSizeBytes`.

Semantic check against the committed vectors, after mapping the renamed keys: `signature/protection.json` has zero differences. The annotation and form vectors also carry upstream DTO changes, not only renames: `richText: null` is now present on created annotations, and authority grants carry `permissionFlags` in place of `pdfBits`.

Rust replay status after this checkpoint (`cargo test --test <name>` in `crates/epdf-engine`), counted as passed/failed:

| Replay | Result | Cause to fix |
|---|---|---|
| annotation_create | 20 / 2 | Read DTO lacks `richText` (null). A line read sent back as a create is refused with `line has no field richText`. |
| annotation_update | 8 / 25 | Not yet investigated past the rename. |
| scope_conformance | 2 / 3 | `PdfBits` to `PdfPermissionFlags` in `auth/scope`. |
| security_state | 0 / 8 | Advisory removed upstream; `bits` replaced by flags on the state. |
| form_write | 3 / 4 | `family` to `kind` on the field DTO. |
| security_scope | 23 / 0 | Passes, although it still names `pdfBits` in grants. Recheck after the flag migration. |
| form_apply | 4 / 0 | Passes. |

The Rust production code is not changed in this checkpoint. Only `crates/epdf-engine/tests/annotation_create.rs` changed, to read `changedRefs`. The failures above are the exact remaining Job 3 work. Do not repair them by restoring the old vector keys.

Remaining in this checkpoint: the Rust migrations above, the other Job 3 contract rows, and the full replay of every regenerated group. Vector checksums in SHA256SUMS are not refreshed here.

## Job 3 checkpoint: markup comment rich text (2026-11-06)

`annotation_create` replay: 22 passed, 0 failed (was 20 passed, 2 failed).

1. Every markup read (`highlight` through `file-attachment`, including `square`, `circle`, `polygon` and `polyline`) now carries `richText`, `null` when the comment has no `/RC`. Reads follow `readCommentRichText`: only an `/RC` source counts, and an absent body family reads as `Helvetica`.
2. Create and patch write a markup comment as `/RC` after the kind's fields (`applyComment`). A body in the default family is written without a family. A `null` on patch removes `/RC`. Free text keeps its own rich text.
3. A `line` reads `captionEnabled: false` when `/Cap` is absent (`readLine`). The replay expectation that required `null` now matches the oracle. Polygon and polyline keep `null`.
4. The action tree no longer serializes `warningFlags`, because `ActionModelReader` returns `{ root, incomplete, warnings }`. The Rust field stays internal for warning conversion.
5. An empty `nm` counts as given, so it is neither stamped nor written. The truthiness check in `writeAnnotationBase` decides this. Before this change the create stamped a UUIDv7 over it.

Full `epdf-engine` library tests: 509 passed. Replays still open: annotation_update 8 passed / 25 failed, scope_conformance 2 / 3, security_state 0 / 8, form_write 3 / 4. security_scope and form_apply pass.

## Job 3 checkpoint: forms kind rename and vector review (2026-11-06)

Full `epdf-engine` suite: 1483 passed, 16 failed. The starting commit `2b86156d3` had 13 failing tests, so 9 failures are new in this tree. Five tests that failed at the start now pass. The library suite passes 509/509.

Vector decisions (user approved items 1, 3; item 2 investigated):

1. `forms/snapshot.json` and `forms/apply.json` move `family` to `kind`. This follows the TypeScript draft, patch and field DTOs (`kind` tag, `FormFieldDraftBase`).
2. The widget `fieldKind`, `fieldFamily` and `hasMark` fields are legitimate. They come from `c4b286a69` ("Tag form fields by kind", API consistency item 10), which is already in the starting commit. The committed snapshot vector predates that change and was never regenerated. The Rust widget rows do not yet emit these fields, so `the_snapshot_agrees_with_the_typescript_engine` stays red until they do.
3. `annotation/appearances.json`, `annotation/read.json`, `fonts/wire.json` and `actions/submit-flags.json` are regenerated, as approved.
4. `forms/field.json` was red because the generator still sent `family:` drafts and patches. The TypeScript create path reads `draft.kind`; a missing kind gives `FAMILY_CODE[undefined]`, so native creation fails with `cannot create field ... (name conflict or invalid)`. The generator now sends `kind`. The TypeScript behavior was already correct.
5. The submit-flags replay no longer reads `flags.raw`. The TypeScript output dropped it; the case-level input `raw` remains.

Still open:
- `files/write.json` and `wire/requests.json` are reverted and have not been decided.
- Rust widget DTO port for `fieldKind`, `fieldFamily`, `hasMark`.
- The remaining failures listed above (signature, write transactions, annotation transfer, document events, fonts corpus, outline, protection).

## Checkpoint: widget kind, record order, golden hashes (2026-11-06)

Full `epdf-engine` suite: 1498 passed, 1 failed. The starting commit had 13 failures. The one remaining failure is `the_font_kinds_answer_what_the_typescript_did`, described below.

1. Widget rows emit `fieldKind` and `hasMark` (`joinWidgetFieldNumbers`). `hasMark` is true for a signature field that is signed or has a mark drawn in it (`EPDFForm_GetFieldValueKind` non-zero) and an appearance-fit record. The Rust fixtures move to the TypeScript names.
2. The markup field spaces include `richText` on every markup kind, matching `markupBaseFields`.
3. An annotation record serializes `richText` before the `subtype` tag, as the TypeScript readers build it. Without this the export text differs in key order, which changes the hash. `Map::shift_remove` keeps the remaining key order.
4. The golden export digests in `annotation_transfer.rs` now match the oracle text. `612f68c7…` and `370f5e29…` are the digests in `runAnnotationExportConformance.ts`, which passes on the oracle. The previous Rust goldens were stale. Verified byte-for-byte against the oracle text.
5. Attachment normalization in the transfer tests uses `sizeBytes` and `md5`.
6. Action trees built in Rust no longer include `warningFlags`.

Open, not resolved:

- `fonts/wire.json`, the approved regeneration, answers `NotFound` at `document.fonts.program` for a display hash that `document.fonts.list` reports one step earlier. The case's covers text says the program is read from the document. Results are deterministic across three regenerations. `generate-font-vectors.ts`, `services/src/features/fonts/` and the TypeScript source are unchanged since the starting commit. The likely cause is the oracle environment, not a TypeScript change. This regeneration was approved on the premise of a TypeScript change, so it needs your decision: revert it and have Rust match the covers text, or keep the `NotFound` answer and treat it as a TypeScript bug.

## Font vector reverted (2026-11-06)

`fonts/wire.json` is back at its committed version (`2b86156d3`). The approved regeneration answered `NotFound` at `document.fonts.program` for a display hash that `document.fonts.list` reports one step earlier. The case's covers text says that program is read from the document. The TypeScript source and generator are unchanged since the starting commit, and three regenerations gave the same answer. The cause is probably the oracle's runtime environment, not a TypeScript change, so the regeneration is not kept.

Open: find why the oracle answers `NotFound` in this environment. Until then, `fonts_wire_corpus` matches the committed vector and passes, and Rust is not changed for this case.

## Plugin conformance tooling: typecheck migration, in progress (2026-11-06)

`tooling/plugin-conformance` typecheck: 464 errors at the start of this step, 340 now. The tooling targets the pre-merge engine API, so this is the catch-up work for plugin contracts, not a starting-commit failure.

Done in this step, all mechanical and each on the lines the compiler flagged:

- Object keys renamed to the current names: `fieldFamily`→`fieldKind`, `size`→`sizeBytes`, `family`→`kind`, `changed`→`changedRefs`, `checksum`→`md5`, `bits`→`flags`.
- Trigger objects: `scope`→`type`, and `name`→`event` where the trigger carries an event (current `ActionTrigger` shape).
- Imports and their usages renamed to the names the compiler suggests (`FormFieldDTO`→`FormField`, `SignatureDTO`→`Signature`, `selectionHandleGeom`→`selectionHandleGeometry`, and similar).
- `labels: []` added to page-list literals that lack it (7 literals).

Still open, grouped by cause:

1. Event `layout` removed from `DocumentEvent`. The `pages.*` event results no longer carry the page layout. About 23 literals and the `layout` branch in `src/fake-engine.ts` depend on it. Decide how the fake derives its page state after a change (re-read from its own table, or apply the operation). This is a fixture-model decision in tooling, not a reference change.
2. `EventOrigin` fields: `sub`→`subject`, `ts`→`at` (ISO 8601), `serverId` removed, `serverSequence` added. About 14 fixture literals.
3. `fake-engine.ts` service member tables: new required members on `pages` (`copy`) and `fonts` (`list`, `resolve`, `getProgram`, `downloadProgram`). The table claims implemented members, so each needs an implementation or an explicit decision.
4. Permission bits: `decodePdfBits` and `permissions.bits` are gone; use `permissionFlagsOf` and `permissions.flags`.
5. Remaining argument, property and assignment mismatches (about 250), to be worked file by file.

Vector checks (`vectors:check`, `coverage:check`) fail at module load on the same API drift, so they cannot run until the typecheck is clean.

## Plugin conformance tooling: progress and approval needed (2026-11-06)

Typecheck: 119 errors, from 464 at the start of this step. The fake engine typechecks clean.

Finding: the tooling was already stale at the starting commit. `595fa2bd5` removed `cssFontFamilyForFont`, `cssFontFamilyForFace` and `familyKey` from the packages before `2b86156d3`, and the tooling still imported them. `vectors:check` could not have run at the starting commit either.

Done in this step: confirmed renames (model `…State`→`…Model`, `removePane`→`deletePane`, `listAllLinks`→`readAllLinks`, `readAssetBytes`→`getAssetBytes`, `dispose`→`destroy`, `tx`→`transaction`, `warningFlags` removed from action trees); origin and event shapes updated; `fake-engine.ts` moved to permission flags, the event `layout` read, and the current member tables; cases for the removed CSS font helpers dropped from `annotation-pure.ts` (the helpers no longer exist in the packages).

Needs your approval (a TypeScript change, or a loss of test coverage):

1. Tooling imports symbols that exist in the packages but are no longer exported: `familyKey` and `interpolate` (rich-text and i18n), `layoutFontOf`, `layoutVariableText`, `VariableTextOptions` and `layout/breaks` (form), `FontProgram` (fonts), `PageGeom` (stage), `CommandDef` (commands). Options:
   a. Export them from their packages. This adds to the TypeScript public surface, so it needs your approval.
   b. Drop the tooling cases that use them. Coverage for those functions is lost until an equivalent is written.
   I recommend (a) for the internal helpers the cases need, and (b) for any whose function no longer exists at all.

Still open (about 100 errors, to be worked file by file): event-model fixtures that build `layout` on `pages.*` events, the `goToPage` trigger `page` field (now `pageIndex`), permission flags in callers that still pass bits, and assorted settings and argument renames. Vector regeneration waits until typecheck is clean, and then each changed vector needs the same review as before.

## Export decisions applied (2026-11-06)

Approved: export the symbols the plugin-conformance tooling imports. Applied as re-exports, with no change to implementation: `familyKey` (plugin/annotation rich-text), `interpolate` (plugin/i18n translate), `FontProgram` (core/fonts index). Remaining missing names are not defined anywhere in the packages: `layoutFontOf`, `layoutVariableText`, `VariableTextOptions`, `layout/breaks` (form), `PageGeom` (stage), `CommandDef` (commands). These need a decision: drop the tooling cases that use them, or name the replacement.

## Replacements for moved symbols (2026-11-06)

Repointed without changing the packages: `PageGeom` to `PageGeometry` (core/stage), `CommandDef` to `CommandDefinition` (plugin/commands contract), and the break rule `needsBreak` to its current module `core/text/src/breaks`. Still unresolved: `layoutFontOf`, `layoutVariableText` and `VariableTextOptions`. The closest current function is `textEditorLayout` in `core/form/src/editor.ts`, which takes a widget and field and returns a different layout. Those cases need rewriting, not repointing.

## Plugin conformance generator: where it stops (2026-11-06)

Typecheck: 85 errors. The generator now gets through the core and early plugin suites, and stops at the first scenario that depends on a changed contract.

Changes in this step, each traced to a current source name:

- Field discriminant `family` to `kind` in fixtures, including the form-field reads that were stale.
- Kernel `status()` to `getStatus()`.
- Selection `set` to `select`.
- `obbFromGeom` to `obbFromGeometry` (exported from `core/annotation`).
- Form painter: the three `layoutFontOf` / `layoutVariableText` wrappers were removed and their cases rewritten against `textEditorLayout` (the approved rewrite). Each former option maps to the box, `textAlign`, `multiline`, `maxLength`, comb, or `password`. The offset of a plate and the width probes of a font have no counterpart and are not carried over. Expected values change and need the vector review.

Stops at, in order of the scenario that fails:

1. `annotation-crud` `restyle-writes-and-rolls-back`: passes after the `select` rename.
2. `form-scripting` `the-time-budget` and `the-time-budget-shrinks` (also `a-fault-rebuilds-the-realm`): expect a realm rebuild behind a slow factory, through `sandbox.builds` and `settle('rebuild')`. The sandbox factory contract used by those fixtures must be checked against the current actions realm (`packages/plugin/actions/src/scripting/realm.ts`). Not yet investigated.

Vectors are not regenerated in this step. Each regenerated group needs the same semantic review as before.

## Plugin conformance generator runs end to end (2026-11-06)

The generator now completes (exit 0) with the current API. Typecheck: 82 errors. Fixes in this step: sandbox members `destroyed` and `destroy()`, with runtime faults recorded as destroying the realm; the stamp import option `kind` renamed to `assetKind`; `fieldKind` on widget rows.

Vectors are NOT committed. The regenerated output is kept at /tmp/regen-vectors in the container. Review shows 47 changed files; after mapping the confirmed key renames, about 52,000 leaf paths still differ. They are state-shape changes: `revision` to `pagesSequence`, `rotationThreshold` to `rotationThresholdDeg`, `cursor` to `turnGestures`, `action.warningFlags` removed, and others. The TypeScript renames behind them are in the starting commit, and the committed vectors were written after those commits yet still contain the old names. Those committed vectors were therefore not reproducible from the current TypeScript. Decision needed: accept the regenerated vectors as the oracle for the current TypeScript, or keep the committed vectors and treat the generator as the thing to fix.

## Plugin vectors regenerated (2026-11-06)

Accepted by the user: the committed plugin vectors are replaced by the output of the generator against the merged TypeScript at HEAD. `vectors:check` passes: 117 files current. Evidence for the decision: the committed vectors were written on a line of history (`df0dcfd04`) that predates the naming renames (PR F, `2ce3f7bc5`), and the generator could not run at that commit (`font-css` removed in `595fa2bd5`). The renames reached this branch through a merge.

Residual review: after mapping confirmed renames (`family`→`kind`, `fieldFamily`→`fieldKind`, `size`→`sizeBytes`, `checksum`→`md5`, `changed`→`changedRefs`, `tx`→`transaction`, `sub`→`subject`, `ts`→`at`, `serverId`→`serverSequence`, `bits`→`flags`), about 52,000 leaf paths across 47 files still differ. Verified by sampling, not per path: state-shape renames in the TypeScript (`revision`→`pagesSequence`, `rotationThreshold`→`rotationThresholdDeg`, `cursor`→`turnGestures`) and removal of `action.warningFlags`. Not yet traced: the case-count changes (`len` differences in `outline`, `interaction`, `link`, `actions-script`, `acrojs-host`) and the `type` changes in the stage and i18n vectors. Those are the first things to check.

## Vector verification, step 2 (2026-11-06)

Tooling bug fixed: three engine-facing meta literals still wrote `changed`, while the TypeScript reads `changedRefs` (outline suite `meta()`, the shared annotation `confirmation()` meta, and one signature suite meta). The outline title regression came from this bug. With the fix, the outline title counts match the committed vector exactly.

Explained by TypeScript commits after the vector line (df0dcfd04 line):
- Renames: field and key names listed earlier, `removePane`→`deletePane`, `selectionHandleGeom`→`selectionHandleGeometry`, and the origin fields.
- i18n locale (`ar`→`en` at the first read): `58435c49a` (fallback loading) makes a locale not ready while the fallback loads.
- Stage camera (search-stage, stage-capability): `1ed34c6fe` (turn pages past the edge) and `f45ac2990` (spreads step as one) change page and camera behavior.

Not yet explained: ui-solver and ui-schema lengths and overflow flags (24 values); signature messages and codes (7); form-scripting (4); render-tile-plane (3); stamp-library (3); view-manager (17, besides the method rename); page-edit (12); signature-verify (2); ui-projection (3); stage-anchor (2); selection (1); i18n CHG beyond the locale (checked by the same commit).

Vectors are not committed in this step. The regenerated set is in the working tree of the repo; the committed vectors remain at 71730f101.
