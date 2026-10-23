# Plugin vector diffs after the upstream catch-up

Task 12c compares committed pre-generation vectors at `b029e3d78dfcd3477f73265b978f52c74b5fb25c` with the current generated files. The oracle is the current shipping TypeScript, based on upstream `b456ee4fd`; reviewed Task 12 generator migrations reach it directly. Historical files explain retired contracts but are not the runtime oracle. No new archive, digest inventory or Rust implementation was added.

## Totals and verification

The old tree had 110 suites plus two metadata files. All 110 suites changed, two suites are new, no files were removed or renamed, and both metadata files are unchanged. The new tree has 112 suites and 114 JSON files. Cases changed from **15,356 to 15,164 (−192)**. Case names are not unique occurrence IDs, so a set comparison of names is not a count of preserved/retired behavior.

The largest net reductions are core annotation **7,713→7,505 (−208)** and core UI **1,299→1,238 (−61)**: current shape/record/update APIs replace retired helpers, and typed layout editing replaces removed global chrome transforms. Kernel cases move **180→174**. Other families net **+83**, including new reconciliation/lifecycle and current contract cases. These are changes across the reviewed source migrations, not losses introduced by running generation. Partial source-contract retirements remain material: retaining an old case name does not prove its old input contract survives.

`pnpm --filter @embedpdf/tooling-plugin-conformance run vectors` completed with exit 0. The existing `vectors:check` rendered all suites again with **zero stale vectors, zero orphaned files and no ICU drift**. It exits 1 solely because no Rust source names the two new suites:

| New file | Cases | Rust owner |
|---|---:|---|
| `plugins/annotation-reconciliation.json` | 32 | Task 13.17, `epdf-plugin-annotation` |
| `plugins/form-core-lifecycle.json` | 3 | Task 13.18, `epdf-plugin-form` |

Thus byte determinism passed; complete Rust replay coverage has not passed. Existing Rust names the other 110 suites but `plugin-conformance` still expects schema 1. Task 13.2 advances the harness; each feature owner then proves replay. `coverage.json` is unchanged pre-migration coverage, not a new coverage measurement. Task 13.2 owns its eventual refresh, integrated by 13.33. `environment.json` remains the recorded ICU/Unicode contract.

## Shared input, output and runtime changes

Every suite envelope advances **schemaVersion 1→2**. Scenario traces now expose `stateChanges` and `documentEvents`, current state-less/stateful plugin construction, current promise settlement, callbacks, reports and subscriber state. `tooling/plugin-conformance/src/{vectors,scenario,fake-engine,probe,canonical}.ts` and `packages/core/main/src/{kernel,context,state,mirror,page-mirror,changes,clock,errors}.ts` are the reference. Task 13.1 owns product semantics; 13.2 owns the reconstruction/comparator and the three kernel suites.

Page and annotation refs use `objectNumber`/`baseIndex` and current page refs, rectangles/points are page space, quads have named corners, annotation fields are flat, and forms include complete fields plus widget rows. Inputs author real `doc.apply` operations, resources, authority, reserved numbers, complete results/meta and session-bound facts with opId/origin.tx **before** mutation settlement. Old aliases, reconstructed reducers and compatibility bridges are not contracts to port. Native host compatibility conversion removal is owned by 13.33 after callers migrate.

Replay must resolve dotted getters when invoked and bind the final receiver, decode explicit `$textLayout`, preserve canonical special values and member order, use the real 16ms host clock with close cancellation, and reproduce scoped case entropy without exporting fixture plumbing as product behavior. Exact ordering and diagnostic channels matter: a fulfilled action refusal, a rejected promise, a reported listener exception and an unhandled host rejection are different outcomes.

Nine suite files differ only by schemaVersion (parsed content and byte comparison after removing that field): `core/annotation-endings.json`, `core/annotation-richtext.json`, `core/geometry-basics.json`, `core/signature-parse.json`, `core/stage-anchor.json`, `core/stage-camera.json`, `core/stage-layout.json`, `plugins/render-policy.json`, `plugins/render-tiles.json`. All other existing suites have further changes. Stable values still need the current schema and replay support.

