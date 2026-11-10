# Swift API idioms plan

The Swift package works, but parts of it read like the wire format instead of Swift. Callers build
JSON strings by hand, compare error codes as strings, convert `Int64` at every call site, and await
a kernel hop to read a value that changed a moment ago. This plan lists the changes we want to make
to the public surface of `platforms/apple/EmbedPDF`, in the order we'd do them.

Nearly all of it lands in the hand-written layer, `Sources/EmbedPDF/` and `Sources/EmbedPDFUI/`.
That layer ships with a Swift build and needs no engine rebuild or new `apple-engine-*` release. The
one exception is called out in item 12. Receipts point at this branch as of a82d0555e.

## Constraints

Keep the iOS 17 and macOS 14 floors and the existing package dependencies. Every rename or signature
change keeps the old spelling as an `@available(*, deprecated, renamed:)` shim for at least one
tagged release, the way `PDFDocument` and `Annotation` are handled today (`Exports.swift:7`,
`Plugins/Annotation.swift:18`). The JSON-string entry points stay as the escape hatch for fields the
typed layer doesn't cover. No change here alters what crosses the UniFFI boundary.

## Phase 1: Swift-only fixes with no design questions

These are self-contained, and each one is about one to two hours with tests.

1. **`PDFError.Code`.** `PDFError.code` is `String?` (`PDFError.swift:34`), but the doc comment
   above it says nothing the viewer throws leaves it `nil`. Let's add a `Code` enum for the nine
   kernel codes plus the bindings' `invalid-state` (`PDFError.swift:4-24`), with an
   `unknown(String)` case so a code the kernel adds later still round-trips. `code` becomes
   non-optional `Code`, and `init(name:code:detail:)` takes `Code`. Handlers and ports that pass
   their own string keep a deprecated `String?` initializer. Cost: a source break for anyone who
   compares `error.code == "not-found"`, softened by the shim.

2. **One stream rule.** `observeStateRevisions()` (`PDFViewer.swift:23`) already gives every caller
   its own subscription. The stored `stateRevisions` and `reportedErrors` (`PDFViewer.swift:14`,
   `:17`) support one consumer each, and a second `for await` silently gets nothing. Let's add
   `observeReportedErrors()` on the same `StateRevisionHub` pattern and deprecate both stored
   streams. `documentEvents` and `SearchCapability.events` already subscribe per access through
   `StreamRegistry`, so they stay.

3. **No `Bool? = nil` parameters.** `canSearch(snippets:)` and `findAll(_:snippets:)`
   (`Plugins/Search.swift:60`, `:157`), `create(_:on:tool:select:)`
   (`Plugins/Annotation.swift:154`) and `createFromSelection(tool:clear:)` (`:438`) use `nil` to
   mean the engine default. Let's write the engine's default into the signature as a plain `Bool`.
   Where the default depends on settings the caller can change, an options struct with a documented
   default is the better fit. Cost: we have to look up and pin each engine default, and a later
   engine change to a default becomes a Swift change.

## Phase 2: typed values at the surface

4. **`Int` at the public surface.** `page(at: Int64)`, `index(of:) -> Int64`, `move(_:to: Int64)`
   and `documentCount() -> UInt64` (`PDFViewer.swift:105-149`), plus `goToHit(_: UInt32)`,
   `hitCount(on:) -> UInt32` and `activeHitIndex() -> UInt32?` (`Plugins/Search.swift:103-137`),
   make every SwiftUI call site convert. Let's expose `Int` and convert in the wrapper. Nothing
   traps: an out-of-range value throws `invalid-input`. About a day, across every capability.

5. **Enums for closed string sets.** `reorder(_:to positionJSON:)` (`Plugins/Annotation.swift:176`)
   takes one of `front`, `back`, `forward`, `backward`, or `{"above": ref}` / `{"below": ref}`, all
   as a JSON string. That's a closed set, so it becomes an `AnnotationPosition` enum and is the
   first one to do. After it come annotation subtypes (`AnnotationRecord.subtype`,
   `createMarkup(_ subtype:quadsJSON:on:preset:)` at `:398`), the `allows(_ capability: String)`
   capability names (`PDFViewer.swift:158`), and tool IDs. The open sets get an `unknown(String)`
   case. About a day.

6. **Typed annotation records.** `AnnotationRecord` comes straight from the generated bindings with
   `createdAt: String?`, `modifiedAt: String?`, UniFFI casing (`groupId`, `fieldsJson`) and the rest
   of the record as a JSON string (`epdf_plugin_annotation.swift`, `public struct AnnotationRecord`).
   Let's put a hand-written struct in front of it with `Date?`, `groupID` and typed fields for the
   common subtypes, keeping `fieldsJSON` for the rest. The name must not be `Annotation`, which is
   the deprecated alias in item 11.

