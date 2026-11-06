# Swift API and plugin coverage

Status: approved for implementation by the user on 2026-11-06; merge remains subject to user review.

Base: `research/multi-platform-rust-core-impl-plugins`, commit
`8fa01cc1ec4d0739af535feec6928e6a81163fd9`.
Planning branch: `research/swift-api-plugin-parity-plan`.

## Intent

Make the Apple package a complete, usable public API for the Rust viewer's
plugins, with SwiftUI controls for the newly exposed capabilities. The user
explicitly requested API coverage plus SwiftUI controls. Existing API names and
the separation between `EmbedPDF` and `EmbedPDFUI` remain the starting point.

This follows the completed October 6 Swift plugin plans. Those plans covered
the plugins present then; their checked boxes do not establish current parity.

## Current evidence

`crates/epdf-viewer-host/src/api.rs` already exposes `history`, `outline`, and
`attachment` booleans. `session.rs` installs their Rust plugins and Cargo links
them. None has a UniFFI handle, host accessor, handwritten Swift capability, or
SwiftUI control. Enabling a plugin is therefore not equivalent to exposing it.

The native translator already supports `outline.get`, `attachments.list`,
`attachments.download`, and `apply`. New wrappers must exercise these paths and
the mutation/event pipeline before adding any alternative engine routes.

### Inventory of all 30 crates under `crates/plugins`

| Crates | Current Apple coverage | Action |
| --- | --- | --- |
| history, outline, attachment | Config/installation only | Add complete document capabilities and UI |
| metadata, form, stage, page-edit, redaction, signature, stamp, link, interaction | Swift capabilities exist; verified method gaps below | Complete wrappers and tests |
| actions, annotation, commands, i18n, measurement, render, search, selection, shell, view-manager | Swift capabilities exist | Audit public controller members against exported handles and Swift; preserve deliberate host-only exclusions |
| annotation-core, form-core, signature-core, stage-core | Pure cores consumed by Rust plugins | Keep semantics in Rust; expose their data through owning capabilities |
| acrojs, fonts, geometry, ui-core | Shared libraries rather than kernel plugins | No fictitious plugin switches; expose only data needed by a supported Apple workflow |

Feedback is supplied by the interaction crate and already has its own Swift
capability and platform provider. Include it in the member audit without
counting it as a separate Rust crate.

### Verified omissions in existing Swift wrappers

These are public exported handle members present in Rust and absent from their
Swift wrappers. Constructors, converters, internal helpers and renamed wrapper
methods are excluded.

| Swift capability | Additions (proposed Swift spelling) |
| --- | --- |
| Metadata | `customSnapshot()`, `customStatus()`, `updateCustom(_:)`, `refreshCustom()` |
| Form | `validate()`, `canRestoreAttribution()`, `canWriteScripts()` |
| Stage | `pageCount()`, `currentDestination()`, `goToDestination(_:behavior:)`, `canZoomIn()`, `canZoomOut()` |
| PageEdit | `selectedPages()`, `select(_:)`, `canExtract()` |
| Redaction | `canUnmark(_:)`, `canUpdateLabel(_:)` |
| Signature | `canReadRevision()` |
| Stamp | `canCreateFromAnnotations(in:)` |
| Link | `status(for:)` |
| Interaction | `hasCursorClaim()` |

Form's escaped Swift `import` method already exists. Feedback methods live in
`Feedback.swift`. Neither should be reported missing by a name-only scan.

## Approaches

| Approach | Benefit | Tradeoff |
| --- | --- | --- |
| Extend typed handles and capabilities (recommended proposal) | Fits existing ownership, errors, cancellation, and tests; callable from Swift without JSON | Requires per-plugin boundary records and tests |
| Expose generic string/JSON dispatch | Fast initial reach into Rust | Leaves type safety, bytes, optional fields, and event decoding to callers |
| Rebuild plugin state and commands in Swift | Native-looking implementation | Duplicates document policy and breaks the repository's single-owner rule |

The recommendation is an agent proposal, not a recorded user choice. The plan
uses it to make the proposal concrete and reviewable; no product code is built
before review.

## API design

Use the existing `Sendable` capability structs holding both a generated handle
and their `PDFViewer`. Accessors are `history(for:)`, `outline(for:)`, and
`attachments(for:)`, accepting `DocumentInfo` and throwing `PDFError`.
Reads remain asynchronous across the kernel thread, matching existing Swift
capabilities even though the underlying Rust capability reads synchronously.

