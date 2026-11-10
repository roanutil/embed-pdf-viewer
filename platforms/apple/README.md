# Apple

The Apple package exposes the Rust viewer through typed Swift capabilities and
SwiftUI controls on iOS 17 and macOS 14 or later.

**This branch requires the local source engine or locally packaged engine.**
The published engine pins predate the app-facing document, settings, editing and
snapshot APIs validated on this branch.
Use the nested package at `platforms/apple/EmbedPDF` with one of the local modes
below. The repository-root package remains pinned to the previous release; a
matching engine must be published and both manifests updated before default
downloaded-package consumption is supported.

```bash
bash platforms/apple/build-xcframework.sh
cd platforms/apple/EmbedPDF
EMBEDPDF_LOCAL_ENGINE=1 xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,id=<UDID>'
```

When two simulators share a name, `name=` is ambiguous. Pass the device's id
instead, from `xcrun simctl list devices`:
`-destination 'platform=iOS Simulator,id=<UDID>'`.

The package has two products over a third, internal target. `EmbedPDF` is the
Swift facade and `EmbedPDFUI` is its SwiftUI layer. Capabilities retain their
viewer and marshal all kernel work to Rust; controls project that state onto
the main actor.
`EmbedPDFBindings` is the internal target under them, holding the uniffi
Swift for every crate in the archive: `epdf_uniffi`, `epdf_ffi` and
`epdf_viewer_host` today, and each plugin's `ffi` module as it lands. It
compiles in Swift 5 mode, because generated code isn't ours to make strict
concurrency clean. The handwritten targets use Swift 6 mode.

`Package.swift` and the handwritten sources and tests are tracked. The
XCFramework, `Sources/EmbedPDFBindings/` and every `Tests/*/Fixtures/` are
generated, and `platforms/apple/.gitignore` keeps them out of the repository.
The packaged engine ZIP is approximately 332 MiB with all four slices.
Generated bindings and binaries are verified build outputs, not reviewable
handwritten sources.

## Products, and why the generated layer isn't one

The package exports two products, `EmbedPDF` and `EmbedPDFUI`
(`platforms/apple/EmbedPDF/Package.swift:17-20`, and the repository-root `Package.swift:8-11`).
`EmbedPDFBindings`, the UniFFI-generated Swift, is a target only. Use `PDFViewer` and its typed
capabilities. For a field the typed layer doesn't cover, use the JSON-string entry points, which
take the engine's own payload: `ViewerConfigBuilder.annotation(settingsJSON:)`, the annotation
`update(_:_:)` field bag (`patchJSON`), and the form `create(draftJSON:)`, `update(_:patchJSON:)`
and `addWidget(to:placementJSON:)` overloads. Needing the raw generated API means asking us for a
typed member, not importing the bindings.

We decided against an `EmbedPDFBindings` library product, on 2026-11-10. It would make all 685
public generated types API. Every Rust change to a record, method or callback interface would then
be a break consumers see, where today only the 515 types that `EmbedPDF` re-exports are. It would
also leave the module half-usable: the linker settings (`c++`, `CoreFoundation`, `CoreGraphics`,
`Foundation`, and `AppKit` on macOS) live on the `EmbedPDFBindings` target in the source layout
(`Package.swift:34-40`), but in the binary layouts that target is a `.binaryTarget` with no
settings, and the loop that adds them (`Package.swift:116-124`) only touches `EmbedPDF`. The
root `Package.swift` declares the same settings inline on its `EmbedPDF` target (lines 22-28). A
direct importer of the binary would have to repeat them. We'll revisit this
if a typed member can't cover a real use.

This isn't a hard wall. `EmbedPDF` writes `public import EmbedPDFBindings` in the files that
re-export, so a client that imports only `EmbedPDF` sees the generated types below, and
`ProductsTests` fails if either manifest gains a `.library`. We checked by compiling a file that
imports only `EmbedPDF` and names each of the 685 generated public types unqualified:

- 515 resolve, through the same-name `public typealias X = EmbedPDFBindings.X` lines in
  `Sources/EmbedPDF/Exports.swift` and `Sources/EmbedPDF/Plugins/*.swift`. (The deprecated
  `PDFDocument` and `Annotation` aren't among them: they alias `DocumentInfo` and
  `AnnotationRecord`.) `RenderSnapshot` also resolves, but to the handwritten
  `EmbedPDF.RenderSnapshot` (`Plugins/Render.swift:58`), not the generated type.
- 169 don't: `ViewerHost`, `CancelToken`, `Subscription` and `PdfError`, 60 `*Handle` and
  `*HandleProtocol` types, 23 `*EventListener` types, 42 `*Impl` types, 15 other `*Protocol`
  types, and 25 more records and callback types such as `PdfEngine`, `RenderBatch` and
  `ViewerDelegate`.

