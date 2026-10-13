# Swift Wrappers for the Remaining Plugins: Umbrella Plan

> **For agentic workers:** This plan coordinates sixteen per-plugin plans in this directory. Run Tasks 0a to 0f once, then execute each plugin plan with superpowers:subagent-driven-development (recommended) or superpowers:executing-plans, in its own worktree, in the order the Waves section gives. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give every plugin crate that `2026-10-01-swift-package.md` left unexported a typed UniFFI handle, a `ViewerHost` accessor and a Swift capability in `platforms/apple/EmbedPDF`, one plugin per plan.

**Architecture:** Nothing new. Every plan repeats the recipe in `docs/superpowers/plans/2026-10-01-swift-package.md` Appendix A §A.0, with metadata (Task 6), search (Task 7) and render (Task 8) as the worked examples, and the code they produced (`crates/plugins/{metadata,search,render}/src/ffi.rs`, `platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/*.swift`) as the reference. This umbrella fixes what the plans share: order, branches and worktrees, names that cross plan boundaries, ownership of shared files, and the definition of done. It also builds, once, every piece more than one plan needs (Tasks 0a to 0f), so no plugin plan creates a shared piece another plugin plan also creates.

**Tech Stack:** Rust 2024, `uniffi =0.32.0`, Swift 6.2 tools, Swift Testing, `swift-custom-dump` 1.3, Xcode 27.0, iPhone 17 simulator (iOS 26.0).

**Spec:** `docs/superpowers/plans/2026-10-01-swift-package.md` (its Global Constraints, Decisions, Appendix A and Appendix C). That plan argues from `multiplatform-plugins-plan.md`, `multiplatform-plan-claude-remix.md` §8 and `multiplatform-plan-claude.md:370-435`; executors read those too.

## Global Constraints

Every constraint in `2026-10-01-swift-package.md` § Global Constraints applies unchanged. The ones plugin plans break most often:

- `uniffi` is pinned at `=0.32.0`.
- UniFFI type names are unique across the workspace. All generated Swift compiles into one module, `EmbedPDFBindings`. Prefix generic names with the plugin (`StageSettings`, not `Settings`).
- Types shared by more than one plugin live in `epdf-ffi` and nowhere else. See "Shared records" below for who adds which.
- No `usize`, `isize`, `Rc` or `serde_json::Value` across the boundary. Mirror them in the plugin's `ffi.rs` with `From` impls.
- Error fields are `detail`, teardown methods are `shutdown`. Swift keywords (`move`, `delete`, `default`, `extension`) aren't method names: `move_pages`, `delete_pages`.
- Facades don't decide PDF semantics. Swift renames and reshapes; behaviour stays in Rust.
- Text offsets are UTF-16 code units.
- The kernel is `!Send`. Every kernel touch goes through `KernelBound` or a kernel job.
- Swift value assertions use `expectNoDifference`. Test targets don't link what the target under test already provides.
- No npm package changes, so no changesets.

## The plans

| # | Plan | Scope | Requires (kernel) | Optional (kernel) | Extra work beyond the recipe |
|---|---|---|---|---|---|
| 01 | `01-interaction.md` | D, plus `feedback` (W) | none | none | `InteractionHandler` and `PlatformFeedback` foreign traits; `dispatch_pointer` round-trip benchmark at 120 Hz |
| 02 | `02-stage.md` | D | none | actions | render's viewport members (`render_policy`, `render_source`, `source_key`, `conform_viewport`, `paint_settings`, `create_view_demand`); gesture `fire` methods; a SwiftUI scrolling reader in `EmbedPDFUI` |
| 03 | `03-selection.md` | D | interaction | feedback | none |
| 04 | `04-link.md` | D | interaction | stage, annotation, actions | the first annotation-core records in `epdf-ffi/src/annotation.rs`; the `page.annotations.list` route with `nest_refs`/`flatten_refs` |
| 05 | `05-annotation.md` | D | interaction | selection, actions | `page.annotations.create`/`update`/`delete` and `annotations.listRawAll` engine paths; extends plan 04's `annotation.rs`; split into sub-handles (98 of 140 callable members) |
| 06 | `06-form.md` | D | interaction | annotation, actions | `forms.*` engine paths; reuses plan 04's `nest_refs`/`flatten_refs`; owns the actions cross-plugin tests (submit, JavaScriptCore) |
| 07 | `07-redaction.md` | D | annotation | selection, search | `redaction.apply` engine path |
| 08 | `08-measurement.md` | D | annotation, interaction | none | `page.measure.setScale` and `page.measure.viewports` in `epdf-engine` and `translate.rs` |
| 09 | `09-stamp.md` | W | none | annotation, actions | `AssetEngine`: a second `NativeEngine` for stamp assets; `pages.setName`/`removeName` in `epdf-engine`; `StampLibraryStore` foreign trait |
| 10 | `10-page-edit.md` | D | none | none | `pages.move`, `pages.rotate`, `pages.delete`, `pages.insert`, `pages.insertBlank`, `pages.extract`; every record new (state is `Value` in Rust) |
| 11 | `11-view-manager.md` | W | documents | none | none |
| 12 | `12-shell.md` | D | none | none | decides whether shell ships to Swift at all (Task 1 of that plan) |
| 13 | `13-i18n.md` | W | none | none | `Loader` foreign trait reading locale JSON from a bundle |
| 14 | `14-commands.md` | W | none | i18n, shell | `CommandRunner` foreign trait |
| 15 | `15-actions.md` | D | none | none | `actions.read` engine path; JavaScriptCore `ScriptEngine`; foreign traits for `register_executor`, `set_submit_handler`, `set_ui_adapter` and `run_document_verb` only (the 8 host members serve sibling Rust plugins and don't cross) |
| 16 | `16-signature.md` | D | form | annotation, interaction, stamp | `signatures.*` in `epdf-engine`; Keychain `SignatureSigner`; bundled-anchor `SignatureTrust`; `NativeSecurity` that enforces document permissions |

Requires and optional come from each crate's `src/lib.rs` (`.requires(...)`, `.optional(...)`). The kernel sorts plugins itself (`crates/epdf-kernel/src/plan.rs`), so `Session::build` push order doesn't matter. It does reject a config that enables a plugin without what it requires, so a plan can't ship its host step before its required plugins' host steps exist.

## Waves

A plan can start once every plan it requires has merged into the integration branch. Optional dependencies don't gate: a plan exercises an optional integration only if that plugin has already merged, and otherwise leaves a test for the later plan to add (the later plan owns it; see each plan's "Cross-plugin tests" section).

- Wave A, after Task 0: 01 interaction, 10 page-edit, 11 view-manager, 12 shell, 13 i18n, 15 actions.
- Wave B: 02 stage and 03 selection after 01. 04 link after 02, because link's resolution carries stage's `StageRevealOptions`. 05 annotation after 04, because it extends 04's `annotation.rs` and `page.annotations.list` route. 14 commands after 12 and 13.
- Wave C, after 05: 06 form, 07 redaction, 08 measurement. 09 stamp after 05 and 10: its `create_asset_from_annotations` test uses annotations, and its asset engine needs 10's `pages.*` routes. Form has no kernel requirement on annotation, but its engine-path task reuses 04's ref rewrites and 05's annotation plane.
- Wave D, after 06: 16 signature.

Run at most three plan worktrees at once. Each worktree builds its own `target/` for three targets (darwin, device, simulator), and the shared-file merges below get harder with more branches in flight.

## Branches and worktrees

All plugin work happens in worktrees. The main checkout stays on whatever it's on.

The integration branch is `research/swift-plugins`, cut from `research/multi-platform-rust-core-impl-plugins`. Each plan runs on `research/swift-<plugin>` in `.claude/worktrees/swift-<plugin>`, cut from the integration branch's tip at the time the plan starts, and merges back with `git merge --no-ff`, run from the integration worktree (`.claude/worktrees/swift-plugins-integration`). `git switch research/swift-plugins` fails in a plan worktree while the integration worktree has that branch checked out. Use superpowers:using-git-worktrees to create each one.

```bash
# once, from the repository root
git branch research/swift-plugins research/multi-platform-rust-core-impl-plugins
# per plan, e.g. 03 selection
git worktree add .claude/worktrees/swift-selection -b research/swift-selection research/swift-plugins
```

The generated inputs of `platforms/apple/EmbedPDF` (`EmbedPDFFFI.xcframework`, `Sources/EmbedPDFBindings`) are ignored, so every worktree runs `bash platforms/apple/build-xcframework.sh` itself before its Swift tests. PDFium for `darwin-arm64`, `ios-arm64` and `ios-sim-arm64` must be fetched in each worktree with `bash packages/engine/runtime/scripts/fetch-libpdfium.sh <slice>`, which reads the slices Task 0b built from `engine-runtime-build.local.json`.

## Shared files