## Family contracts and ownership

The table is a complete per-file inventory: paths are relative to `tooling/plugin-conformance/vectors/`; counts are old→new. Each suite has exactly one owner in the expanded plan. TS references name the current implementation, while suite `describes` and imports identify the individual functions.

### plugin-conformance — Task 13.2

Reference: `tooling/plugin-conformance/src/{vectors,scenario,fake-engine,probe,canonical,case-entropy}.ts`.

Advance SCHEMA_VERSION 1→2 and reconstruct stateChanges/documentEvents, per-session fake handles, current DTOs and origin, current factory registries, dotted getter resolution at invocation with bound receiver, $textLayout hydration, callable callback specs, deferred labels and promise turn accounting. Use real 16ms HostClock frames with lifetime cancellation and canonical number/undefined/byte/map semantics. Preserve shared per-case UUID entropy and descriptor cleanup without porting loader scaffolding as product API. Own environment.json and the coverage.json metadata refresh after all owners land.

| File | Cases |
|---|---:|
| `kernel/lifecycle.json` | 98→94 |
| `kernel/turns.json` | 12→12 |
| `kernel/utils.json` | 70→68 |

### epdf-geometry — Task 13.3

Reference: `packages/core/geometry/src/index.ts; packages/engine/core/src/geometry/{convert,pageSpace}.ts`.

Port page-space coordinates, current PageRef and named quad corners, transform/conversion inputs and current geometry helpers; remove retired helper dispatches.

| File | Cases |
|---|---:|
| `core/geometry-affine.json` | 110→110 |
| `core/geometry-basics.json` | 140→140 |
| `core/geometry-page.json` | 122→118 |
| `core/geometry-text-quad.json` | 280→280 |

### epdf-stage-core — Task 13.6

Reference: `packages/core/stage/src/index.ts`.

Port current camera/layout/anchor inputs and page-space geometry, retaining input member order where it is contractual.

| File | Cases |
|---|---:|
| `core/stage-anchor.json` | 32→32 |
| `core/stage-camera.json` | 165→165 |
| `core/stage-layout.json` | 103→103 |

### epdf-annotation-core — Task 13.7

Reference: `packages/core/annotation/src/{types,measurement}.ts; packages/core/annotation/src/record/{shape,style}.ts; packages/core/annotation/src/update/; packages/core/annotation/src/shapes/; packages/engine/core/src/measure/`.

Port current full model/session/Shape and record/draft/patch/value DTOs, named quads, styles, canonical IDs, link defaults, scene and direct update messages. Use current shared engine measurement labels instead of the older RL formatter. H3 is stable reserved identities plus synchronous forget cleanup; do not resurrect temp-id reconciliation APIs.

| File | Cases |
|---|---:|
| `core/annotation-anchor.json` | 582→582 |
| `core/annotation-cloudy.json` | 369→354 |
| `core/annotation-endings.json` | 88→88 |
| `core/annotation-flags.json` | 650→650 |
| `core/annotation-geometry-basics.json` | 338→294 |
| `core/annotation-geometry-hit.json` | 248→248 |
| `core/annotation-geometry-shapes.json` | 3340→3279 |
| `core/annotation-geometry-text.json` | 97→59 |
| `core/annotation-groups.json` | 15→15 |
| `core/annotation-hit.json` | 17→17 |
| `core/annotation-kinds.json` | 137→109 |
| `core/annotation-measurement-model.json` | 40→39 |
| `core/annotation-measurement.json` | 505→496 |
| `core/annotation-placement.json` | 194→194 |
| `core/annotation-props.json` | 533→532 |
| `core/annotation-richtext.json` | 114→114 |
| `core/annotation-scene.json` | 276→276 |
| `core/annotation-snap.json` | 17→17 |
| `core/annotation-update.json` | 123→112 |
| `core/annotation-view.json` | 30→30 |