Three of those 169 do appear in a public signature: `DocumentFetcher`, `DocumentFetchCompletion`
and `DocumentFetchCancellation`, in `PDFViewer.init(_:documentFetcher:)` and
`URLSessionDocumentFetcher.fetch(url:completion:)`. A caller can pass the default fetcher, but
can't write their own without importing the bindings module, which isn't a product. We haven't
changed that here. It needs either three more typealiases or a handwritten protocol, and either
is new API.

Could UniFFI hide the generated layer instead, so we'd get this by construction? Not with 0.32.0,
the version pinned as `=0.32.0` in `crates/uniffi-bindgen/Cargo.toml:14`, each plugin crate
(for example `crates/plugins/form/Cargo.toml:44`) and `Cargo.lock:2343-2344`. The Swift `Config`
(`uniffi_bindgen-0.32.0/src/bindings/swift/gen_swift/mod.rs:167-189`) has `module_name`,
`ffi_module_name`, `generate_immutable_records`, `omit_argument_labels`, `exclude`, `rename` and
the conformance switches, but no access-level option. The templates write `public` literally
(`RecordTemplate.swift:5,11,16`, `EnumTemplate.swift:5`, `ObjectTemplate.swift:68`,
`TopLevelFunctionTemplate.swift:1`, `Protocol.swift:2`). Hiding the types means rewriting the
generated files after `uniffi-bindgen generate`, and then `Exports.swift` and the hand-written
layer, which re-export and conform them publicly, stop compiling.

## Where the engine comes from

`Package.swift` gets the engine one of three ways, chosen by environment variables when the
manifest is evaluated:

- By default, SwiftPM downloads `EmbedPDFBindings.xcframework.zip` from the `engineRelease`
  release on `roanutil/embed-pdf-runtime` and verifies it against `engineChecksum`. Nothing
  builds Rust.
- `EMBEDPDF_PREBUILT_ENGINE=local` uses the same binary target, read from
  `EmbedPDF/EmbedPDFBindings.xcframework`, which `package-engine-framework.sh` writes.
- `EMBEDPDF_LOCAL_ENGINE=1` is the source layout above: `EmbedPDFFFI.xcframework` plus the
  generated Swift as a source target. `EMBEDPDF_EXPERIMENTAL_BRIDGE=1` implies it. It's the only
  mode with `EmbedPDFBindingsSmokeTests`, because `@testable import` can't see into a binary.

The downloaded engine needs Xcode 27.1 or later: its `.swiftinterface` was written by Swift 6.4
with `-language-mode 5`, and only the Xcode 27.1 toolchain has been checked against it.

The pin goes stale whenever a crate the engine links changes. `bash
platforms/apple/check-apple-engine.sh` hashes the tree with `apple-engine-hash.sh` and compares
the result with `engineRelease`. Exit 0 means the pin is current. Exit 1 means it's stale and
nothing is published for this tree yet. Exit 2 means the release exists and only the
`Package.swift` bump is missing. Codes 64, 69 and 70 are errors, not verdicts: bad arguments or
manifest, `gh` failing, the hash failing.

To publish:

```sh
node scripts/publish-engine-payload.mjs --dry-run --only apple
node scripts/publish-engine-payload.mjs --only apple
```

It wants a clean, committed tree, since the release notes record `HEAD`. It first checks
`APPLE_CRATES` in `apple-engine-hash.sh` against what `epdf-viewer-host` links according to
`cargo metadata`, and stops on a mismatch. On exit 1 it builds the fork's `ios-arm64` and
`ios-sim-arm64` slices (cached under `perf-compare/.work`, or `PERF_WORK_DIR`), then runs
`build-xcframework.sh` and `package-engine-framework.sh`. Then it runs the `EmbedPDF-Package`
tests against the packaged framework with `EMBEDPDF_PREBUILT_ENGINE=local`, so a broken framework
never reaches a release. Last, it creates `apple-engine-<hash>` with the zip and its `.sha256`, and
rewrites `engineRelease` and `engineChecksum`. On exit 2 it builds nothing and pins from the
release's `.sha256`. `--dry-run` does everything except create the release and rewrite the
manifest.

The test run uses the simulator named iPhone 17. When two share that name, set
`EMBEDPDF_TEST_DESTINATION='platform=iOS Simulator,id=<UDID>'`.

Either way the script leaves `platforms/apple/EmbedPDF/Package.swift` modified for you to commit.
That costs a window: on the commit the release was built from, `check-apple-engine.sh` exits 2.
It has to. The release names a commit, and that commit can't contain its own checksum.

## Plugins

