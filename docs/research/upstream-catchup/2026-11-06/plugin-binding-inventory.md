# Current plugin/native contract worklist — planning only

Target `8297bad8d388eb9c70324c22cb001c59248f95f4` and runtime `4346c5131cd009c4086fdce13bde7fd43fbd4ce0` are fixed. Both public branch starting SHAs are in [pinned-targets.json](pinned-targets.json). This worklist supersedes C7's six-package/36-path characterization; **155 plugin source paths in 21 packages change after C7 alone**. C7 P1–P5 behavior remains required. [Historical detail](historical/c7/plugin-binding-inventory.md) is retained without overwriting its seals.

## Portable behavior worklist

| Work | Existing owner families | Required new proof |
|---|---|---|
| P1 annotation/ink, P2 caret/fonts | `crates/plugins/annotation-core/src/{props,record,scene,ink,richtext}.rs`, annotation read/write/model/FFI, font consumers | Stable sibling identity, omitted/null/dash distinctions, no-stroke width geometry, Bezier ink/bounds/hit testing, markup comment projection, returned draft paragraphs, caret/range/whole-box formatting, stale font/close cancellation and standard fallback. Engine mutation/DTO policy stays in Job 3. |
| P3 stage/spreads and P4 snapshot | Stage contract/settings/navigation/read/camera/FFI; render contract/controller/model/host/JS/FFI; commands/image ports | Current page groups, spread stepping, wheel/swipe/flick/RTL/tall pages/disabled turns; region and rotation; whole-page-only batch rendering; scale 2 and 16,000,000 pixel cap; clip/downscale/permission/refusal/cancel/close cases; raw native image lifetime and actual browser activation/copy/save. Preserve C8 visibility/tile model ownership. |
| Search and signature mark | Search controller/read/FFI plus form and signature controller/paint owners; engine hasMark policy from Job 3 | Default snippets start false without copy permission. Explicit snippets true refuses before engine. Unpinned permission-race denial retries without snippets. Keep hit identity regression. Unsigned box painter must not overwrite a visual/signed mark; move/resize still works; marked style refusal and refreshed void result agree. |
| I18n fallback/lazy loading | `crates/plugins/i18n/src/{controller,model,loaders,ffi,js}.rs`; TS plugin-i18n contract/controller and tests | Start requested and fallback loading; do not show requested language before fallback ready. Fallback failure/current locale behavior follows final oracle; existing locale strings survive until loader replaces them; settings fallback changes load it; stale completions, loader rejection and destruction cannot publish obsolete state. Native loader host ports must preserve lifecycle. |
| Live command labels/plurals | `crates/plugins/commands/src/{contract,availability,controller,ffi}.rs`, i18n plural owner, command rendering/search surfaces | `labelParams(context)` reads current appState, `count` selects language plural form; command-ID translation gets same params; command search sees current label; no-i18n labels still interpolate; thrown callback leaves placeholders. Preserve existing native appState/command setting locks. |

New behavior sources: `9dc4c2aa5` search permissions, `14fbaf093` signature mark, `58435c49a` i18n fallback/lazy loading, `ccc4bf7fe` command params and `6dcc2376f` viewer plural forms. Full identities and later merge/follow-up commits are in [history](viewer-since-c7-commits.tsv); final source wins over intermediate commit subjects.

## Cross-plugin contract migration