### epdf-acrojs — Task 13.8

Reference: `packages/core/acrojs/src/{host,prelude,index}.ts; packages/core/js-sandbox/src/`.

Port current worlds/Identity/field value-entry unions and full FormSnapshot widgets/calculation order; boot/run/wait/throw transaction arrays, seedFrom environment, output and UTF-16/UTF-8 budgets, boot latch, disposal and rebuilding. Preserve the observed admitted transaction creating a second sandbox after the pending first factory was disposed.

| File | Cases |
|---|---:|
| `core/acrojs-host.json` | 17→20 |
| `core/acrojs.json` | 75→75 |

### epdf-signature-core — Task 13.9

Reference: `packages/core/signature/src/index.ts and its parsing, trust, verify and document helpers`.

Port current signer/document request shapes and typed byte/certificate/verification results, preserving the parsing and trust semantics.

| File | Cases |
|---|---:|
| `core/signature-document.json` | 35→35 |
| `core/signature-parse.json` | 204→204 |
| `core/signature-signers.json` | 28→28 |
| `core/signature-trust.json` | 96→96 |
| `core/signature-verify.json` | 97→97 |

### epdf-ui-core — Task 13.10

Reference: `packages/core/ui/src/{layout,solver,projection,shortcuts,index}.ts`.

Replace removed global chrome/bundle transforms with current typed layout validation, immutable add/replace, scoped remove, edit chains, identity and warnings. Keep solver/projection/shortcut semantics.

| File | Cases |
|---|---:|
| `core/ui-helpers.json` | 11→3 |
| `core/ui-projection.json` | 28→28 |
| `core/ui-schema.json` | 61→48 |
| `core/ui-shortcuts.json` | 843→843 |
| `core/ui-solver.json` | 303→303 |
| `core/ui-transforms.json` | 53→13 |

### epdf-plugin-interaction — Task 13.11

Reference: `packages/plugin/interaction/src/{controller,model,interaction.plugin}.ts`.

Port create/connect, current tool registration, session transitions, page-space input routing and lifetime-owned lending/disposal.

| File | Cases |
|---|---:|
| `plugins/interaction-functions.json` | 19→19 |
| `plugins/interaction.json` | 27→27 |

### epdf-plugin-actions — Task 13.12

Reference: `packages/plugin/actions/src/{controller,settings}.ts; packages/plugin/actions/src/{lifecycle/document-events,scripting/executor}.ts; packages/plugin/actions/src/{dispatch,submit}/`.

Port current PdfActionTree/SubmitFormPayload, typed destinations, document/annotation/widget triggers, settings policy rows, current adapter/executor/sink contracts and commit/submit receipts. Preserve deferred queue ordering, exact diagnostics/refusals and script/hover budgets. M14: test script print under blocking policy against the actual executor and non-user-origin lifecycle guard; adopt stricter upstream rule only where it subsumes M14.

| File | Cases |
|---|---:|
| `plugins/actions-dispatcher.json` | 30→30 |
| `plugins/actions-hover.json` | 10→10 |
| `plugins/actions-lifecycle.json` | 27→27 |
| `plugins/actions-script.json` | 32→32 |
| `plugins/actions-submit.json` | 10→10 |
| `plugins/actions-triggers.json` | 15→15 |
| `plugins/actions.json` | 36→36 |

### epdf-plugin-stage — Task 13.14

Reference: `packages/plugin/stage/src/{controller,model}.ts and destination/motion/feed helpers`.

Port document views/settings, page-space XYZ destinations, camera feeds and host-clock motion/lifetime cancellation.

| File | Cases |
|---|---:|
| `plugins/stage-capability.json` | 54→54 |
| `plugins/stage-destination.json` | 29→29 |
| `plugins/stage-events.json` | 3→3 |
| `plugins/stage-feeds.json` | 10→10 |
| `plugins/stage-motion.json` | 15→15 |
| `plugins/stage-pure.json` | 87→87 |