| Plugin | Swift | `ViewerConfig` | Plan |
|---|---|---|---|
| metadata | `MetadataCapability` | `metadata: true` | `2026-10-01-swift-package.md` Task 6 |
| search | `SearchCapability` | `search: SearchConfig()` | `2026-10-01-swift-package.md` Task 7 |
| render | `RenderCapability` | `render: RenderConfig()` | `2026-10-01-swift-package.md` Task 8 |
| interaction | `InteractionCapability` | `interaction: true` | `2026-10-06-swift-plugins/01-interaction.md` |
| feedback | `FeedbackCapability` | `feedback: FeedbackConfig(provider: HapticFeedback())` | `2026-10-06-swift-plugins/01-interaction.md` |
| stage | `StageCapability`, `StageReaderView` (EmbedPDFUI) | `stage: StageConfig()` | `2026-10-06-swift-plugins/02-stage.md` |
| selection | `SelectionCapability` | `interaction: true, selection: true` | `2026-10-06-swift-plugins/03-selection.md` |
| link | `LinkCapability` | `interaction: true, link: true` | `2026-10-06-swift-plugins/04-link.md` |
| annotation | `AnnotationCapability`, `AnnotationFilePicker` | `interaction: true, annotation: ""` | `2026-10-06-swift-plugins/05-annotation.md` |
| history | `HistoryCapability`, `HistoryModel`, `HistoryControls` | `history: true` | `2026-11-06-swift-api-plugin-parity.md` |
| outline | `OutlineCapability`, `OutlineModel`, `OutlinePanel` | `outline: true` | `2026-11-06-swift-api-plugin-parity.md` |
| attachment | `AttachmentCapability`, `AttachmentModel`, `AttachmentPanel` | `attachment: true` | `2026-11-06-swift-api-plugin-parity.md` |
| form | `FormCapability` | `interaction: true, form: FormConfig()` | `2026-10-06-swift-plugins/06-form.md` |
| redaction | `RedactionCapability` | `interaction: true, annotation: "", redaction: RedactionConfig()` | `2026-10-06-swift-plugins/07-redaction.md` |
| measurement | `MeasurementCapability` | `interaction: true, annotation: "", measurement: MeasurementConfig()` | `2026-10-06-swift-plugins/08-measurement.md` |
| stamp | `StampCapability`, `FileStampLibraryStore` | `stamp: StampConfig()` (with `interaction: true, annotation: ""` to place) | `2026-10-06-swift-plugins/09-stamp.md` |
| page-edit | `PageEditCapability` | `pageEdit: true` | `2026-10-06-swift-plugins/10-page-edit.md` |
| view-manager | `ViewManagerCapability` | `viewManager: true` | `2026-10-06-swift-plugins/11-view-manager.md` |
| shell | `ShellCapability` | `shell: true` | `2026-10-06-swift-plugins/12-shell.md` |
| i18n | `I18nCapability`, `BundleLocaleLoader` | `i18n: I18nConfig(locales: [...], lazyLocales: [...], loader: BundleLocaleLoader())` | `2026-10-06-swift-plugins/13-i18n.md` |
| commands | `CommandsCapability`, `CommandHandler` | `commands: true` | `2026-10-06-swift-plugins/14-commands.md` |
| actions | `ActionsCapability`, `JavaScriptCoreEngineFactory` | `actions: .forCurrentTimeZone(javascript: ActionsJavaScriptConfig(enabled: true, engine: JavaScriptCoreEngineFactory()))` | `2026-10-06-swift-plugins/15-actions.md` |
| signature | `SignatureCapability`, `KeychainSigner`, `TrustAnchors` | `interaction: true, form: FormConfig(), signature: SignatureConfig()` | `2026-10-06-swift-plugins/16-signature.md` |

A plugin left out of `ViewerConfig` is refused at its accessor. `search(in:)`, `stamps()` and the
other accessors throw `PDFError` with code `.unsupported`, even for a document that is closed or
still loading, so a capability you hold has its own plugin. `DisabledPluginTests` checks every
accessor and fails when `ViewerConfig` gains a plugin without a row.

A few calls also use a companion plugin and refuse on each call, with `.unsupported`, when it is
left out. `RedactionCapability.markSelection()` needs `selection` and `markMatches(_:on:)` needs
`search`. `StageCapability.registerScrollHandler(panFallback:)` needs `interaction`.
`SignatureCapability.place(_:at:)` with a `.free` target needs `stamp` for an `.asset` mark and
`annotation` for a `.source` mark. Stamp placement without `annotation` throws
`.permissionDenied` instead, and `canPlace(in:)` reads `false`. Every other optional companion
is skipped without an error.