Every plan edits these. Edits are additive, one block per plugin, in plan-number order, so a merge conflict is always two adjacent additions. Resolve it by keeping both blocks in plan-number order. Never reorder or reformat another plugin's block.

| File | What a plan adds |
|---|---|
| `crates/epdf-viewer-host/Cargo.toml` | `epdf-plugin-<name> = { path = "../plugins/<name>", default-features = false, features = ["uniffi"] }`, alphabetical |
| `crates/epdf-viewer-host/src/api.rs` | one `ViewerConfig` field with a doc comment and a `#[uniffi(default = ...)]` |
| `crates/epdf-viewer-host/src/session.rs` | one `if` block in `Session::build` |
| `crates/epdf-viewer-host/src/lib.rs` | one `use` and one async accessor in the `#[uniffi::export] impl ViewerHost` block |
| `crates/epdf-viewer-host/src/translate.rs` | the engine paths the plan owns (Appendix C of the spec): `route` rows, arms added to `translate`'s `extra` match (never a replacement of the match, which stays keyed on `kind`; use a guard on `path` when two paths share a kind), and `decode` arms, each with a test in `tests/translate.rs`. Plugins send JavaScript numbers (`90.0`) and `$undefined` args: use Task 0e's `whole` and `optional_whole`, and rely on its filter that drops `null` and `$undefined` request fields. The first plan that sends a payload (10, else 05) adds `let mut payloads = Vec::new();` above the match. `Decode` variant names are taken: `Annotations`, `AnnotationMutation` (04, 05), `Forms`, `FormMutation`, `ExportBytes` (06), `PageMutation`, `HostBytes` (10), `InlineBytes`, `ResultWithBytes` (16) |
| `crates/epdf-viewer-host/src/translate.rs` `PATHS` | every routed kernel path, one per line, both spellings when two paths share a route. Task 0e adds the list, the namespace rule in `maps` and `every_routed_path_is_listed` |
| `crates/epdf-viewer-host/src/translate.rs` `FLAGS` | a feature name a plugin checks with `doc.has` that's neither a path nor a namespace of one (16 adds `document.versioned`) |
| `crates/pdfium-sys/wrapper.h` | one `#include` per fork header the plan binds (08 `epdf_measure.h`, 09 `epdf_named_pages.h`, 16 `epdf_signature.h`, `epdf_checkpoint.h`, `epdf_digest.h`). Task 0b makes sure the slices ship them |
| `crates/epdf-engine/tests/common/mod.rs` | helpers the plan's engine tests share (08, 09, 16) |
| `crates/test-support/src/lib.rs` | one fixture constant, when a Rust test reads a fixture more than once |
| `platforms/apple/build-xcframework.sh` | one line per fixture in `FIXTURES` (Task 0f), repo-relative, globs allowed. `Tests/*/Fixtures/` is ignored, so a file copied there by hand is never committed |
| `platforms/apple/EmbedPDF/Sources/EmbedPDF/PDFViewer.swift` | a plugin accessor lives in the plugin's own file as an `extension PDFViewer`; edit `PDFViewer.swift` itself only to add viewer-wide state (16 adds `setSigner(_:)` and `setTrust(_:)`) |
| `.github/workflows/plugin-rust.yml` | one line in the `uniffi` test list (Task 0 makes it a list) |
| `platforms/apple/README.md` | one row in the plugin table (Task 0 adds the table) |

## Cross-plan names

A later plan consumes these by exact name. Rust paths are `epdf_plugin_<crate>::ffi::<Name>`. Swift names are in module `EmbedPDF`.

| Plan | Rust handle | `ViewerConfig` field | `ViewerHost` accessor | Swift capability | `PDFViewer` accessor |
|---|---|---|---|---|---|
| 01 | `InteractionHandle`, `FeedbackHandle` | `interaction: bool`, `feedback: Option<FeedbackConfig>` | `interaction(document_id)`, `feedback()` | `InteractionCapability`, `FeedbackCapability` | `interaction(for:)`, `feedback()` |
| 02 | `StageHandle` | `stage: Option<StageConfig>` | `stage(document_id)` | `StageCapability` | `stage(for:)` |
| 03 | `SelectionHandle` | `selection: bool` | `selection(document_id)` | `SelectionCapability` | `selection(for:)` |
| 04 | `LinkHandle` | `link: bool` | `link(document_id)` | `LinkCapability` | `link(for:)` |
| 05 | `AnnotationHandle` (+ `AnnotationDraftsHandle`, `AnnotationSelectionHandle`, `AnnotationThreadsHandle`, `AnnotationToolsHandle`) | `annotation: Option<AnnotationConfig>` | `annotation(document_id)` | `AnnotationCapability` | `annotations(for:)` |
| 06 | `FormHandle` | `form: Option<FormConfig>` | `form(document_id)` | `FormCapability` | `form(for:)` |
| 07 | `RedactionHandle` | `redaction: Option<RedactionConfig>` | `redaction(document_id)` | `RedactionCapability` | `redaction(for:)` |
| 08 | `MeasurementHandle` | `measurement: Option<MeasurementConfig>` | `measurement(document_id)` | `MeasurementCapability` | `measurement(for:)` |
| 09 | `StampHandle` (+ foreign `StampLibraryStore`) | `stamp: Option<StampConfig>` | `stamp()` | `StampCapability` | `stamps()` |
| 10 | `PageEditHandle` | `page_edit: bool` | `page_edit(document_id)` | `PageEditCapability` | `pageEdit(for:)` |
| 11 | `ViewManagerHandle` | `view_manager: bool` | `view_manager()` | `ViewManagerCapability` | `viewManager()` |
| 12 | `ShellHandle` | `shell: bool` | `shell(document_id)` | `ShellCapability` | `shell(for:)` |
| 13 | `I18nHandle` | `i18n: Option<I18nConfig>` | `i18n()` | `I18nCapability` | `i18n()` |
| 14 | `CommandsHandle` | `commands: bool` | `commands()` | `CommandsCapability` | `commands()` |
| 15 | `ActionsHandle` (+ foreign `ActionExecutor`, `ActionUiHandler`, `ActionSubmitHandler`, `DocumentVerbOperation`, `ScriptEngineFactory`) | `actions: Option<ActionsConfig>` | `actions(document_id)` | `ActionsCapability` | `actions(for:)` |
| 16 | `SignatureHandle` (+ foreign `SignatureSigner`, `SignatureTrust`, set on the host) | `signature: Option<SignatureConfig>` | `signature(document_id)`, `set_signature_signer(_)`, `set_signature_trust(_)` | `SignatureCapability` | `signatures(for:)`, `setSigner(_:)`, `setTrust(_:)` |

A `bool` field means the plugin takes no config, or its config has a sensible default and no field Swift needs to set. An `Option<Config>` field means Swift sets something; `None` leaves the plugin out. A plan that finds the table wrong for its plugin (say, a config Swift does need) changes its row here in the same commit and says why in the commit message.

Two rules for `ViewerConfig` fields. `ViewerConfig` derives `Debug`, and a foreign trait object doesn't, so a config record that holds one needs a manual `Debug` impl. Whether a UniFFI 0.32 record can hold a foreign trait object at all is open (see Open questions); plan 16 sidesteps it with host-level setters (`ViewerHost::set_signature_signer`, `set_signature_trust`), which is the fallback for any plan that finds it can't. And `Session::build` moves `post` into `NativeEngine::new`; the first plan whose plugin needs `post` too changes that argument to `post.clone()`. Two plans making the same one-word edit merge cleanly.

## Shared records

`epdf-ffi` today holds `PageRef`, `Point`, `Size`, `Rect`, `TextQuad`, `ResourceStatus`, `PluginErrorInfo`, `ViewerError`, `CancelToken` and `Subscription` (`crates/epdf-ffi/src/*.rs`). Before writing a record, `grep -rn 'pub struct <Name>\|pub enum <Name>' crates/epdf-ffi/src crates/plugins/*/src/ffi*`. If it exists, use it. `epdf-ffi` can't depend on a plugin crate (the plugins depend on it), so only core-library types (`epdf-geometry`, `epdf-stage-core`, `epdf-annotation-core`) and plugin-free helpers go there.

Task 0 adds, before any plugin plan:

- `ForeignSettle<T>` and `HostError` (`foreign.rs`, Task 0c): a completion a foreign implementation answers from any thread, and the one error type every foreign trait method throws. `From<UnexpectedUniFFICallbackError>` exists for both `HostError` and `ViewerError`.
- `RegistrationHandle` (`registration.rs`, Task 0c): what every `register_*`/`set_*` member answers (interaction, i18n, commands, actions). Swift wraps it as `Registration` (Task 0f).
- `mirror_enum!` and `to_u32` (`mirror.rs`, Task 0c), and `Clone` on `KernelBound` (Task 0c), for sub-handles that share one capability.
- `PageInfo` and `page::{decode_page, decode_pages}` (`page.rs`, Task 0d): the one page record. `ViewerHost::pages`, page edit's results and stage's page reads all return it.