### epdf-plugin-selection — Task 13.15

Reference: `packages/plugin/selection/src/{controller,model}.ts and geometry/handles helpers; packages/engine/core/src/pageSpace/text.ts`.

Port explicit page-space text layouts/$textLayout marker, current selection cells and named quads. Replace the crate’s old text layout copy with the shared-edge tie-break semantics already ported in engine text layout.

| File | Cases |
|---|---:|
| `plugins/selection-geometry.json` | 290→288 |
| `plugins/selection-handles.json` | 43→43 |
| `plugins/selection.json` | 34→34 |

### epdf-plugin-render — Task 13.16

Reference: `packages/plugin/render/src/{controller,model,appearance-store,raster-store}.ts and policy/tile helpers`.

Port policy service, layer read rights, current annotation/field ledgers and fact payload paths, current invalidation transitions and tile work cancelled on close. Remove retired paint-version/reducer APIs.

| File | Cases |
|---|---:|
| `plugins/render-capability.json` | 39→39 |
| `plugins/render-ledger.json` | 33→32 |
| `plugins/render-policy.json` | 180→180 |
| `plugins/render-strategy.json` | 475→475 |
| `plugins/render-tile-plane.json` | 43→43 |
| `plugins/render-tiles.json` | 701→701 |

### epdf-plugin-annotation — Task 13.17

Reference: `packages/plugin/annotation/src/{controller,model}.ts; packages/plugin/annotation/src/services/store.ts; packages/plugin/annotation/src/sync/{records,confirmed}.ts; packages/engine/core/src/annotation/relationships.ts`.

Port flat hydration, complete records, stable objectNumber/baseIndex annotationKey identity and comments relation keys/sort/thread classification, current nested nouns and atomic apply with resources. Delete thread replies/collateral records from the view on confirmation; do not keep post-delete stale replies or rename inline refs. Port all pending CRUD/grouping/markup/hover/tools and writable JS target lowering.

| File | Cases |
|---|---:|
| `plugins/annotation-api.json` | 19→19 |
| `plugins/annotation-comments.json` | 16→16 |
| `plugins/annotation-crud.json` | 13→13 |
| `plugins/annotation-grouping.json` | 5→5 |
| `plugins/annotation-hover.json` | 3→3 |
| `plugins/annotation-hydration.json` | 17→18 |
| `plugins/annotation-markup.json` | 9→9 |
| `plugins/annotation-pure.json` | 155→153 |
| `plugins/annotation-reconciliation.json` (new) | 0→32 |
| `plugins/annotation-repository.json` | 695→690 |
| `plugins/annotation-tools.json` | 33→34 |

### epdf-plugin-form — Task 13.18

Reference: `packages/plugin/form/src/{controller,model}.ts; packages/plugin/form/src/sync/{fields,predict}.ts; packages/plugin/form/src/scripting/controller.ts; packages/plugin/form/src/tools/handlers.ts`.

Port FieldIndex, full snapshots/widgets joins and current field authority, flat flags, values/projection/prediction/submission, native queue actions and addWidget in place of attachWidget. Keep empty session state and mirror-owned status, exact own-field forms.valueSet facts/tx before settlement, repair/bundle/refMap results and whole reset semantics. Reconstruct actual K/V/C/F controller phases/budgets and native sink staging before later refusal.

| File | Cases |
|---|---:|
| `plugins/form-actions.json` | 13→13 |
| `plugins/form-core-lifecycle.json` (new) | 0→3 |
| `plugins/form-core.json` | 101→99 |
| `plugins/form-design.json` | 29→29 |
| `plugins/form-fill.json` | 31→31 |
| `plugins/form-scripting.json` | 46→46 |

### epdf-plugin-page-edit — Task 13.19

Reference: `packages/plugin/page-edit/src/{controller,model}.ts and operations`.

Port current page refs, neighbor ListPosition mutations, named-page/layout facts and history-backed edits; remove old page event names and index-position conversions.