> Note: `JavaScriptCoreEngine` enforces a script's time budget by abandoning the realm, because
> JavaScriptCore has no public execution limit. A runaway script keeps one thread busy until the
> app exits. Memory and stack budgets aren't enforced. JavaScript on a real document also needs
> the form and annotation plugins (plans 05 and 06), whose engine paths the script world reads.
>
> Note: trust anchors are the app's own, set with `PDFViewer.setTrust(_:)`. iOS can't list the
> system roots (`SecTrustCopyAnchorCertificates` is macOS only), and validating against the system
> store would need a `SecTrust`-evaluating port in `epdf-signature-core`. Without anchors a good
> signature reads `.validUntrusted`. Document permissions follow `ViewerConfig.scope`; `nil` grants
> everything.

### Building a `ViewerConfig` with `ViewerConfigBuilder`

`ViewerConfig(search: SearchConfig(), ...)` keeps working. Its memberwise initializer gains a
parameter with every plugin we add, though, so we also ship a chained builder, one method per
field, each returning a changed copy:

```swift
let viewer = try PDFViewer(
  ViewerConfigBuilder().interaction().annotation(settingsJSON: "").search(SearchConfig()))
```

An unset field is the engine's default, so `ViewerConfigBuilder().build()` equals `ViewerConfig()`:
every plugin left out and `scope` at `nil`. `PDFViewer.init(_:documentFetcher:)` takes either
spelling. `ViewerConfigBuilderTests` fails when `ViewerConfig` gains a field the builder lacks.
The cost is a second spelling to keep in step, which is what that test is for.

## What the script assembles

Per slice, `libtool -static` merges `libepdf_viewer_host.a` with the fork's
`libembedpdf.a` into one archive, because `xcodebuild -create-xcframework` takes
one `-library` per platform. The fork halves come from
`packages/engine/runtime/build/libpdfium/{ios-arm64,ios-sim-arm64}`, put there by
`scripts/fetch-libpdfium.sh` reading `engine-runtime-build.local.json`. There is
no download: those entries are `file://` URLs into a local fork checkout, per
the standing decision not to cut fork releases.

The headers and the Swift are regenerated from the built library on every run
rather than checked in. uniffi's proc macros are the only thing that knows the
current shape of the API, and a hand-kept copy drifts without saying so.

Bindgen runs in library mode against the simulator's static
`libepdf_viewer_host.a`, and uniffi 0.32 reads the metadata straight out of the
archive, so no dylib is needed. It writes one `*FFI.h` and one
`*FFI.modulemap` per crate, and the script concatenates the maps into the
single `module.modulemap` the XCFramework wants. A crate only shows up if the
archive links it: rustc leaves an unreferenced dependency out of a staticlib,
which is why `crates/epdf-viewer-host/src/lib.rs` names `extern crate
epdf_uniffi`. Without that line the bindings come out with no `PdfEngine` and
the smoke tests don't compile.

## Observing viewer state

`PDFViewer.observeStateRevisions()` returns a new subscription on each call.
It installs synchronously, keeps the newest revision, and finishes on viewer
destruction. Subscribe before reading the initial snapshot so a change during
that read is queued. Revisions invalidate reads; they are not an event log and
there is no initial revision emission. Cancelling a consuming task cancels only
that subscription. The stored `stateRevisions` property supports one consumer
and is deprecated in favor of this method.

`PDFViewer.observeReportedErrors()` follows the same rule for errors the kernel
reports rather than throws. Each call is its own subscription that keeps the
newest 16 errors and sees only errors reported after it returns, so subscribe
before the call that might report one. The stored `reportedErrors` property
supports one consumer and is deprecated.

```swift
let revisions = viewer.observeStateRevisions()
var rows = try await outline.rows()
for await _ in revisions {
  rows = try await outline.rows()
}
```

Models and views should own the consuming task for their visible lifetime.
Each panel may subscribe independently; no panel needs to forward revisions to
another panel.

## What the generated Swift gives the facade

Records conform to everything the facade needs to re-export them. uniffi 0.32
declares `public struct DocumentInfo: Equatable, Hashable` and
`public struct PageRef: Equatable, Hashable`, each followed by
`extension DocumentInfo: Sendable {}` inside `#if compiler(>=6)`. Every record
and enum in the three generated files has the same shape, so the facade needs
no `@retroactive @unchecked Sendable` lines.

Cancelling a Swift `Task` does not cancel the Rust call. The generated
`uniffiRustCallAsync` awaits each poll in a bare `withUnsafeContinuation`, with
no `withTaskCancellationHandler` around it, and only calls `freeFunc(rustFuture)`
from a `defer` once the future reports ready. The headers declare the
`ffi_*_rust_future_cancel_*` functions, but the generated Swift never calls
them. So the Rust future runs to completion, and abort-on-drop in `epdf-ffi`
never fires from Swift. Task 7's `searchCancelsWhenTheTaskIsCancelled` therefore
needs the plan's fallback: a `CancelToken` object passed to every
promise-returning export and cancelled from `withTaskCancellationHandler` in the
facade's `bridged`. The cost is one extra parameter on every such export.