Plugin plans own these, and later plans consume them by name:

- Pointer input (`PointerSample`, `PointerPhase`, `PointerKind`, `PointerGesture`, `Modifiers`, `PointerPageHit`): plan 01, `crates/epdf-ffi/src/input.rs`. A tool id is a plain `String`.
- Stage-core view records (`Camera`, `ViewAnchor`, `PageFrameInsets`, `ZoomMode`, `ZoomSpec`, `StageAlign`, `StageAlignValue`, `StageAlignment`, `StageAlignmentValue`, `StageDirection`, `StageSpreadMode`, `StageSizingMode`, `StageScrollMetrics`): plan 02, `crates/epdf-ffi/src/view.rs`. The stage plugin's own records (`StageVisiblePage`, `StageRevealOptions` and the rest) stay in `epdf_plugin_stage::ffi`. A viewport size is the existing `Size`. Search's `SearchRevealOptions` stays where it is.
- Annotation-core records, first half (`AnnotationRef`, `AnnotationRevisionToken`, `PdfDestination`, `PdfLinkTarget`, and the core `PageRef` conversions): plan 04, `crates/epdf-ffi/src/annotation.rs`, because link needs them first.
- Annotation-core records, second half (`AnnotationFlags`, `AnnotationFlagsPatch`, `AnnotationPropKey`, `AnnotationProps`, `AnnotationPropsPatch`, `AnnotationLineEnding`, `AnnotationLineEndings`, `AnnotationLineEndingsPatch`, `AnnotationBlendMode`, `AnnotationTextAlign`, `AnnotationBorder`, `AnnotationCallout`, `CreationDraftAnchor`): plan 05, appended to the same file. The `Annotation` record itself stays in the annotation crate.
- Form records (`FormFieldRef`, `FormFieldFamily`): plan 06, `crates/epdf-ffi/src/form.rs`.

## The template every plugin plan follows

Every plan in this directory has the same header as `superpowers:writing-plans` requires (Goal, Architecture, Tech Stack, Spec, Global Constraints, Review Focus), names this umbrella as part of its spec, and has these tasks:

1. **Rust FFI module.** Feature, records and mirrors, handle, one `pub async fn` per member, event enum and listener, `subscribe`, `tests/ffi.rs` over `plugin_conformance::test_kernel::TestKernel` with `FakeEngine`. The member list is copied from `crates/plugins/<name>/src/harness.rs` `MEMBERS` and checked against the capability's `impl` block, not against Appendix A, which is from 2026-10-01 and may have drifted. Large plugins split this task by member group.
2. **Ports**, when the plugin has one Swift must supply: the `with_foreign` trait and its Rust adapter in `ffi.rs`, a Rust test with a Rust implementation of the foreign trait.
3. **Engine paths**, when Appendix C assigns the plan any: `translate.rs` rows and their tests, plus `epdf-engine` work where the wire has no kind.
4. **Host wiring.** The shared-file edits above, and a test in `crates/epdf-viewer-host/tests/viewer_host.rs` that builds a host with the plugin on, opens `report.pdf`, gets the handle and calls one member.
5. **Swift capability.** `platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/<Name>.swift` (typealiases, capability struct, `EventRelay` extension, `PDFViewer` accessor) and `Tests/EmbedPDFTests/<Name>Tests.swift` against a real fixture, including the three standing tests below.
6. **Docs and CI.** The `plugin-rust.yml` line, the README row, and the plan's Appendix A entry in the spec marked delivered (`**stage** (D), delivered by 02-stage.md`).

Every Swift test file includes these three, adapted to the plugin, because Review Focus items 1 to 3 of the spec apply to every plugin:

- `aClosedDocumentsCapabilityThrows`: close the document, call any member, expect `PDFError` with code `instance-closed`.
- `droppingTheStreamUnsubscribes`: iterate `events` in a task, cancel it, then trigger an event and confirm nothing leaks (the pattern in `MetadataTests.swift`).
- For every member that takes a `CancelToken`: `<member>CancelsWhenTheTaskIsCancelled`.

A workspace plugin replaces the first with `aDestroyedViewersCapabilityThrows`. A plugin with no events of its own skips the second and tests whatever stream does carry its changes (page-edit tests `.pagesChanged` on `documentEvents`).

## Definition of done, per plan

Run in the plan's worktree. Every command must pass before merging into `research/swift-plugins`.

```bash
cargo test -p epdf-plugin-<name> --features uniffi
cargo test -p epdf-ffi -p epdf-viewer-host
cargo clippy -p epdf-plugin-<name> -p epdf-viewer-host --all-targets --features epdf-plugin-<name>/uniffi -- -D warnings
cargo test -p epdf-plugin-<name>   # the conformance vectors, without uniffi, still pass
bash platforms/apple/build-xcframework.sh
(cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17')
# Kotlin keyword collisions surface at generation time, before any Android build exists for the host
cargo build -p epdf-viewer-host -p uniffi-bindgen
./target/debug/uniffi-bindgen generate --library target/debug/libepdf_viewer_host.dylib --language kotlin --no-format --out-dir "$TMPDIR/epdf-kotlin-check"
```

After the merge, run the last three commands again on `research/swift-plugins`. Two plugins can each pass alone and still collide on a generated name once both are in the archive.

Once the post-merge rerun passes, remove the plan's worktree and branch. `git worktree remove` refuses a worktree with uncommitted or untracked changes, and `git branch -d` refuses a branch that isn't merged, so both double as a check that nothing was left behind. Don't pass `--force` to either; if one refuses, find out why first.

```bash
git worktree remove .claude/worktrees/swift-<plugin>
git branch -d research/swift-<plugin>
git worktree prune
```

A plan isn't done, and doesn't get its tick under Tasks 1 to 16, until its worktree is gone.

## Review Focus

These cut across plans. Each plugin plan carries its own five as well.

1. Two plugins each exporting a record with the same name. Expected: a build failure in `build-xcframework.sh`, caught before merge. Pinned by the post-merge rerun above, not by a test.
2. A plugin enabled without its required plugin (`selection: true`, `interaction: false`). Expected: `ViewerHost::new` throws a `ViewerError::Kernel` naming the missing plugin, not a panic. Pinned in plan 03's host task (`a_plugin_without_its_requirement_fails_to_build`). Interaction requires nothing, so plan 03 is the first plan with a requirement to test against.
3. An optional integration that arrives later (link before annotation). Expected: the earlier plugin's tests keep passing once the later one merges. Pinned by each later plan's "Cross-plugin tests" section.
4. A foreign trait implementation in Swift that throws or panics on the kernel thread. Expected: the kernel reports the error through `ViewerDelegate.report` (Swift sees it on `PDFViewer.reportedErrors`, Task 0f) as a `HostError`, and the call fails with a typed error. Pinned in plan 01 (`a_throwing_handler_is_reported_and_does_not_capture`, `aThrowingHandlerReportsAndFails`), the first plan with a foreign trait.
5. Binary growth. Expected: each plan records the `libepdf_viewer_host.a` size for the simulator slice before and after in its final commit message, so B5's size budget has a series to work from.

## Open questions

These are decisions or unknowns the plans record rather than settle. Each plan names the step where it'll hit its own.

1. Plan 15's script engine. The spec names JavaScriptCore, and plan 15 uses it, but JavaScriptCore has no public execution time limit (`JSContextGroupSetExecutionTimeLimit` is SPI). On timeout plan 15 abandons the realm, so a runaway script keeps one thread spinning until the app exits, and memory and stack budgets aren't enforced. QuickJS compiled into Rust (`rquickjs`) would enforce all four budgets on iOS and Android behind the same `ScriptEngine` trait, at the cost of a second JavaScript engine in the binary.
2. A native editing overlay. Plan 05 exports 98 of the annotation plugin's 140 callable members. It leaves out the web canvas projections (live ink, selection chrome, text overlays) and the rich-text members. Whether a native reader needs those, through a follow-on plan, is undecided.
3. iOS trust. iOS can't list the system's trust anchors (`SecTrustCopyAnchorCertificates` is macOS only), and `epdf-signature-core` validates paths against the anchors it's given. Plan 16's `SignatureTrust` therefore takes anchors the app bundles. Validating against the system store would need a different port shape in `epdf-signature-core`.
4. `createAssetFromAnnotations` on iOS. `annotations.exportAppearance` isn't in `epdf-engine`, so plan 09 makes the member answer `unsupported` rather than port it.
5. Engine ports by transcription. Plan 08's measure codec and plan 16's `analyzer.rs` and `mutator.rs` (about 2,500 lines in all) are written as "transcribe TypeScript function X, lines Y" with concrete tests and wiring around them, not spelled out line by line. A reviewer should hold those steps to the TypeScript, not to the plan.
6. UniFFI 0.32 details nobody has built yet. Plans 01, 13, 14 and 15 put `Arc<dyn ForeignTrait>` inside a record; plan 16 assumes a record can't hold one and uses host-level setters. Plan 01 hits it first (its `FeedbackConfig`) and carries a fallback that makes the config an object; whatever it finds settles it for the rest. Plan 11 derives records whose fields use the alias `PaneId = String`, and falls back to mirrors if UniFFI doesn't resolve the alias. Plan 05's file-picker port assumes `#[async_trait]` works with UniFFI's async foreign traits, with a check step.