| File | Cases |
|---|---:|
| `plugins/page-edit.json` | 19→19 |

### epdf-plugin-search — Task 13.20

Reference: `packages/plugin/search/src/{controller,model}.ts and session/scan helpers`.

Port current SearchSlice including pagesSearched, page-space bounds/snippets, permissions, latest-session cancellation, stage reveal and named state transitions.

| File | Cases |
|---|---:|
| `plugins/search-find-all.json` | 10→10 |
| `plugins/search-reducer.json` | 22→21 |
| `plugins/search-stage.json` | 6→6 |
| `plugins/search.json` | 39→39 |

### epdf-plugin-link — Task 13.21

Reference: `packages/plugin/link/src/{controller,model}.ts and resolution helpers`.

Port live annotation/actions capabilities, obj:n/base:page:n keys, page-space bounds/destinations, flat visibility and complete URI/group reply action trees. Preserve current cache/pending cancellation contracts.

| File | Cases |
|---|---:|
| `plugins/link-functions.json` | 9→8 |
| `plugins/link.json` | 20→29 |

### epdf-plugin-measurement — Task 13.22

Reference: `packages/plugin/measurement/src/{controller,model}.ts; packages/plugin/measurement/src/{sync/viewports,write/scale}.ts`.

Port full PdfMeasure/PageScale/local viewports and mirror cache, required page.measure listViewports/setScale with pages.scaleSet/refetch; typed apply/resources/opId/tx and current engine labels for measured shapes/caption-only updates.

| File | Cases |
|---|---:|
| `plugins/measurement-functions.json` | 42→45 |
| `plugins/measurement.json` | 19→19 |

### epdf-plugin-redaction — Task 13.23

Reference: `packages/plugin/redaction/src/{controller,model}.ts and createStore/pending-read helpers`.

Port current marking/session transitions, guarded annotation.list({pages}) projection, source-owned pending/collateral policy, typed search/selection and named page-space quads. Use current atomic applied results and batch failures, not retired Failed/Skipped wire status variants.

| File | Cases |
|---|---:|
| `plugins/redaction-functions.json` | 28→32 |
| `plugins/redaction.json` | 22→25 |

### epdf-plugin-stamp — Task 13.24

Reference: `packages/plugin/stamp/src/{controller,model}.ts and library/assets/placement helpers`.

Port canonical/template/materialized bytes, resource ownership, typed LocalEngine services, full metadata/PieceInfo merge/null deletion, key-sorted named-page registries and surviving size/ref layouts. Port per-library queues/cache/persistence, reserved annotation refs/apply confirmations, script environment/budgets and current placement; remove obsolete flatten page-status checks.

| File | Cases |
|---|---:|
| `plugins/stamp-api.json` | 21→21 |
| `plugins/stamp-assets.json` | 20→20 |
| `plugins/stamp-functions.json` | 19→20 |
| `plugins/stamp-library.json` | 21→23 |
| `plugins/stamp-placement.json` | 19→22 |

### epdf-plugin-signature — Task 13.25

Reference: `packages/plugin/signature/src/{controller,model}.ts and signing/stamp helpers`.

Port signing keys/services/authority args, current form/widget DTOs, nested stamp APIs and preparation/completion/cancellation facts. Use current connected-controller model transitions instead of removed capability-only constructor.

| File | Cases |
|---|---:|
| `plugins/signature-functions.json` | 19→14 |
| `plugins/signature.json` | 39→39 |

### epdf-plugin-metadata — Task 13.26

Reference: `packages/plugin/metadata/src/{controller,model}.ts and mirror sync helpers`.

Port lazy separate standard/custom mirrors, ensureLoaded joining both and explicit per-mirror refresh, ISO createdAt/modifiedAt/trapped, current permission errors and confirmed events before settlement.

| File | Cases |
|---|---:|
| `plugins/metadata.json` | 14→17 |

### epdf-plugin-i18n — Task 13.27