## The smoke test

`Tests/EmbedPDFBindingsSmokeTests/SmokeTests.swift` asserts exactly what
`crates/epdf-uniffi/tests/native_surface.rs` asserts, deliberately. The Rust
test already covers the logic on the host; what only an iOS process can say is
that the XCFramework links, loads and runs there against the simulator slice of
the fork's PDFium. Keeping the assertions identical means a divergence between
the two is a distribution problem and nothing else.

Results, 2026-09-23, Xcode 27.0, iPhone 17 simulator (iOS 26.0): 8 tests, 0
failures, 0.28s.

Results, 2026-10-01, with the archive built from `epdf-viewer-host`: 8 tests, 0
failures, 0.60s.

This is a test bundle rather than an app with a screen. A bundle runs on the
simulator's real iOS runtime through the same dyld path an app uses, and it
runs unattended, which an app with a button does not. What it does not cover is
a physical device: code signing and the hardened runtime are only exercised
there, and Phase 1's device run still needs a device and a signing identity.

## One thing this shook out

`PdfError` carried `#[uniffi(flat_error)]`. Flattening keeps the fields on the
Rust side and lowers only the `Display` string, so `native_surface.rs` went on
matching `PdfError::Engine { code, .. }` while Swift received `case
Engine(message: String)` and no `code` at all. Every facade contract that says
"match on the code and prompt for a password" was unimplementable, and nothing
in the Rust test suite could notice. Removing the attribute gives Swift `case
Engine(code: String, detail: String)`.

The cost of removing it is that every field on every error variant now has to be
an FFI type, so an error can no longer carry an arbitrary Rust payload. That is
the right trade here and worth knowing before adding a variant.

## What `import EmbedPDF` exposes

`EmbedPDF` re-exports the generated records through one `public typealias` each
in `Exports.swift`, and Swift rejects that over an `internal import`: with
`internal import EmbedPDFBindings` the compiler reports "type alias cannot be
declared public because its underlying type uses an internal type", and a public
signature in `PDFViewer.swift` that uses an alias reports "'DocumentInfo' aliases
'EmbedPDFBindings.DocumentInfo' and cannot be used here because
'EmbedPDFBindings' was not imported publicly". So `Exports.swift` and
`PDFViewer.swift` use `public import EmbedPDFBindings`, and every other file
keeps `internal import`.

The cost is that the generated records, enums and callback protocols
re-exported by `Exports.swift` and `Plugins/*.swift` become API. `ViewerHost` and the plugin
handles don't resolve from `import EmbedPDF` alone; see "Products, and why the generated layer
isn't one". The supported API is
`PDFViewer` and the capabilities; anything else from the bindings module is
unsupported and can change with every regeneration.

The document record is `DocumentInfo`, the name the bindings give it. It was
`PDFDocument` until 2026-10-16, which PDFKit also declares, so any file that
imported both modules got "'PDFDocument' is ambiguous for type lookup in this
context". `PDFDocument` is still there as a deprecated typealias, removed in
0.0.0-swift.6. The deprecated alias collides exactly as before: it keeps old code
compiling only in files that don't import PDFKit.

The annotation read record is `AnnotationRecord` for the same reason. It was
`Annotation`, and MapKit's SwiftUI overlay (iOS 17) declares a generic
`Annotation` view, so a file importing EmbedPDF, SwiftUI and MapKit could not
write `[Annotation]`. This one was renamed at the source,
`crates/plugins/annotation/src/ffi/mod.rs`, so the generated Swift and Kotlin
both say `AnnotationRecord`. The deprecated `Annotation` alias has the same
limit as `PDFDocument`.

Reads no longer hand out `AnnotationRecord` itself. `get(_:)`, `list(_:)`,
`selected()` and `hitTest(at:on:)` return `AnnotationInfo`, a hand-written
struct over it with `Date` timestamps and typed ink, highlight and note
members, and `AnnotationInfo.record` gives the generated record back.

## Moving from 0.0.0-swift.4

Most renames since `0.0.0-swift.4` keep the old spelling as a deprecated
shim, so the compiler names the replacement. These changes have no shim and
need an edit:

- `PDFError.code` is a non-optional `PDFError.Code`, not `String?`.
  `error.code == "not-found"` still compiles, because a string literal is a
  code, and `if let code = error.code` doesn't. `error.code == nil` compiles
  with only a warning and is always false, so a branch behind it silently
  never runs: delete it. Prefer `catch PDFError.Code.notFound`.