---

### Task 0a: Prepare the integration branch

Run once, before any other task. Every Task 0 step runs in the integration worktree.

**Files:**
- Modify: `.github/workflows/plugin-rust.yml`
- Modify: `platforms/apple/README.md`

- [x] **Step 1: Create the integration branch and its worktree**

```bash
git branch research/swift-plugins research/multi-platform-rust-core-impl-plugins
git worktree add .claude/worktrees/swift-plugins-integration research/swift-plugins
cd .claude/worktrees/swift-plugins-integration
```

- [ ] **Step 2: Turn the uniffi test step into a list** (skipped: no CI edits on this branch)

The step `2026-10-01-swift-package.md` Task 10 added is one long line, so every plan would conflict on it. Replace it with:

```yaml
      - name: Test the uniffi exports
        run: |
          crates=(
            metadata
            search
            render
          )
          args=(-p epdf-ffi)
          features=()
          for c in "${crates[@]}"; do
            args+=(-p "epdf-plugin-$c")
            features+=("epdf-plugin-$c/uniffi")
          done
          cargo test "${args[@]}" --features "$(IFS=,; echo "${features[*]}")"
```

Each plan appends its crate name to `crates=( ... )`.

- [ ] **Step 3: Run the step locally** (skipped with Step 2)

Run the `run:` body in bash from the repository root.
Expected: the metadata, search and render `ffi` tests pass, as they did before the edit.

- [x] **Step 4: Add the plugin table to the README**

Under a new `## Plugins` heading in `platforms/apple/README.md`:

```markdown
## Plugins

| Plugin | Swift | `ViewerConfig` | Plan |
|---|---|---|---|
| metadata | `MetadataCapability` | `metadata: true` | `2026-10-01-swift-package.md` Task 6 |
| search | `SearchCapability` | `search: SearchConfig()` | `2026-10-01-swift-package.md` Task 7 |
| render | `RenderCapability` | `render: RenderConfig()` | `2026-10-01-swift-package.md` Task 8 |
```

- [x] **Step 5: Commit**

```bash
git add .github/workflows/plugin-rust.yml platforms/apple/README.md
git commit -m "List the uniffi plugin crates one per line and add the README's plugin table"
```

### Task 0b: PDFium slices with every fork header the plans bind

The slices under `packages/engine/runtime/build/libpdfium/` were built from fork commit `7f0fa5dbe` (iOS) and `f5ca81379` (darwin, a `file://` path on another machine, per `engine-runtime-build.local.json`). They ship `epdf_action.h`, `epdf_font.h`, `epdf_form.h`, `epdf_pieceinfo.h`, `epdf_redact.h` and `epdf_text.h` only. Plans 08, 09 and 16 bind `epdf_measure.h`, `epdf_named_pages.h`, `epdf_signature.h`, `epdf_checkpoint.h` and `epdf_digest.h`, which the fork has from commit `6dc31d14e` ("Stamps share drawings; one digest implementation"). The fork checkout is the submodule at `packages/engine/runtime/runtime-src`, whose `HEAD` (`7c7b600b1` when this was written) contains `6dc31d14e`. Rebuild all three slices from it once, here, so no plan finds a header missing halfway through.

**Files:**
- Modify: `packages/engine/runtime/engine-runtime-build.local.json`

- [x] **Step 1: Confirm the headers are missing**

```bash
for t in darwin-arm64 ios-arm64 ios-sim-arm64; do
  for h in epdf_measure epdf_named_pages epdf_signature epdf_checkpoint epdf_digest; do
    test -f packages/engine/runtime/build/libpdfium/$t/include/$h.h || echo "MISSING $t $h"
  done
done
```

Expected: fifteen `MISSING` lines. None means a slice was already rebuilt; skip to Step 4.

- [x] **Step 2: Build and package each slice in the fork**

```bash
cd packages/engine/runtime/runtime-src
git merge-base --is-ancestor 6dc31d14e HEAD && echo "has 6dc31d14e"
ls public/epdf_measure.h public/epdf_named_pages.h public/epdf_signature.h public/epdf_checkpoint.h public/epdf_digest.h
for t in darwin-arm64 ios-arm64 ios-sim-arm64; do
  scripts/embedpdf-runtime/build-target.sh "$t"
  scripts/embedpdf-runtime/package-target.sh "$t"
done
ls out/embedpdf-runtime-artifacts/libembedpdf-runtime-{darwin-arm64,ios-arm64,ios-sim-arm64}-$(git rev-parse --short HEAD).tar.gz
cd -
```

Expected: `has 6dc31d14e`, the five headers listed, and three archives named for the fork's short `HEAD`. A full slice build takes a while; run the three in one go.

- [x] **Step 3: Point the manifest at them and fetch**

```bash
sha=$(git -C packages/engine/runtime/runtime-src rev-parse --short HEAD)
for t in darwin-arm64 ios-arm64 ios-sim-arm64; do
  bash packages/engine/runtime/scripts/runtime-use-local.sh "$t" \
    "$PWD/packages/engine/runtime/runtime-src/out/embedpdf-runtime-artifacts/libembedpdf-runtime-$t-$sha.tar.gz"
  bash packages/engine/runtime/scripts/fetch-libpdfium.sh "$t"
done
```

- [x] **Step 4: Run Step 1 again, then the existing suites on the new slices**

Run Step 1's loop.
Expected: no output.

Run: `cargo test -p epdf-engine -p epdf-ffi -p epdf-viewer-host && bash platforms/apple/build-xcframework.sh && (cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17')`
Expected: PASS. A fork bump that changes behaviour shows up here, before any plugin plan builds on it.

- [x] **Step 5: Commit**

```bash
git add packages/engine/runtime/engine-runtime-build.local.json
git commit -m "Build the darwin and iOS PDFium slices from a fork with the measure, named-page and signature headers"
```

The `file://` URLs in the manifest point into this machine's fork checkout, per the standing decision not to cut fork releases (`platforms/apple/README.md`). Each plan worktree reuses them by running `fetch-libpdfium.sh` for the three slices.

### Task 0c: Shared foreign-port support in `epdf-ffi`

**Files:**
- Create: `crates/epdf-ffi/src/foreign.rs`, `crates/epdf-ffi/src/registration.rs`, `crates/epdf-ffi/src/mirror.rs`
- Create: `crates/epdf-ffi/tests/foreign.rs`, `crates/epdf-ffi/tests/registration.rs`, `crates/epdf-ffi/tests/mirror.rs`
- Modify: `crates/epdf-ffi/src/bound.rs`, `crates/epdf-ffi/src/lib.rs`, `crates/epdf-ffi/tests/bound.rs`

**Interfaces:**
- Produces: `epdf_ffi::ForeignSettle<T: Clone + 'static>` with `new(resolver: Resolver<T>, post: KernelPost, what: &'static str) -> Self` (kernel thread only) and `settle(&self, make: impl FnOnce() -> Result<T, Error> + Send + 'static)` (any thread, first call wins, a drop without an answer rejects with `operation-failed`); `epdf_ffi::HostError::Failed { detail: String }` with `From<uniffi::UnexpectedUniFFICallbackError>` and `From<HostError> for epdf_kernel::Error` (name `HostError`, code `operation-failed`); `From<uniffi::UnexpectedUniFFICallbackError> for ViewerError` (the same name and code); `epdf_ffi::RegistrationHandle` with `RegistrationHandle::new(post: KernelPost, unregister: impl Fn() + 'static) -> Arc<RegistrationHandle>` (kernel thread) and the export `unregister(&self)` (idempotent; dropping the handle does not unregister); `#[macro_export] mirror_enum!(Ffi <=> path::Rust { A, B, C })`; `epdf_ffi::to_u32(n: usize, what: &str) -> Result<u32, epdf_kernel::Error>` (code `invalid-state`); `impl<C> Clone for KernelBound<C>` (clones share one slab entry, freed when the last drops).

- [x] **Step 1: Write the failing tests**

`crates/epdf-ffi/tests/foreign.rs`:

```rust
use epdf_ffi::{ForeignSettle, HostError, KernelBound, KernelThread, ViewerError};
use epdf_kernel::rt::Promise;
use futures::executor::block_on;

fn thread() -> KernelThread<()> {
    KernelThread::spawn(|_| Ok(())).unwrap()
}

/// A pending promise parked on the kernel thread, and the settle for it.
fn pending(thread: &KernelThread<()>) -> (KernelBound<Promise<u32>>, ForeignSettle<u32>) {
    let post = thread.post();
    block_on(thread.call(move |_| {
        let (promise, resolver) = Promise::<u32>::pending();
        let settle = ForeignSettle::new(resolver, post.clone(), "a test port");
        (KernelBound::new(promise, post), settle)
    }))
    .unwrap()
}

fn wait(promise: &KernelBound<Promise<u32>>) -> Result<u32, ViewerError> {
    block_on(promise.call_promise(|p| Ok(p.clone()), |v| v))
}

#[test]
fn an_answer_from_another_thread_settles_the_promise() {
    let thread = thread();
    let (promise, settle) = pending(&thread);
    std::thread::spawn(move || settle.settle(|| Ok(7))).join().unwrap();
    assert_eq!(wait(&promise).unwrap(), 7);
}

#[test]
fn the_first_answer_wins() {
    let thread = thread();
    let (promise, settle) = pending(&thread);
    settle.settle(|| Ok(1));
    settle.settle(|| Ok(2));
    assert_eq!(wait(&promise).unwrap(), 1);
}

#[test]
fn a_settle_dropped_without_an_answer_rejects() {
    let thread = thread();
    let (promise, settle) = pending(&thread);
    drop(settle);
    match wait(&promise).unwrap_err() {
        ViewerError::Kernel { code, detail, .. } => {
            assert_eq!(code.as_deref(), Some("operation-failed"));
            assert!(detail.contains("a test port"), "{detail}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_callback_error_keeps_its_reason() {
    let host = HostError::from(uniffi::UnexpectedUniFFICallbackError { reason: "boom".into() });
    let kernel: epdf_kernel::Error = host.into();
    assert_eq!(kernel.name, "HostError");
    assert_eq!(kernel.code(), Some("operation-failed"));
    assert!(kernel.message.contains("boom"), "{}", kernel.message);
    let viewer = ViewerError::from(uniffi::UnexpectedUniFFICallbackError { reason: "boom".into() });
    match viewer {
        ViewerError::Kernel { name, code, detail } => {
            assert_eq!((name.as_str(), code.as_deref(), detail.as_str()), ("HostError", Some("operation-failed"), "boom"));
        }
        other => panic!("{other:?}"),
    }
}
```

`crates/epdf-ffi/tests/registration.rs`:

```rust
use std::cell::Cell;

use epdf_ffi::{KernelThread, RegistrationHandle};
use futures::executor::block_on;

thread_local! {
    static CALLS: Cell<u32> = const { Cell::new(0) };
}

fn registration() -> (KernelThread<()>, std::sync::Arc<RegistrationHandle>) {
    let thread = KernelThread::spawn(|_| Ok(())).unwrap();
    let post = thread.post();
    let handle = block_on(thread.call(move |_| {
        RegistrationHandle::new(post, || CALLS.with(|c| c.set(c.get() + 1)))
    }))
    .unwrap();
    (thread, handle)
}

fn calls(thread: &KernelThread<()>) -> u32 {
    block_on(thread.call(|_| CALLS.with(Cell::get))).unwrap()
}

#[test]
fn unregister_runs_the_closure_on_the_kernel_thread_once() {
    let (thread, registration) = registration();
    registration.unregister();
    registration.unregister();
    assert_eq!(calls(&thread), 1);
}

#[test]
fn dropping_the_handle_does_not_unregister() {
    let (thread, registration) = registration();
    drop(registration);
    assert_eq!(calls(&thread), 0);
}
```

`crates/epdf-ffi/tests/mirror.rs`:

```rust
use epdf_ffi::{mirror_enum, to_u32};

mod core_side {
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum Side {
        Left,
        Right,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Side {
    Left,
    Right,
}

mirror_enum!(Side <=> core_side::Side { Left, Right });

#[test]
fn a_mirrored_enum_round_trips() {
    for side in [Side::Left, Side::Right] {
        assert_eq!(Side::from(core_side::Side::from(side)), side);
    }
}

#[test]
fn a_count_past_u32_is_invalid_state() {
    assert_eq!(to_u32(7, "a page index").unwrap(), 7);
    assert_eq!(to_u32(usize::MAX, "a page index").unwrap_err().code(), Some("invalid-state"));
}
```

Append to `crates/epdf-ffi/tests/bound.rs`:

```rust
thread_local! {
    static WATCH: std::cell::RefCell<Option<std::rc::Weak<Cell<u32>>>> =
        const { std::cell::RefCell::new(None) };
}

#[test]
fn a_clone_shares_the_value_and_the_last_drop_frees_it() {
    let (thread, bound) = bound();
    block_on(bound.call(|c| {
        WATCH.with(|w| *w.borrow_mut() = Some(Rc::downgrade(&c.0)));
        Ok(())
    }))
    .unwrap();
    let clone = bound.clone();
    drop(bound);
    let n = block_on(clone.call(|c| {
        c.0.set(5);
        Ok(c.0.get())
    }))
    .unwrap();
    assert_eq!(n, 5);
    drop(clone);
    let alive = block_on(thread.call(|_| {
        WATCH.with(|w| w.borrow().as_ref().is_some_and(|weak| weak.upgrade().is_some()))
    }))
    .unwrap();
    assert!(!alive, "the slab entry outlived its last handle");
}
```

If `UnexpectedUniFFICallbackError`'s field isn't named `reason` in `uniffi` 0.32.0 (`rg -n 'pub struct UnexpectedUniFFICallbackError' -A3 ~/.cargo/registry/src/*/uniffi_core-0.32.0/src`), use its constructor or field name here and in `foreign.rs`.

- [x] **Step 2: Run them to verify they fail**

Run: `cargo test -p epdf-ffi`
Expected: FAIL to compile: unresolved imports `epdf_ffi::ForeignSettle`, `HostError`, `RegistrationHandle`, `mirror_enum`, `to_u32`, and "no method named `clone`" on `KernelBound`.

- [x] **Step 3: Write `foreign.rs`**

```rust
//! Answers a foreign implementation sends back later, from any thread, and
//! the error every foreign trait method throws.
//!
//! A foreign trait method runs on the kernel thread and can't block it, so a
//! port whose Rust side wants a `Promise` hands Swift a completion object.
//! That object holds a `ForeignSettle`: the promise's resolver, parked on
//! the kernel thread, and a way to post the answer there.

use std::sync::Mutex;

use epdf_kernel::Error;
use epdf_kernel::error::code;
use epdf_kernel::rt::Resolver;

use crate::{KernelBound, KernelPost, ViewerError};

pub struct ForeignSettle<T: Clone + 'static> {
    resolver: Mutex<Option<KernelBound<Resolver<T>>>>,
    what: &'static str,
}

impl<T: Clone + 'static> ForeignSettle<T> {
    /// On the kernel thread. `what` names the port in the rejection a
    /// dropped completion produces.
    pub fn new(resolver: Resolver<T>, post: KernelPost, what: &'static str) -> Self {
        ForeignSettle { resolver: Mutex::new(Some(KernelBound::new(resolver, post))), what }
    }

    /// From any thread. The first call wins; later ones do nothing. `make`
    /// runs on the kernel thread, so it may build `!Send` values.
    pub fn settle(&self, make: impl FnOnce() -> Result<T, Error> + Send + 'static) {
        let bound = self.resolver.lock().unwrap().take();
        if let Some(bound) = bound {
            bound.fire(move |resolver| resolver.settle(make()));
        }
    }
}

impl<T: Clone + 'static> Drop for ForeignSettle<T> {
    fn drop(&mut self) {
        let what = self.what;
        self.settle(move || {
            Err(Error::new(format!("{what} was dropped without an answer"))
                .with("code", code::OPERATION_FAILED))
        });
    }
}

/// What a foreign trait method throws. UniFFI needs the conversion from
/// `UnexpectedUniFFICallbackError` for any callback that returns `Result`:
/// a Swift error it has no case for arrives as that, and keeps its text.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum HostError {
    #[error("{detail}")]
    Failed { detail: String },
}

impl From<uniffi::UnexpectedUniFFICallbackError> for HostError {
    fn from(error: uniffi::UnexpectedUniFFICallbackError) -> Self {
        HostError::Failed { detail: error.reason }
    }
}

impl From<HostError> for Error {
    fn from(error: HostError) -> Self {
        match error {
            HostError::Failed { detail } => {
                Error::named("HostError", detail).with("code", code::OPERATION_FAILED)
            }
        }
    }
}

/// For a foreign trait method that returns `Result<_, ViewerError>`.
impl From<uniffi::UnexpectedUniFFICallbackError> for ViewerError {
    fn from(error: uniffi::UnexpectedUniFFICallbackError) -> Self {
        ViewerError::Kernel {
            name: "HostError".into(),
            code: Some(code::OPERATION_FAILED.into()),
            detail: error.reason,
        }
    }
}
```

- [x] **Step 4: Write `registration.rs`**