Reference: `packages/plugin/i18n/src/{controller,model}.ts and locale/translation helpers`.

Port workspace settings, script-aware negotiation, loading identity dedup, superseded waiters, late pack registration without switching and stable translation references.

| File | Cases |
|---|---:|
| `plugins/i18n/core.json` | 79→86 |
| `plugins/i18n/plugin.json` | 24→24 |
| `plugins/i18n/plurals.json` | 532→532 |

### epdf-plugin-shell — Task 13.28

Reference: `packages/plugin/shell/src/{controller,model}.ts`.

Port named transitions, snapshot restore (without public id/focusRequest; restore initializes focusRequest 0), current surface/menu events and scoped lifecycle.

| File | Cases |
|---|---:|
| `plugins/shell/plugin.json` | 6→7 |
| `plugins/shell/reducer.json` | 32→40 |

### epdf-plugin-commands — Task 13.29

Reference: `packages/plugin/commands/src/{controller,model}.ts and definition/run helpers`.

Port stateless settings-backed definitions, runtime shadowing, identity-aware disposers, live derivations, scoped capability/context callbacks and {commandId,refusal} shortcuts.

| File | Cases |
|---|---:|
| `plugins/commands/functions.json` | 52→54 |
| `plugins/commands/plugin.json` | 24→25 |

### epdf-plugin-view-manager — Task 13.30

Reference: `packages/plugin/view-manager/src/{controller,model}.ts`.

Port generated pane IDs/createPane(options.documentIds), positional movePane, split/focus/remove with live DocumentsToken and memoized PaneInfo.

| File | Cases |
|---|---:|
| `plugins/view-manager/plugin.json` | 12→13 |
| `plugins/view-manager/reducer.json` | 64→82 |

## Negative outcomes and remaining source limits

A successful renderer means its inputs executed and required fake calls matched; it does not make every recorded exception intended product behavior. The reviewed suites preserve these material negatives:

| Contract | Current negative evidence | Owner |
|---|---|---|
| Pure validation | Singular affine/page transforms; invalid calibration/base unit; duplicate UI units and unknown modifiers; incomplete action trees; compose member collision; malformed/unsupported CMS; render lattice/policy arguments | 13.3, 13.10, 13.8, 13.2, 13.9, 13.16, 13.22 |
| Annotation records/tools | Missing/null ref TypeErrors, eight inherited-key kind lookup TypeErrors, invalid tool definitions and twelve distinct missing-icon errors. These are authored adversarial inputs, not broad proof arbitrary DTOs are supported. | 13.7, 13.17 |
| Lifecycle/write refusals | Current permission gates and instance-closed/cancellation errors, whole-change engine refusal, queued failures and listener reports; only actual per-page batch APIs return mixed failed/skipped/applied outcomes. | Every owning plugin; fake ordering in 13.2 |
| Stage malformed/empty input | Current TS throws on empty paged camera, NaN grid and unreadable page rows. Existing safe Rust stage divergences stay. | 13.6, 13.14 |
| Form-only palette placement | Current handler can return before awaiting created; engine refusal can be unhandled with no form console diagnostic. Current coverage uses successful form-only placement plus explicitly handled/awaited refusal paths. Do not claim the unhandled edge is repaired. | 13.18; Task 14 decision |
| Default QuickJS | Scripted sandbox and explicit validation-none/lazy setup cases do not exercise omitted-factory default VM execution. Under tsx the serialized prelude can fail on `__name`. No substitute factory or cleaned prelude was used to declare it passing. | 13.8 and consuming form/stamp tasks; Task 14 decision |
| Admitted sandbox transaction after disposal | Current host may build a second sandbox after disposal of the pending first factory; exact run/dispose ordering is preserved, not generalized into cancellation. | 13.8; Task 14 decision |
| Print | Checked action-executor/M14 behavior and form-pipeline lifecycle print are distinct. The latter remains a source gap, not a generator or M14 success claim. | 13.12, 13.18; Task 14 decision |
| Defensive absent services | Required page.measure, document.redaction and local stamp services are supplied by current typed fixtures. Engine Unsupported/refusal replacements do not execute malformed absent-service fallback branches. Dormant redaction page.text defaults are not active text coverage. | 13.22, 13.23, 13.24 |