History exposes undo/redo, availability and labels, clear, settings/update/reset,
and events for completion, refusal, and settings changes. Its `undo` and `redo`
are synchronous Rust submissions: returning from Swift means accepted/submitted,
not confirmed. Completion is the existing Rust event when work was submitted. An empty-history
no-op can return successfully without an event, so controls clear their
submission indicator when the call returns. Do not invent a promise
or claim the mirror has changed when the wrapper returns. Keep the existing
`ViewerConfig.history: Bool`; change limits through the capability after open.

Outline exposes every public controller operation: load/refresh/status,
items/children/rows/current/selection, incomplete state, expand/collapse/toggle,
reveal/activate, current view target, editing permission, and create/update/move/
delete. Use typed object-number refs, typed position and target unions, and
typed inputs/patches. An omitted patch member preserves it; an explicit clear
removes it. Reuse existing link destination/action types where their semantics
match; preserve action data that a writable target cannot represent.

Attachment exposes list/get/selection/pending/status, load/refresh, permission
checks, batch add/remove/download/reveal, and events. Keep document files and page
attachment annotations distinct in a typed home enum. Keep stable IDs opaque;
do not identify files by display name. Bytes cross UniFFI as owned buffers and
become `Data`; do not make base64 JSON part of the supported Swift API.

New event enums preserve confirmed origin and resync information. Each stream
subscription owns its Rust unsubscribe and finishes on document close/viewer
destroy, including subscribing after closure. Session-only changes drive UI
through a dedicated revision subscription described below; do not synthesize
fact events to refresh a control.

## SwiftUI controls

Ship reusable `HistoryControls`, `OutlinePanel`, and `AttachmentPanel` in
`EmbedPDFUI`, each backed by a `@MainActor @Observable` model. Native buttons,
lists, menus, sheets and file pickers are sufficient; no shared chrome renderer
or full native viewer redesign is required.

| Control | Required behavior |
| --- | --- |
| HistoryControls | Undo/redo buttons with current labels, permission/availability state, submission state, and reported undo failure |
| OutlinePanel | Hierarchy, expansion, selection and navigation; create/edit/rename/move/delete where allowed; loading/empty/error/incomplete states |
| AttachmentPanel | List both file homes; import document files, remove, select, reveal page files, export/download through a user-selected destination; loading/empty/error states |

File I/O belongs to the UI adapter or host app. API calls never present a picker
or open a downloaded attachment automatically. Support VoiceOver labels,
Dynamic Type, keyboard activation and macOS menus. Use capability predicates for
availability; the engine remains authoritative if permissions change during a
request. Pass attachment bytes to a file exporter only after an explicit user
action; hold security-scoped URLs only while copying their contents.

Each new model owns a cancellable `task()` called from the view's `.task`.
Add `PDFViewer.observeStateRevisions() -> AsyncStream<UInt64>`: a fresh,
independently cancellable subscription per caller, finished on destroy. Retain
the existing single-consumer `stateRevisions` property for compatibility. Models
subscribe before their initial reads, coalesce revision notifications, and read
current snapshots after changes. Revisions invalidate state; they are not an
operation log. Concurrent reads must never overwrite a newer snapshot. A model
must stop its subscriptions when its view disappears or changes document.

## Boundaries and constraints

- Swift tools version 6.2, iOS 17, macOS 14; generated bindings keep Swift 5 language mode and the current UniFFI pin `=0.32.0`.
- All kernel access uses `KernelBound`/kernel jobs. Every exported promise operation forwards `CancelToken`; Swift uses `bridgedCancellable`.
- Swift adds platform adaptation and presentation only. Mirrors, undo policy, permissions, attachment naming, outline tree rules, and confirmed event origin stay in Rust.
- Preserve existing config defaults, typed `PDFError` codes, source compatibility, and generated-file exclusions. Do not hand-edit generated bindings or pin binaries that do not contain the new API.
- The initial planning commit changes documentation only. Implementation changes no npm package unless a separately justified boundary requires it; any directly affected npm packages then need separate changeset files.

## Completion criteria

Every Rust plugin has an explicit coverage disposition and every public
capability member is mapped to Swift or a reviewed exclusion with a reason.
All three new capabilities work with a real document, together and independently
of optional siblings. Existing gaps have permission and lifecycle coverage.
The three controls work together without competing for one event iterator.
Save/reopen preserves edited outlines and attachments. Both source-generated
and packaged engine builds expose the same API.

No claim of full engine API parity, a full editing UI for all existing plugins,
printing, or standalone wrappers for pure helper libraries is included. Existing
JSON APIs remain compatible; replacing all of them with typed alternatives is
a separate expansion after the member audit identifies concrete workflows.

Implementation sequence and exact verification gates:
[`2026-11-06-swift-api-plugin-parity.md`](../plans/2026-11-06-swift-api-plugin-parity.md).