- `AnnotationInfo`, the new read result, types what `AnnotationRecord` kept
  as text: `createdAt` and `modifiedAt` are `Date?` (the engine's text is
  `rawCreatedAt` and `rawModifiedAt`), `subtype` is an `AnnotationSubtype`,
  and ink, highlight and note colours are `AnnotationColor`. Code that names
  `AnnotationRecord` as the result type still compiles against a deprecated
  overload.
- Counts and indexes are `Int`. An untyped `let count = try await
  viewer.documentCount()` is now an `Int`, so arithmetic that mixed it with
  `UInt64` or `UInt32` needs a conversion. A value out of the engine's range
  throws `.invalidInput` instead of trapping.

The deprecated spellings, with their replacements:

| Deprecated | Use |
|---|---|
| `PDFViewer.stateRevisions`, `reportedErrors` | `observeStateRevisions()`, `observeReportedErrors()` |
| `PDFError(name:code:detail:)` with a `String?` code | `PDFError(_:detail:name:)` |
| `allows(_:in:)` with a `String` | a `DocumentPermission`, or a string literal |
| tool ids as `String` | a `ToolID`, or a string literal |
| annotation subtypes as `String` (`createMarkup`, `previewMarkup`) | an `AnnotationSubtype`, or `AnnotationSubtype(_:)` for a string |
| `activeToolId()`, `defaultToolId()` | `activeToolID()`, `defaultToolID()`, which return a `ToolID` |
| `reorder(_:to:)` with a JSON string | an `AnnotationPosition` |
| `Bool?` parameters whose `nil` meant the engine default | a plain `Bool`, or a `ContentInclusion` where the default follows the session's scope |
| `UInt32`, `UInt64` and `Int64` counts and indexes | `Int` |
| `SearchModel.activeIndex` | `activeHitIndex` |
| `StageReaderModel.currentPageIndex` | `pageIndex` |
| `AnnotationInfo.fieldsJson`, `groupId` | `fieldsJSON`, `groupID` |
| `get(_:)`, `list(_:)`, `selected()` and `hitTest(at:on:)` returning `AnnotationRecord` | the overloads returning `AnnotationInfo`; `AnnotationInfo.record` gives the record |

The two renamed types go in 0.0.0-swift.6, and their deprecation messages say so:

| Deprecated | Use | Removed in |
|---|---|---|
| `PDFDocument` | `DocumentInfo` | 0.0.0-swift.6 |
| `Annotation` | `AnnotationRecord` | 0.0.0-swift.6 |

Every other shim above is removed no earlier than 0.0.0-swift.6. The rule is
that a deprecated shim survives at least one tagged release: 0.0.0-swift.5
ships with all of them in place, so a project can build against it, read the
warnings and move over before anything is deleted. We haven't picked a release
for the overloads beyond that floor. `AliasRemovalTests` fails if `PDFDocument`
or `Annotation` is deleted early or stops naming its release.

## Names that overlap Apple frameworks

We compiled a probe that imports `EmbedPDF` and an Apple framework and uses
each unqualified public type name in a type position. It covered the 600
top-level public names `EmbedPDF` and `EmbedPDFUI` declare or re-export,
including `Rect`, `Point`, `Size`, `PageInfo`, `Quad` and `Camera`, and the 687
further public names in `EmbedPDFBindings`, which `Exports.swift` imports
publicly. The frameworks were Foundation, CoreGraphics, CoreImage, CoreText,
SwiftUI, PDFKit, AppKit, UIKit (iOS 17 simulator SDK), MapKit, Combine,
Observation, UniformTypeIdentifiers, Security and Vision, then SwiftUI, MapKit,
PDFKit and AppKit together. The probe ran on Xcode 27.2 (Swift 6.4.2) against
a stub module that declares the same names, so it tests name lookup and not
the types themselves.

| Name | Framework | What happens | Workaround |
|---|---|---|---|
| `PDFDocument` (deprecated alias) | PDFKit | ambiguous for type lookup | `EmbedPDF.PDFDocument`, or use `DocumentInfo`; removed in 0.0.0-swift.6 |
| `Annotation` (deprecated alias) | MapKit with SwiftUI (iOS 17, macOS 14) | ambiguous for type lookup | `EmbedPDF.Annotation`, or use `AnnotationRecord`; removed in 0.0.0-swift.6 |
| every other name, including `Rect`, `Point`, `Size`, `PageInfo` | none | no clash | none needed |

No name collides with the Swift standard library. `Point`, `Size` and `Rect`
are not CoreGraphics' `CGPoint`, `CGSize` and `CGRect`, so they never meet
them. A clash only shows when a file imports both modules. When one does, write
the module name: `EmbedPDF.Rect`.

## The demo app