7. **Typed annotation inputs.** `create(_ fieldsJSON:on:tool:select:)`, `update(_:_ patchJSON:)` and
   `updateDefaults(_:for:)` (`Plugins/Annotation.swift:154`, `:162`, `:308`) take field bags as
   strings. Callers end up building `[String: Any]` and running it through `JSONSerialization`, so
   a typo in a key name compiles and fails at runtime, if it fails at all. Let's add `Encodable`
   input and patch types for ink, highlight and note first, encoding to the exact bytes the engine
   already reads, with the `String` overloads kept. Cost: this is the largest item, about two to
   three days with tests, and each typed model is a promise to keep in step with the engine's
   annotation schema as the plugin changes. We'd add more subtypes only when a caller asks.

## Phase 3: reads and configuration

8. **Reads as observable properties.** Search `status()`, `activeHitIndex()`, `hitCount(on:)` and
   `progress()` (`Plugins/Search.swift:117-145`), annotation `canRead()` and `canCreate()`, and the
   viewer's `activeDocument()` and `documentCount()` are each an `async throws` hop to the kernel
   thread. A view that wants the active hit has to schedule a task to read it. Let's add these as
   stored properties on `@MainActor @Observable` models in `EmbedPDFUI`, next to `SearchModel`
   (`EmbedPDFUI/SearchModel.swift:11-12`), refreshed from `observeStateRevisions()`. The async
   methods stay for callers outside the UI module. Cost: a cached value is one revision behind the
   kernel until the refresh lands, and the docs have to say so.

9. **Report a disabled plugin once.** Capability accessors such as `search(in:)`
   (`Plugins/Search.swift:183`) already exist, but a plugin left out of `ViewerConfig` only shows up
   as `unsupported` on each later call. Let's check the plugin at the accessor and throw
   `unsupported` there, so code that holds a capability never has to handle that case per call.
   This depends on whether `ViewerHost` can answer "is this plugin configured" without a document;
   if it can't, this item needs a Rust export and moves to phase 4.

10. **A Swift builder for `ViewerConfig`.** `ViewerConfig` is the generated struct re-exported as-is
    (`Exports.swift:3`), with one field per plugin and a mix of `Bool`, optional config structs and
    `String?`. Every new plugin field breaks callers' memberwise initializers. A chained builder
    (`ViewerConfig().search(.init()).annotations(settings)`) or a hand-written struct with defaults
    keeps those callers compiling.

## Phase 4: names and packaging

11. **Finish the alias removals.** `Annotation` and `PDFDocument` are deprecated
    (`Plugins/Annotation.swift:18`, `Exports.swift:7`) but have no removal date. Let's name the
    tagged release that deletes them and audit the other generic one-word names (`Rect`, `Point`,
    `PageInfo`) for collisions with Apple's frameworks.

12. **Decide on an `EmbedPDFBindings` product.** Today `Package.swift` exports only `EmbedPDF` and
    `EmbedPDFUI`, so a third party who wants the raw generated types has to fork. Exposing it is one
    `.library` line, but it makes the whole UniFFI surface public API, and every Rust change to a
    record, method or callback interface becomes a break consumers see. The remote-mode linker
    settings are also added to the `EmbedPDF` target only (`Package.swift`, the loop after
    `usesLocalEngine`), so a direct importer of the bindings may need them too. We recommend not
    exposing it yet, and saying in `platforms/apple/README.md` that the generated layer is internal.

13. **Task-named entry points over engine-named plugins.** `Stage`, `Shell`, `Interaction` and
    `ViewManager` (`Sources/EmbedPDF/Plugins/`) name engine parts, not what a caller wants to do.
    Thin façades named after tasks, delegating to the plugins, would help discovery. This comes
    last because it adds surface we then have to maintain, and the plugin names stay public as the
    escape hatch.

14. **Synchronous capability accessors.** Every accessor (`annotations(for:)`, `search(in:)`,
    `stage(for:)`, and the rest) is `async throws`. If item 9 moves the configuration check to a
    place that doesn't need the kernel, getting a capability can become a plain call. This is a
    breaking signature change with no clean deprecation path, so it waits for a major version.

## Open questions

We haven't checked whether UniFFI 0.32.0 can emit the generated types as `internal`, which would let
item 12 hide them instead of just not exporting them. We also haven't confirmed the engine defaults
for the `Bool?` parameters in item 3; those need reading from the plugin crates before the change.
Nothing in this plan has been built yet.