Pure `outcome.threw` counts are listed below to make expected negatives reviewable; scenario rejections/reports are separate channels and are not included in this count.

| File | Recorded throws |
|---|---:|
| `core/acrojs.json` | 4 |
| `core/annotation-kinds.json` | 8 |
| `core/geometry-affine.json` | 3 |
| `core/geometry-page.json` | 1 |
| `core/signature-parse.json` | 39 |
| `core/stage-anchor.json` | 7 |
| `core/stage-layout.json` | 5 |
| `core/ui-schema.json` | 9 |
| `core/ui-shortcuts.json` | 8 |
| `kernel/utils.json` | 2 |
| `plugins/annotation-pure.json` | 22 |
| `plugins/annotation-repository.json` | 2 |
| `plugins/measurement-functions.json` | 8 |
| `plugins/render-policy.json` | 31 |

## Carry routing and new coverage

This routes material carry work; earlier engine-task and compiler-cleanup notes already superseded by approved later work are not resurrected as additional archive tasks.

| Carry | Single owning task (consumers noted where necessary) |
|---|---|
| Current lifecycle, settings, mirrors, pending queue, per-session facts, authority and error codes | 13.1; replay mechanics 13.2 |
| H3 stable identity, create-before-ack, synchronous disappearance/forget, callback reentry | 13.17 (core update semantics supplied by 13.7) |
| Shared-edge text tie-break and named page-space selection quads | 13.15 |
| BaseIndex comment relation keys/sort, flat hydration, atomic threads and collateral replies | 13.17 |
| Full form snapshot/widgets/flat flags, attachWidget→addWidget, whole reset, native action ordering and transfer results | 13.18 |
| Current scale cache, engine label formatter, measured geometry and caption updates | 13.22 (shared annotation label support 13.7) |
| Redaction current scopes/results, pending/collateral policy and removed Failed/Skipped wire statuses | 13.23 |
| Stamp atomic flatten results, bytes/PieceInfo/named registry and post-close work | 13.24 |
| Rejected-password shim; refs/revision-loss rewrites; CancelToken::never listing; raw flags synthesis; form status/draft conversions; export labels/stamped imports; quad/page-space and event-name translations; authority fallback | 13.33 removes native stopgaps after feature callers migrate; no deleting them early |
| Signature signing authority/stamp fallback/confirmed events | 13.25 |
| Remaining commands/i18n/shell/view-manager current transition contracts | 13.29 / 13.27 / 13.28 / 13.30 respectively |
| Member order, explicit callable fixtures, entropy, source factory registrations and coverage metadata | 13.2 |

Five packages have no generated suite yet. Their concrete tasks own both new generators and Rust replays: **13.4 core/fonts** (`core/fonts.json`), **13.5 core/form** (`core/form-painter.json`), **13.13 plugin/history** (`plugins/history.json`), **13.31 plugin/outline** (`plugins/outline.json`), and **13.32 plugin/attachment** (`plugins/attachment.json`). Attachment's optional annotation/stage dependencies are ported before it. These new suites are not part of the 112-suite totals.

The native host task is **13.33**. Every feature task re-checks kept divergences against `docs/research/rust-plugins-branch-review.md`, Status. In particular retain H1/H2, H4, M15, M20, M23, M25–M27, N6 and R5/R7 protections where applicable; do not turn current TS exception cases into native panics. The Task 10 H3 ruling supersedes the old temp-id prescription. Engine no-op/event-envelope choices remain as already ruled.

Task 14 must state whether existing `historical/task-12b*/` directories stay or are replaced by a short committed hash list before merging into plugins. They are untouched here. No new historical preservation framework is required.