```rust
//! What a plugin hands back for a registration (a tool, a handler, a
//! command, a locale): a way to undo it, callable from any thread.
//!
//! Dropping the handle does not unregister. A registration nobody keeps a
//! handle to is the common case, and it should stay registered.

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::{KernelBound, KernelPost};

#[derive(uniffi::Object)]
pub struct RegistrationHandle {
    unregister: KernelBound<Rc<dyn Fn()>>,
    done: AtomicBool,
}

impl RegistrationHandle {
    /// On the kernel thread.
    pub fn new(post: KernelPost, unregister: impl Fn() + 'static) -> Arc<Self> {
        let unregister: Rc<dyn Fn()> = Rc::new(unregister);
        Arc::new(RegistrationHandle {
            unregister: KernelBound::new(unregister, post),
            done: AtomicBool::new(false),
        })
    }
}

#[uniffi::export]
impl RegistrationHandle {
    /// Idempotent. Runs on the kernel thread after this returns.
    pub fn unregister(&self) {
        if !self.done.swap(true, Ordering::SeqCst) {
            self.unregister.fire(|unregister| unregister());
        }
    }
}
```

- [x] **Step 5: Write `mirror.rs`**

```rust
//! Helpers every plugin's `ffi` module uses for its mirrors.

/// `From` both ways between two fieldless enums with the same variants.
#[macro_export]
macro_rules! mirror_enum {
    ($ffi:ident <=> $rust:path { $($variant:ident),+ $(,)? }) => {
        impl From<$rust> for $ffi {
            fn from(value: $rust) -> Self {
                match value {
                    $(<$rust>::$variant => $ffi::$variant,)+
                }
            }
        }
        impl From<$ffi> for $rust {
            fn from(value: $ffi) -> Self {
                match value {
                    $($ffi::$variant => <$rust>::$variant,)+
                }
            }
        }
    };
}

/// A count or index as a `u32`, or `invalid-state`: never a clamp.
pub fn to_u32(n: usize, what: &str) -> Result<u32, epdf_kernel::Error> {
    u32::try_from(n).map_err(|_| {
        epdf_kernel::Error::named("InvalidState", format!("{what} ({n}) does not fit a u32"))
            .with("code", "invalid-state")
    })
}
```

`mirror_enum!` uses `<$rust>::$variant` so that a path like `core_side::Side` parses as a type in pattern position. If rustc rejects that form in a pattern on this toolchain, change the macro's patterns to `$rust::$variant`.

- [x] **Step 6: Make `KernelBound` cloneable**

In `crates/epdf-ffi/src/bound.rs`, replace `pub struct KernelBound<C: 'static> { id: u64, post: KernelPost, _value: PhantomData<fn() -> C> }` and its `impl Drop` with a shared slot:

```rust
/// A slab entry's id and the post that frees it, shared by every clone of
/// a `KernelBound`. The last clone to drop frees the entry.
struct Slot {
    id: u64,
    post: KernelPost,
}

impl Drop for Slot {
    fn drop(&mut self) {
        let id = self.id;
        (self.post)(Box::new(move || {
            SLAB.with(|s| s.borrow_mut().remove(&id));
        }));
    }
}

pub struct KernelBound<C: 'static> {
    slot: Arc<Slot>,
    _value: PhantomData<fn() -> C>,
}

impl<C: 'static> Clone for KernelBound<C> {
    fn clone(&self) -> Self {
        KernelBound { slot: self.slot.clone(), _value: PhantomData }
    }
}
```

In `KernelBound::new`, build it as `KernelBound { slot: Arc::new(Slot { id, post }), _value: PhantomData }`. In `call`, `call_promise`, `call_promise_cancellable` and `fire`, read `self.slot.id` for `self.id` and `self.slot.post` for `self.post`. Delete `impl<C: 'static> Drop for KernelBound<C>`: `Slot`'s drop replaces it.

- [x] **Step 7: Export them from `lib.rs`**

In `crates/epdf-ffi/src/lib.rs`, beside the existing modules and re-exports:

```rust
pub mod foreign;
pub mod mirror;
pub mod registration;

pub use foreign::{ForeignSettle, HostError};
pub use mirror::to_u32;
pub use registration::RegistrationHandle;
```

- [x] **Step 8: Run the tests to verify they pass**

Run: `cargo test -p epdf-ffi && cargo clippy -p epdf-ffi --all-targets -- -D warnings`
Expected: PASS: the four `foreign` tests, two `registration`, two `mirror`, the new `bound` test, and every existing one. `tests/bound.rs`'s drop-on-the-kernel-thread tests pin that the shared slot still frees on the kernel thread.

- [x] **Step 9: Commit**

```bash
git add crates/epdf-ffi
git commit -m "Add ForeignSettle, HostError, RegistrationHandle and the mirror helpers to epdf-ffi, and make KernelBound cloneable"
```

### Task 0d: `PageInfo` in `epdf-ffi`

Page edit returns pages, stage returns pages, and `ViewerHost::pages` already does. One record serves all three.

**Files:**
- Create: `crates/epdf-ffi/src/page.rs`
- Modify: `crates/epdf-ffi/Cargo.toml`, `crates/epdf-ffi/src/lib.rs`
- Modify: `crates/epdf-viewer-host/src/api.rs` (remove `PageInfo`, `decode_page`, `decode_pages`; re-export)

**Interfaces:**
- Produces: `epdf_ffi::PageInfo { page: PageRef, index: u32, width: f64, height: f64, rotation: u32 }`, `epdf_ffi::page::decode_page(&Value) -> Result<PageInfo, epdf_kernel::Error>`, `epdf_ffi::page::decode_pages(&[Value]) -> Result<Vec<PageInfo>, Error>`. A malformed page errs with code `invalid-state`. The generated Swift name stays `PageInfo`, so `Exports.swift` doesn't change.

- [x] **Step 1: Write the failing test**

Create `crates/epdf-ffi/src/page.rs` with only the tests:

```rust
#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_layout_page_decodes_to_page_info() {
        let page = json!({
            "pageObjectNumber": 7, "index": 2, "rotation": 90,
            "size": { "width": 612.0, "height": 792.0 },
            "ref": { "kind": "objectNumber", "pageObjectNumber": 7 }
        });
        assert_eq!(
            decode_page(&page).unwrap(),
            PageInfo {
                page: PageRef { page_object_number: 7 },
                index: 2,
                width: 612.0,
                height: 792.0,
                rotation: 90,
            }
        );
    }

    #[test]
    fn a_page_without_its_size_is_invalid_state() {
        let page = json!({ "pageObjectNumber": 7, "index": 2, "rotation": 0 });
        assert_eq!(decode_page(&page).unwrap_err().code(), Some("invalid-state"));
    }
}
```

Add `pub mod page;` to `crates/epdf-ffi/src/lib.rs`.

- [x] **Step 2: Run it to see it fail**

Run: `cargo test -p epdf-ffi page::`
Expected: FAIL to compile, `cannot find function decode_page` and `cannot find type PageInfo`.

- [x] **Step 3: Move the record and its decoding**

Add to `crates/epdf-ffi/Cargo.toml` `[dependencies]`:

```toml
serde_json = "1"
```

Above the tests in `crates/epdf-ffi/src/page.rs`:

```rust
//! A page as `pages.list` and every page mutation's `layout` describe it.
//! Here rather than in `epdf-viewer-host` because the page-edit and stage
//! plugins return it too.

use epdf_kernel::Error;
use serde_json::Value;

use crate::PageRef;

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PageInfo {
    pub page: PageRef,
    pub index: u32,
    pub width: f64,
    pub height: f64,
    /// Degrees clockwise: 0, 90, 180 or 270.
    pub rotation: u32,
}

fn malformed(what: String) -> Error {
    Error::named("InvalidState", format!("[epdf-ffi] {what}")).with("code", "invalid-state")
}

fn whole(value: &Value, field: &str, what: &str) -> Result<u32, Error> {
    value
        .get(field)
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| malformed(format!("{what} without a whole \"{field}\": {value}")))
}

fn number(value: &Value, field: &str, what: &str) -> Result<f64, Error> {
    value
        .get(field)
        .and_then(Value::as_f64)
        .ok_or_else(|| malformed(format!("{what} without \"{field}\": {value}")))
}

/// A `PageLayout`: `pageObjectNumber`, `index`, `size` and `rotation`.
pub fn decode_page(page: &Value) -> Result<PageInfo, Error> {
    let size =
        page.get("size").ok_or_else(|| malformed(format!("a page without \"size\": {page}")))?;
    Ok(PageInfo {
        page: PageRef { page_object_number: whole(page, "pageObjectNumber", "a page")? },
        index: whole(page, "index", "a page")?,
        width: number(size, "width", "a page size")?,
        height: number(size, "height", "a page size")?,
        rotation: whole(page, "rotation", "a page")?,
    })
}

pub fn decode_pages(pages: &[Value]) -> Result<Vec<PageInfo>, Error> {
    pages.iter().map(decode_page).collect()
}
```

