# Swift app-facing API coverage

## Intent and scope

The user authorized merging `research/swift-api-plugin-parity-plan` into
`research/multi-platform-rust-core-impl-plugins`, pushing to `fork`, transferring
any independent non-plugin fixes to `research/multi-platform-rust-core-impl`,
and closing remaining Swift binding gaps. They selected app-facing APIs over
exporting every public Rust implementation method.

The merge is a separate first deliverable. Its compatibility fixes preserve
current Rust semantics: attachment create/delete, sizeBytes/md5 metadata, ISO
event origins, transaction/serverSequence, and settings changedKeys. The old
coverage report remains historical; the inventory must follow the current tree.

There is no independent core-only change in the incoming branch. Its shared
FFI and revision-stream changes depend on the plugin viewer layer, which the
core-only branch does not contain. Do not merge that layer into the core branch.

## Approaches

1. **Typed app-facing capabilities (selected).** Extend the existing UniFFI
   handles and Swift capability structs. Keep operations, permissions,
   cancellation, events, and lifetime in Rust. Swift owns native presentation
   and converts owned pixel buffers into CGImage.
2. **Export all public Rust methods.** Rejected by the user: exposes caches,
   kernel resources, host painter values, and lifecycle internals without an
   app workflow.
3. **Generic string dispatch or Swift implementations of plugin policy.**
   Rejected by the existing architecture. Domain JSON remains appropriate only
   where the existing native API already represents an open engine payload
   (annotation bundles, rich text, form patches); it is not a dispatch protocol.

## App-facing additions

### Documents and viewer settings

Extend ViewerHost and PDFViewer with document retry and rename, close-all,
document lookup and active-document access/selection, document order changes,
page lookup/index/revision, permission and download checks, and layer saving.
Reuse DocumentInfo, PageInfo, PageRef, SaveMode, and existing error/cancellation
bridges. Avoid Swift-side permission guesses or independently maintained lists.

Expose viewer settings through the existing Rust settings boundary. Opening
URLs must use KernelOptions.fetch through a foreign HostFetch provider backed
by Foundation URLSession. Preserve the kernel's loading/failed tab, retry and
refetch, HTTP error mapping, and close-driven cancellation. Downloading in Swift
before calling the existing byte-open method is not equivalent.

Printing requires a real host printer and its prepared-print lifetime contract.
Do not report a working print API by exporting methods that always refuse for
the native host. Inventory this as a host integration requirement if the native
engine cannot provide the required prepared representations.

### Plugin settings and command controls

Expose readable settings, update/reset and change observation for plugins that
already support these operations in Rust. Reuse existing typed config records
where their semantics include the complete mutable settings; otherwise add
resolved records and patches for the mutable scalar subset. Reset restores
registration values, not factory defaults. Registration-only
ports (signers, trust providers, script factories, locale loaders, asset engines)
stay at registration boundaries; they must not appear in serializable settings.
Use the viewer state observation stream when a settings event does not have a
dedicated native event yet; it must still be possible to read current settings.

Add per-command disable/enable/read/set operations and structured permanent
refusal details. Expose named-action permission probing. Preserve category
controls and Rust refusal behavior.

### Annotation and form editing

Add annotation bundle export/import, preserving bytes and typed identities with
existing Rust bundle conversion. Add the selected-link update workflow when it
does more than the existing per-annotation thread operation.

Add form calculation-order editing and independent widget update/delete, plus
public field/widget/selection reads needed by these operations. Reuse the
existing form patch and interchange conventions; do not reproduce form policy,
editor layout, scripting sinks, or widget painting in Swift.

### Snapshot and native viewport workflows

Expose snapshot availability, held snapshot (page and box immediately), clear,
box lookup, and changes. Resolve the image separately through a cancellable
snapshot handle because the Rust image is a Promise. Cancelling one consumer
ends its wait; clearing or closing cancels the shared render. The interaction
plugin remains the owner of snapshot gestures. A resolved snapshot image must
survive clearing and document close because Swift owns its returned pixels.
Add native zoom-box operations
only where they are distinct from already available camera/zoom gestures.

The render picture-state/painter handover protocol remains private: CGImage,
RenderViewDemand and tile plans are the native painting contract. Add any missing
current-page read that has app-visible semantics beyond currentItemPages and
visiblePages; do not add duplicate aliases solely to improve a member count.

## Coverage and acceptance

The inventory must distinguish mapped members from internal resources and host
integration requirements. Remove blanket exclusions that hide implemented
workflows (the old i18n classification is one example). Include public kernel
document operations in the audit so plugin accounting cannot imply core parity.
List remaining deliberate omissions and reasons in the report; no claim that
accounting every member means implementing every operation.

Every added boundary workflow is exercised through a real kernel/host or the
existing conformance test kernel. Tests cover cancellation, closed documents,
permission refusals, empty values, saved/reopened data where applicable, and
independent documents. Do not write source-string tests for trivial forwarding.

Rebuild generated bindings; run Rust tests/clippy for affected crates, inventory
checker tests, and the complete native Swift package suite on macOS. Run the
iOS simulator package suite when the available runtime permits it. Report exact
validation limits. Generated sources and binary artifacts remain ignored.

## Distribution

Commit and push verified source changes to the plugins branch on `fork`.
Publishing a replacement engine binary is a separate release operation; do not
claim the default downloaded engine includes new exports until both package
pins refer to a matching published artifact. Local source-engine tests establish
the source API; document the binary requirement in the Apple README.

## Implementation choices delegated to the builder

Keep the current checkout for the named-branch integration, since the user
explicitly requested changes to that branch and the working tree was clean.
Split code at existing host/plugin/Swift file boundaries. Settle exact Swift
labels from adjacent APIs and prefer existing typed records. If a proposed
workflow requires missing engine or host behavior, document that concrete
dependency instead of implementing a misleading no-op wrapper.