| Final target change | Owning seams and required acceptance |
|---|---|
| Capability/method/type names | Compare all final `packages/plugin/*/src/contract.ts` with Rust router method strings, capability implementations, FFI/JS and Swift/Kotlin callers. Examples: attachment `add/remove`→`create/delete`, annotation selection `set/list`→`select/getSelected`, fonts `getProgram(displayHash)`→`readProgram(displaySha256)`, stage `getPageFrame`→`getPageBox`, reveal `anchor`→`align`, interaction `onDown`→`onPointerDown`, stamp `readAssetBytes`→`getAssetBytes`. Exercise actual string dispatch; compilation alone cannot prove method names. |
| Shared settings/events | TS kernel owner is `packages/core/main/src/settings.ts`; mirror final `changedKeys` in Rust kernel and all listeners. Attachment events `added/removed`→`created/deleted`; annotation comment `change`→`type`, stamp-library `reason`→`type`; propagate to generated records/native consumers without duplicate event emission. |
| Null and return shape | Search/signature `refresh` resolves void. Render Picture format/quality use final nullable values. Required-null results must remain present through JSON/UniFFI; rejected writes still fail. Host lifecycle final `destroy` replaces old `dispose` at the relevant boundary. Check settings reset semantics, retained handles and asynchronous teardown. |
| Tags, units, hashes and permission names | Field `family`→`kind` and outcome discriminants→`status` at final public boundaries; do not mechanically rename internal PDF field-family policy. `dragThresholdPx`, `shortcutLabel`, signing `signingKey`, signature `validationTime` and final SignatureInfo records; shared engine unit/hash/date fields come from Job 3 once. |
| Type/adapter/public entry point names | Api→Capability and DTO/type renames span plugin declarations, built-in viewer/framework doors, JS/WASM and platform facades. Preserve existing native supported product scope. Match updated API snapshots and imports rather than preserving stale aliases without a scoped compatibility decision. |

The [contract migration excerpts](contract-migration-map.txt) and [functional diff](viewer-functional.diff) are exhaustive source inputs, while these examples are review focus. Do not treat the examples as the entire migration. Upstream website/docs/menu/sheet/CloudPDF/SDK corrections arrive through TS and run their owning checks; portable decisions at existing Rust owners still require parity. Importing server/SDK source does not create a native CloudPDF service project or turn the existing .NET facade into a new plugin product.

## Dependency and acceptance sequence

1. Consume reviewed Job 3 shared contracts and matching runtime. Complete annotation/text vocabulary and geometry once before snapshot/native work.
2. Migrate portable plugin decisions and cross-plugin contracts; add failing target cases, including the new search/i18n/commands/signature cases above. Replay through the real Rust router/harness.
3. Regenerate UniFFI Swift/Kotlin/WASM/C metadata together, update actual facade/host seams and verify method/key/tag/null behavior. Retain root SwiftPM/iOS/macOS package, static slices and tiled rendering after f28ed; do not reset them to the C7 snapshot.
4. Validate real supported native/browser consumers and default artifact loading; compare source/runtime/header/symbol/font/payload association. Report missing host/device/RID legs, preserve old published bytes, keep local .NET package status truthful.
5. Audit boundaries/API snapshots and one changeset per directly affected publishable package. Final Job 5 consumer and package checks are shared, not duplicate builds for each branch name.

Proposed entry points during authorized implementation:

```sh
pnpm check:plugin-boundaries
pnpm --filter @embedpdf/tooling-plugin-conformance vectors:check
pnpm --filter @embedpdf/tooling-plugin-conformance coverage:check
CARGO_BUILD_JOBS=2 cargo test -p epdf-annotation-core -p epdf-plugin-annotation -p epdf-plugin-stage -p epdf-plugin-render -p epdf-plugin-commands -p epdf-plugin-search -p epdf-plugin-i18n
pnpm api:check
```

Confirm package names/scripts and required features in the merged tree; include `uniffi` legs and missing target scenario coverage. Run matching Apple Swift consumer, Kotlin generation/compile, browser and existing .NET graph scripts under their exact resource wrappers. Historical manifests and old pass counts cannot claim new acceptance.

Updated P1–P5 estimate: **41–74 engineering hours** (old29–52 plus12–22 for this broader plugin/native migration), excluding shared engine DTO work. Native/device queues are additional. Job 4 owns this allocation; Job 5 performs the final shared artifact acceptance. See the consolidated **80–145 engineering hour / 8–24 machine hour** envelope in the main plan.

Next action (under two minutes): inspect one changed final plugin contract against its Rust router name to confirm the migration boundary before implementation.