In `crates/epdf-ffi/src/lib.rs`, add `pub use page::PageInfo;` beside the other re-exports.

In `crates/epdf-viewer-host/src/api.rs`, delete `pub struct PageInfo`, `fn decode_page` and `fn decode_pages`, plus the `number` and `whole` helpers if nothing else uses them, and add:

```rust
pub use epdf_ffi::PageInfo;
pub(crate) use epdf_ffi::page::decode_pages;
```

`crates/epdf-viewer-host/src/lib.rs` keeps `use api::{Lifecycle, decode_pages};` and `pub use api::{..., PageInfo, ...}` unchanged.

- [x] **Step 4: Run the tests**

Run: `cargo test -p epdf-ffi -p epdf-viewer-host`
Expected: PASS, including `a_document_opens_and_lists_its_pages`, which now decodes through `epdf_ffi::page`.

- [x] **Step 5: Commit**

```bash
git add crates/epdf-ffi crates/epdf-viewer-host/src/api.rs
git commit -m "Move PageInfo into epdf-ffi so plugins can return it"
```

### Task 0e: Shared translation pieces in `epdf-viewer-host`

**Files:**
- Modify: `crates/epdf-viewer-host/src/translate.rs`, `crates/epdf-viewer-host/src/native_engine.rs`
- Test: `crates/epdf-viewer-host/tests/translate.rs`

**Interfaces:**
- Produces: `pub const PATHS: &[&str]` and `pub const FLAGS: &[&str]` in `translate`; `maps(path)` answering a routed path, a namespace with a listed path under it (`maps("metadata")`), or a listed flag; `pub fn whole(path: &str, field: &str, value: &Value) -> Result<Value, Error>` (a whole, non-negative JavaScript number as an integer, else `invalid-input`) and `pub fn optional_whole(path: &str, field: &str, value: Option<&Value>) -> Result<Value, Error>` (absent, `null` and `$undefined` answer `Value::Null`); `translate` leaving out any request field whose value is `null` or `$undefined`; `NativeEngine::open` taking `{"bytes": <Bytes JSON>}` (any `kind`) beside `{"$host": id}`.

- [x] **Step 1: Write the failing tests**

Append to `crates/epdf-viewer-host/tests/translate.rs`:

```rust
use epdf_viewer_host::translate::{PATHS, maps, optional_whole, whole};

#[test]
fn every_routed_path_is_listed() {
    for path in PATHS {
        assert!(translate("d1", path, Some(1), &[json!({}), json!({})], 0).is_ok(), "{path}");
    }
}

#[test]
fn a_namespace_answers_when_a_listed_path_is_under_it() {
    assert!(maps("metadata"));
    assert!(maps("metadata.read"));
    assert!(maps("page.text"));
    assert!(!maps("meta"));
    assert!(!maps("metadata.write"));
    assert!(!maps("signatures"));
}

#[test]
fn a_whole_javascript_number_goes_out_as_an_integer() {
    let value = whole("pages.rotate", "rotation", &json!(90.0)).unwrap();
    assert_eq!(value, json!(90));
    assert!(value.is_u64());
    for bad in [json!(1.5), json!(-1.0), json!({ "$number": "NaN" }), json!("3")] {
        assert_eq!(whole("p", "f", &bad).unwrap_err().code(), Some("invalid-input"), "{bad}");
    }
}

#[test]
fn an_absent_or_undefined_whole_is_left_out() {
    assert_eq!(optional_whole("p", "f", None).unwrap(), Value::Null);
    assert_eq!(optional_whole("p", "f", Some(&Value::Null)).unwrap(), Value::Null);
    assert_eq!(optional_whole("p", "f", Some(&epdf_kernel::undefined())).unwrap(), Value::Null);
    assert_eq!(optional_whole("p", "f", Some(&json!(2.0))).unwrap(), json!(2));
}

#[test]
fn an_undefined_argument_is_left_out_of_the_request() {
    let call = translate("d1", "metadata.update", None, &[epdf_kernel::undefined()], 9).unwrap();
    assert_eq!(call.request, json!({ "kind": "metadata.update", "jobId": 9, "docId": "d1" }));
}

#[test]
fn the_native_engine_opens_bytes_sent_as_json() {
    let pages = on_kernel(EngineWorker::spawn(), |engine, tx| {
        let bytes = epdf_kernel::bytes::Bytes(test_support::fixture(test_support::REPORT)).to_json();
        engine.open(json!({ "kind": "bytes", "bytes": bytes }), None).on_settle(move |doc| {
            let doc = doc.as_ref().ok().cloned().unwrap();
            doc.call("pages.list", None, vec![]).on_settle(move |list| {
                let _ = tx.send(list.as_ref().ok().map(|l| l["pages"].as_array().map_or(0, Vec::len)));
            });
        });
    });
    assert_eq!(pages, Some(14));
}

#[test]
fn an_open_without_bytes_is_invalid_input() {
    let code = on_kernel(EngineWorker::spawn(), |engine, tx| {
        engine.open(json!({ "kind": "bytes" }), None).on_settle(move |doc| {
            let _ = tx.send(doc.as_ref().err().and_then(|e| e.code().map(str::to_owned)));
        });
    });
    assert_eq!(code.as_deref(), Some("invalid-input"));
}
```