`EmbedPDFDemo/` is a small SwiftUI app over the package. It opens `report.pdf` on launch,
uses `StageReaderView` for Rust-driven scrolling and navigation, searches with
`SearchModel`, and shows standard/custom document metadata. It includes annotation
creation with undo/redo, bookmark navigation/editing, and attachment import/export
inspectors. Native Files sharing exposes its Documents folder. The
open menu takes a PDF from Files or one of two bundled samples; `encrypted-hello.pdf`
prompts for its password, which is `hôtel`.

The Xcode project is generated by Tuist from `Project.swift` and isn't checked in.
`mise.toml` pins the Tuist version. Build the XCFramework first, then:

```sh
cd platforms/apple/EmbedPDFDemo
tuist generate
```

It signs ad hoc, which is enough for the simulator. A device run needs a development team
in `Project.swift`.

## Profiling the demo app

`EmbedPDF` emits `os_signpost` intervals under Points of Interest (subsystem
`com.embedpdf`): `open`, `pages`, `search`, `hits`, and `renderPage` split into
`renderPage.rust` (the call into Rust, including the kernel and engine threads) and
`renderPage.cgImage` (wrapping the pixels). They show up in any Instruments template that
has the Points of Interest track, Time Profiler included.

`EmbedPDFDemoUITests` is the scenario: it flings through `report.pdf` and back, searches
for "trace" and steps through ten hits. With `TEST_RUNNER_EMBEDPDF_PROFILE_DELAY=10` it
waits ten seconds after the document appears, long enough to attach Instruments to the
app's process. `scripts/profile.sh` does both:

```sh
cd platforms/apple/EmbedPDFDemo
tuist generate
xcodebuild build-for-testing -workspace EmbedPDFDemo.xcworkspace -scheme EmbedPDFDemo \
  -destination 'platform=iOS Simulator,name=iPhone 17' -derivedDataPath .build/dd
scripts/profile.sh "Time Profiler" time.trace
```

Attach to the process; don't record all processes. An `--all-processes` recording on the
simulator could not be symbolicated, and `XCUIApplication.activate()` relaunches an app
Instruments launched, so the traced process is not the one the test drives.

The simulator runs on the Mac's CPU and GPU. Trust its CPU profiles, allocations and
signpost durations; don't trust its frame times or hitch counts.

## The Nutrient comparison app

`NutrientDemo/` is the same app over Nutrient (PSPDFKit) 26.11.0: the same views,
accessibility identifiers, UI test and signpost names, with `renderPage.engine` standing in
for `renderPage.rust`. It renders through Nutrient's render queue with its default cache
policy and searches with `TextSearch`. It runs without a license key, so Nutrient draws its
evaluation watermark on every page. Generate it with `tuist generate` in `NutrientDemo/`
and profile it with `NutrientDemo/scripts/profile.sh`, exactly as above.

Nutrient keeps a disk cache of rendered pages across launches. Uninstall the app from the
simulator before a measured run (`xcrun simctl uninstall <sim> com.embedpdf.nutrientdemo`),
or its renders are mostly cache hits and the comparison is meaningless.


## History, bookmarks and embedded files

Construct one set of models per open document, and retain them in your host's
observable state. Each control owns its observation with `.task`; changing its
model identity cancels the old subscription. All three can observe one viewer
without consuming each other's revisions. The deprecated `stateRevisions`
property is a single-consumer stream; create `observeStateRevisions()` for every
independent observer and subscribe before reading its initial snapshot.

```swift
import EmbedPDF
import EmbedPDFUI
import SwiftUI

@MainActor
struct PluginReader: View {
  let reader: StageReaderModel
  let history: HistoryModel
  let outline: OutlineModel
  let attachments: AttachmentModel

  var body: some View {
    TabView {
      StageReaderView(model: reader)
        .tabItem { Label("Reader", systemImage: "doc") }
      OutlinePanel(model: outline)
        .tabItem { Label("Bookmarks", systemImage: "bookmark") }
      AttachmentPanel(model: attachments)
        .tabItem { Label("Attachments", systemImage: "paperclip") }
    }
    .safeAreaInset(edge: .bottom) {
      HistoryControls(model: history).padding()
    }
  }
}

// In the host's document-open task:
let viewer = try PDFViewer(ViewerConfig(
  render: RenderConfig(), interaction: true, stage: StageConfig(),
  annotation: "", history: true, outline: true, attachment: true))
try await viewer.start()
let document = try await viewer.open(pdfData)
let screen = PluginReader(
  reader: StageReaderModel(stage: try await viewer.stage(for: document),
    render: try await viewer.render(in: document)),
  history: HistoryModel(viewer: viewer, history: try await viewer.history(for: document)),
  outline: OutlineModel(viewer: viewer, outline: try await viewer.outline(for: document)),
  attachments: AttachmentModel(viewer: viewer, attachments: try await viewer.attachments(for: document)))
```