`on_kernel` is the file's existing helper. If a path in `PATHS` fails on `every_routed_path_is_listed`'s dummy args (a later plan's path that needs a page list or bytes), the plan that lists it skips it there with a comment naming the test that covers it.

- [x] **Step 2: Run them to verify they fail**

Run: `cargo test -p epdf-viewer-host --test translate`
Expected: FAIL to compile on the `PATHS`, `whole` and `optional_whole` imports.

- [x] **Step 3: List the paths and derive namespaces**

In `crates/epdf-viewer-host/src/translate.rs`, replace `maps`:

```rust
/// Every kernel path `route` maps, for `maps`' namespace check. A path added
/// to `route` goes here too, one per line, in plan order;
/// `every_routed_path_is_listed` checks the two agree.
pub const PATHS: &[&str] = &[
    "close",
    "pages.list",
    "metadata.read",
    "metadata.update",
    "page.text.read",
    "page.geometry.read",
    "page.render.image",
    "search.query",
];

/// Feature names a plugin checks with `doc.has` that are neither a path nor
/// a namespace of one (`document.versioned`). One per line, in plan order.
pub const FLAGS: &[&str] = &[];

/// Whether `translate` maps `path`; or, for a bare namespace such as
/// `redaction`, any listed path under it; or a listed flag. This is what
/// `DocumentHandle::has` answers, and the plugins ask all three ways
/// (`doc.has("forms.applyEffects")`, `doc.has("redaction")`,
/// `doc.has("document.versioned")`).
pub fn maps(path: &str) -> bool {
    route(path).is_some()
        || FLAGS.contains(&path)
        || PATHS.iter().any(|p| p.strip_prefix(path).is_some_and(|rest| rest.starts_with('.')))
}
```

- [x] **Step 4: Add the number helpers and drop `$undefined` fields**

Below `translate`:

```rust
/// A JavaScript number as the whole, non-negative integer the host parses.
/// Plugins send `90.0` where the host's serde wants `90`; anything else (a
/// fraction, a negative, `{"$number": "NaN"}`) is refused with a message
/// that names the field.
pub fn whole(path: &str, field: &str, value: &Value) -> Result<Value, Error> {
    value
        .as_f64()
        .filter(|n| n.is_finite() && *n >= 0.0 && n.fract() == 0.0 && *n <= f64::from(u32::MAX))
        .map(|n| Value::from(n as u64))
        .ok_or_else(|| {
            coded(
                "invalid-input",
                format!("[epdf-viewer-host] {path}'s {field} is not a whole number: {value}"),
            )
        })
}

/// `whole`, with an absent, `null` or `undefined` value left out.
pub fn optional_whole(path: &str, field: &str, value: Option<&Value>) -> Result<Value, Error> {
    match value {
        None | Some(Value::Null) => Ok(Value::Null),
        Some(v) if epdf_kernel::is_undefined(v) => Ok(Value::Null),
        Some(v) => whole(path, field, v),
    }
}
```

In `translate`, change the request loop's filter from `if !value.is_null()` to:

```rust
        if !value.is_null() && !epdf_kernel::is_undefined(&value) {
```

- [x] **Step 5: Open from `Bytes` JSON**

In `crates/epdf-viewer-host/src/native_engine.rs`, in `Engine::open`, replace the `let Some(bytes) = ... else { ... };` block with:

```rust
        // The host's own opens name their bytes in the `HostTable`. A plugin
        // opening a document of its own (page edit's subset insert, the
        // stamp plugin's asset libraries) sends them as `Bytes` JSON under
        // `bytes`.
        let bytes = match input.get("$host").and_then(Value::as_u64) {
            Some(id) => HostTable::take(id),
            None => input.get("bytes").and_then(epdf_kernel::bytes::Bytes::from_json).map(|b| b.0),
        };
        let Some(bytes) = bytes else {
            return Promise::rejected(
                Error::new(
                    "[epdf-viewer-host] open needs {\"$host\": id} or {\"bytes\": ...} naming the bytes",
                )
                .with("code", "invalid-input"),
            );
        };
```

- [x] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p epdf-viewer-host && cargo clippy -p epdf-viewer-host --all-targets -- -D warnings`
Expected: PASS, the seven new tests and every existing one.

- [x] **Step 7: Commit**

```bash
git add crates/epdf-viewer-host
git commit -m "Share the native translator's path list, number helpers and Bytes opens"
```

### Task 0f: Shared Swift pieces and test fixtures

**Files:**
- Modify: `platforms/apple/EmbedPDF/Sources/EmbedPDF/PDFViewer.swift`, `platforms/apple/EmbedPDF/Sources/EmbedPDF/EventRelay.swift`, `platforms/apple/EmbedPDF/Sources/EmbedPDF/Exports.swift`
- Create: `platforms/apple/EmbedPDF/Sources/EmbedPDF/Registration.swift`
- Modify: `platforms/apple/EmbedPDF/Tests/EmbedPDFTests/ViewerTests.swift`
- Modify: `platforms/apple/build-xcframework.sh`
- Modify: `crates/test-support/src/lib.rs`, `crates/epdf-viewer-host/tests/viewer_host.rs`

**Interfaces:**
- Consumes: Task 0c's `RegistrationHandle` and `HostError`.
- Produces: `PDFViewer.reportedErrors: AsyncStream<PDFError>` (errors the kernel reports rather than throws; keeps the newest 16; single consumer; finishes on `destroy()`); Swift `Registration` (wraps `RegistrationHandle`; `unregister()`); `public typealias HostError`; `FIXTURES` in `build-xcframework.sh`; `test_support::FEATURES`.

- [x] **Step 1: Write the failing tests**

Append to `ViewerTests` in `platforms/apple/EmbedPDF/Tests/EmbedPDFTests/ViewerTests.swift`:

```swift
    @Test func reportedErrorsFinishWhenTheViewerIsDestroyed() async throws {
      let viewer = try await started()
      let drained = Task {
        var count = 0
        for await _ in viewer.reportedErrors { count += 1 }
        return count
      }
      try await viewer.destroy()
      expectNoDifference(await drained.value, 0)
    }

    @Test func theFeaturesFixtureIsBundled() async throws {
      let viewer = try await started()
      let document = try await viewer.open(Fixture.data("features"))
      expectNoDifference(document.pageCount, 3)
    }
```

Append to `crates/epdf-viewer-host/tests/viewer_host.rs`:

```rust
#[test]
fn the_features_fixture_opens() {
    let host = started();
    let doc = block_on(host.open_document(test_support::fixture(test_support::FEATURES), options()))
        .unwrap();
    assert_eq!(doc.page_count, 3);
}
```

- [x] **Step 2: Run them to verify they fail**

Run: `cargo test -p epdf-viewer-host --test viewer_host features_fixture`
Expected: FAIL to compile, "cannot find value `FEATURES` in crate `test_support`".

- [x] **Step 3: Add the fixture constant**

In `crates/test-support/src/lib.rs`, beside the other constants:

```rust
/// Three 600 by 400 pages. Page 1 has an internal `GoTo` link to page 3
/// (`/Rect [40 300 320 330]`) and a `URI` link to
/// `https://example.invalid/target` (`/Rect [40 240 360 270]`).
pub const FEATURES: &str = "tooling/viewer-e2e/fixtures/features.pdf";
```

- [x] **Step 4: Copy fixtures from one list**

In `platforms/apple/build-xcframework.sh`, replace the `for tests in "${TEST_TARGETS[@]}"; do ... done` loop with:

```bash
# Repo-relative files every test bundle gets, one per line. Each plugin plan
# appends its own after the last plan's lines. Entries are expanded
# unquoted, so a glob copies every match.
FIXTURES=(
  pdfium-multiplatform-prototype/fixtures/report.pdf
  pdfium-multiplatform-prototype/fixtures/colors.pdf
  pdfium-multiplatform-prototype/fixtures/encrypted-hello.pdf
  tooling/viewer-e2e/fixtures/features.pdf
)
for tests in "${TEST_TARGETS[@]}"; do
  mkdir -p "$OUT/Tests/$tests/Fixtures"
  for fixture in "${FIXTURES[@]}"; do
    # shellcheck disable=SC2086
    cp "$ROOT"/$fixture "$OUT/Tests/$tests/Fixtures/"
  done
done
```

- [x] **Step 5: Forward reported errors**

In `EventRelay.swift`, replace `StateRelay` with:

```swift
final class StateRelay: ViewerDelegate, @unchecked Sendable {
  let continuation: AsyncStream<UInt64>.Continuation
  let errors: AsyncStream<PDFError>.Continuation

  init(_ continuation: AsyncStream<UInt64>.Continuation, errors: AsyncStream<PDFError>.Continuation) {
    self.continuation = continuation
    self.errors = errors
  }

  func stateChanged(revision: UInt64) {
    continuation.yield(revision)
  }

  func report(error: ViewerError) {
    errors.yield(PDFError(error))
  }
}
```

In `PDFViewer.swift`, add the stored stream and continuation:

```swift
  private let errorContinuation: AsyncStream<PDFError>.Continuation

  /// Errors the kernel reports rather than throws: a listener that failed,
  /// a foreign handler that threw (`HostError`). Keeps the newest 16. The
  /// stream supports a single consumer.
  public let reportedErrors: AsyncStream<PDFError>
```

In `init`, before building the host:

```swift
    let (errors, errorContinuation) = AsyncStream<PDFError>.makeStream(bufferingPolicy: .bufferingNewest(16))
```

pass `StateRelay(continuation, errors: errorContinuation)` as the delegate, and after the `do`/`catch` assign `reportedErrors = errors` and `self.errorContinuation = errorContinuation`. In `finishStreams()`, add `errorContinuation.finish()`.

- [x] **Step 6: Add `Registration` and the `HostError` alias**

`platforms/apple/EmbedPDF/Sources/EmbedPDF/Registration.swift`:

```swift
internal import EmbedPDFBindings

/// A registered tool, handler, command or locale. `unregister()` undoes it;
/// dropping this without calling it leaves the registration in place.
public struct Registration: Sendable {
  let handle: RegistrationHandle

  /// Idempotent.
  public func unregister() {
    handle.unregister()
  }
}
```

In `Exports.swift`, after `PluginErrorInfo`:

```swift
public typealias HostError = EmbedPDFBindings.HostError
```

- [x] **Step 7: Run the tests to verify they pass**

Run: `cargo test -p epdf-viewer-host --test viewer_host && bash platforms/apple/build-xcframework.sh && (cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17')`
Expected: PASS, including `the_features_fixture_opens`, `reportedErrorsFinishWhenTheViewerIsDestroyed` and `theFeaturesFixtureIsBundled`.

- [x] **Step 8: Commit**

```bash
git add crates/test-support crates/epdf-viewer-host/tests platforms/apple
git commit -m "Add the reported-error stream, Registration and a shared fixture list to the Swift package"
```

Task 0 is done when 0a to 0f are committed on `research/swift-plugins`. No plugin plan starts before then.

### Task 1 to 16: the plugin plans

Execute `01-interaction.md` through `16-signature.md` by wave. Tick each one here after it merges, its post-merge rerun passes and its worktree and branch are removed.

- [ ] 01 interaction
- [ ] 02 stage
- [ ] 03 selection
- [ ] 04 link
- [ ] 05 annotation
- [ ] 06 form
- [ ] 07 redaction
- [ ] 08 measurement
- [ ] 09 stamp
- [ ] 10 page-edit
- [ ] 11 view-manager
- [ ] 12 shell
- [ ] 13 i18n
- [ ] 14 commands
- [ ] 15 actions
- [ ] 16 signature

### Task 17: Close out

- [ ] **Step 1:** On `research/swift-plugins`, run the full definition of done with every plugin on: `ViewerConfig` with every field set, in `crates/epdf-viewer-host/tests/viewer_host.rs` as `every_plugin_builds_together`.
- [ ] **Step 2:** Update `multiplatform-port-status.md` with one line per plugin pointing at its Swift test file.
- [ ] **Step 3:** Hand B5 (hardening, spec Appendix B) the binary size series from Review Focus item 5.
- [ ] **Step 4:** Confirm no plan worktree is left: `git worktree list | grep swift-` prints only `swift-plugins-integration`. Then remove that one too, once `research/swift-plugins` is merged or pushed wherever it's going: `git worktree remove .claude/worktrees/swift-plugins-integration`.