Enable annotation support to include page file-attachment annotations alongside
document embedded files. Bookmark activation reveals through the installed stage;
`OutlinePanel` also accepts `onActivation` for results the host must handle, such
as external addresses. Swift does not calculate destinations or rebuild trees.
An omitted outline patch property preserves it; use `clear: [.target, .color]`
to remove those values explicitly. Unknown PDF action data remains on the item.

History undo/redo returns when submitted, not when confirmed. Observe
`HistoryCapability.events` for completion/refusal and skipped parts. Empty
history can return without an event; `HistoryControls` clears its submission
indicator as soon as the call returns. Configure its global limit through
`updateSettings`; optional `I18nCapability` translates change labels in the model.

Attachment inputs and downloads use owned `Data`, preserving zero-byte files,
Unicode names and MIME metadata. File selection reads off the main actor and
balances security-scoped URL access. Export begins only after an explicit Save
copy action and removal asks for confirmation. Arbitrary attachment bytes export
as the FileDocument's declared `.data` type, with the original suggested filename
and extension. The API retains MIME metadata; it never executes a downloaded file.

Capability calls preserve `PDFError` codes and cooperative cancellation. Close
finishes document subscriptions; destroy finishes viewer revision subscriptions.
Cached snapshots in controls refresh on revisions/events, and a generation guard
rejects stale or cancelled reads. Reopening a document creates new capabilities
and models. Panel editor, confirmation and file-operation state is scoped to
model identity, so swapping models disposes pending presentations and tasks.
`StageReaderModel` observes Rust render invalidations; `StageReaderView` passes
per-page revisions to `PageImageView` so document edits, undo and redo refresh
visible pixels. Direct `PageImageView` hosts can pass its defaulted `revision`
parameter when their page is invalidated. The host owns `close`, `destroy`, and
handling locked tabs/passwords.

## Audited API coverage

`swift-api-coverage.json` accounts for 860 public members across 31 Rust plugin-directory
crates and the kernel: 724 mapped members and 136 explicit exclusions. The nine
pure libraries are classified as owner-consumed code without fictional plugin APIs.
The checker inventories public kernel `Documents` operations and `Kernel` methods,
including viewer settings; internal kernel types are classified separately.
These are accounting counts, not an implementation parity percentage or a count
of Swift methods. Open form payload overloads are checked alongside typed conveniences,
matched by Swift argument label so removing one overload cannot hide behind the other.

Delivered workflows include document lookup/order/retry/URL loading and layer
save/open, live settings/read/update/reset for 13 plugins, command disabling and
refusal details, named-action permission checks, annotation bundle import/export
and selected-link edits, independent form widget edits/deletion and calculation
order, snapshots with cancellable owned images, and native zoom boxes. Rust owns
permissions, registration reset values, geometry and document lifetime.

Form authoring keeps its typed convenience methods and accepts the engine's open
payloads through `create(draftJSON:)`, `update(_:patchJSON:)` and
`addWidget(to:placementJSON:)`. A patch's omitted members preserve values;
explicit null removes nullable values. Field script writes use
`{"type":"javascript","script":"event.value = 7"}` inside their event,
rather than the read tree's `root` wrapper. `fieldSettings(_:)` returns typed
family-specific panel descriptors and current values JSON. Malformed JSON or a
non-object payload refuses with `invalid-input` before mutation.

Remaining exclusions name concrete resources or host integrations in the inventory:
Rust caches and ownership handles, painter/appearance and text-editor/pointer
presentation seams, scripting sibling ports, equivalent JSON settings forms, and
startup registration helpers. Printing still requires a native `HostPrinter`
with prepared PDF/picture representations and disposal lifetime; it is not
implemented. Observe current settings through `observeStateRevisions()` and a
fresh settings read. Native rendering uses owned images and render view demand.

The [validation report](../../docs/superpowers/reports/2026-11-09-swift-app-api-coverage.md)
records exact source-engine results. Source commits do not publish an engine:
matching downloaded artifacts and both package pins still need a separate release.

```sh
python3 platforms/apple/check-swift-api-coverage.py
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s platforms/apple/tests -p test_swift_api_coverage.py
```

For local binary verification, first run `package-engine-framework.sh` after
building the source engine, then run the same package tests with
`EMBEDPDF_PREBUILT_ENGINE=local` and `EMBEDPDF_LOCAL_ENGINE` unset. This verifies
the compiled Swift module in addition to the Rust archive. The release workflow
owns release names, hashes and checksums:

```sh
node scripts/publish-engine-payload.mjs --dry-run --only apple
```

The dry run builds and verifies a concrete payload without publishing or changing
engine pins. A real release and updating both manifests are separate authorized
publishing work. Never attach a new checksum to the old release tag.
