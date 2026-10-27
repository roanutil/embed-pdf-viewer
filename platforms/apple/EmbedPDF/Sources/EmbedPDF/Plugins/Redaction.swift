public import EmbedPDFBindings
import Foundation

/// Default fill and overlay text styling for newly authored redaction marks.
public typealias RedactionConfig = EmbedPDFBindings.RedactionConfig
/// The color, font and size of text painted over an applied redaction.
public typealias RedactionOverlayText = EmbedPDFBindings.RedactionOverlayText
/// A pending redaction projected from a live redact annotation.
public typealias RedactionMark = EmbedPDFBindings.RedactionMark
/// Whether a pending redaction covers an area or text quadrilaterals.
public typealias RedactionMarkKind = EmbedPDFBindings.RedactionMarkKind
/// An estimate of other annotations that applying marks would remove.
public typealias RedactionCollateral = EmbedPDFBindings.RedactionCollateral
/// Changes to a pending mark's overlay label and repetition setting.
public typealias RedactionLabelPatch = EmbedPDFBindings.RedactionLabelPatch
/// Removed, skipped and failed pending marks from an unmark operation.
public typealias RedactionUnmarkResult = EmbedPDFBindings.RedactionUnmarkResult
/// A pending mark skipped during a redaction operation.
public typealias RedactionSkipped = EmbedPDFBindings.RedactionSkipped
/// A mark reference and the error from a failed removal.
public typealias RedactionUnmarkFailure = EmbedPDFBindings.RedactionUnmarkFailure
/// The per-page outcomes of permanently applying redaction marks.
public typealias RedactionApplyResult = EmbedPDFBindings.RedactionApplyResult
/// The scope of marks submitted for redaction application.
public typealias RedactionApplyScope = EmbedPDFBindings.RedactionApplyScope
/// One page's application status and resulting redaction information.
public typealias RedactionPageResult = EmbedPDFBindings.RedactionPageResult
/// Whether redaction application succeeded, was skipped or failed for a page.
public typealias RedactionPageStatus = EmbedPDFBindings.RedactionPageStatus
/// A pending-mark change or a confirmed redaction application.
public typealias RedactionEvent = EmbedPDFBindings.RedactionEvent

/// A document's redactions: mark regions, review them, apply them. Get one
/// with `PDFViewer.redaction(for:)`; the viewer needs `ViewerConfig(
/// interaction: true, annotation: "", redaction: RedactionConfig())`. It keeps the viewer alive. After its document
/// closes, every call throws `instance-closed`.
public struct RedactionCapability: Sendable {
  let handle: RedactionHandle
  let viewer: PDFViewer

  /// Lists pending redaction marks on `page`, or across the document when `page` is `nil`.
  public func pending(on page: PageRef? = nil) async throws(PDFError) -> [RedactionMark] {
    try await bridged { try await handle.pending(page: page) }
  }

  /// Returns the pending mark represented by `ref`, or `nil` if it is not a pending redaction.
  public func pendingItem(_ ref: AnnotationRef) async throws(PDFError) -> RedactionMark? {
    try await bridged { try await handle.pendingItem(annotationRef: ref) }
  }

  /// Counts pending marks on `page`, or across the document when `page` is `nil`.
  public func pendingCount(on page: PageRef? = nil) async throws(PDFError) -> UInt32 {
    try await bridged { try await handle.pendingCount(page: page) }
  }

  /// What applying `refs` (every pending mark when `nil`) would also remove.
  public func estimateCollateral(_ refs: [AnnotationRef]? = nil) async throws(PDFError) -> RedactionCollateral {
    try await bridged { try await handle.estimateCollateral(refs: refs) }
  }

  /// Creates pending redaction marks from the current text selection, one per page, then clears the selection.
  ///
  /// Requires the selection plugin. Returns an empty array when there is no selected text.
  public func markSelection() async throws(PDFError) -> [RedactionMark] {
    try await bridgedCancellable { token in try await handle.markSelection(cancel: token) }
  }

  /// Creates a pending area mark with page-space `bounds`; content remains until the mark is applied.
  public func markArea(_ bounds: Rect, on page: PageRef) async throws(PDFError) -> RedactionMark {
    try await bridgedCancellable { token in try await handle.markArea(page: page, bounds: bounds, cancel: token) }
  }

  /// Creates a pending area mark covering the page; content remains until application.
  public func markPage(_ page: PageRef) async throws(PDFError) -> RedactionMark {
    try await bridgedCancellable { token in try await handle.markPage(page: page, cancel: token) }
  }

  /// Finds text matching `query` and creates pending marks on the specified pages, or all pages when `nil`.
  public func markMatches(_ query: SearchQuery, on pages: [PageRef]? = nil) async throws(PDFError) -> [RedactionMark] {
    try await bridgedCancellable { token in try await handle.markMatches(query: query, pages: pages, cancel: token) }
  }

  /// Removes pending mark annotations and reports removed, skipped and failed references.
  public func unmark(_ refs: [AnnotationRef]) async throws(PDFError) -> RedactionUnmarkResult {
    try await bridgedCancellable { token in try await handle.unmark(refs: refs, cancel: token) }
  }

  /// Removes all pending redaction marks without applying them and reports individual outcomes.
  public func clearPending() async throws(PDFError) -> RedactionUnmarkResult {
    try await bridgedCancellable { token in try await handle.clearPending(cancel: token) }
  }

  /// Changes a pending mark's overlay label and repetition settings and returns the updated mark.
  @discardableResult
  public func updateLabel(_ ref: AnnotationRef, _ patch: RedactionLabelPatch) async throws(PDFError)
    -> RedactionMark
  {
    try await bridgedCancellable { token in
      try await handle.updateLabel(annotationRef: ref, patch: patch, cancel: token)
    }
  }

  /// Apply these pending marks. Irreversible.
  public func apply(_ refs: [AnnotationRef]) async throws(PDFError) -> RedactionApplyResult {
    try await bridgedCancellable { token in try await handle.apply(refs: refs, cancel: token) }
  }

  /// Apply every mark on these pages. Irreversible.
  public func apply(pages: [PageRef]) async throws(PDFError) -> RedactionApplyResult {
    try await bridgedCancellable { token in try await handle.applyPages(pages: pages, cancel: token) }
  }

  /// Apply every redaction in the document. Irreversible.
  public func applyAll() async throws(PDFError) -> RedactionApplyResult {
    try await bridgedCancellable { token in try await handle.applyAll(cancel: token) }
  }

  /// Returns whether the annotation authoring capability currently allows creating redaction marks.
  public func canMark() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canMark() }
  }

  /// Returns whether the engine supports redaction and the required redaction, page and annotation grants are present.
  public func canApply() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canApply() }
  }

  /// Returns whether a redaction application is currently in progress.
  public func isApplying() async throws(PDFError) -> Bool {
    try await bridged { try await handle.isApplying() }
  }

  /// Returns the most recent application result, or `nil` before an application has completed.
  public func lastResult() async throws(PDFError) -> RedactionApplyResult? {
    try await bridged { try await handle.lastResult() }
  }

  /// Applies and pending-mark changes. Finishes when the document closes
  /// (at once if it already has) or the viewer is destroyed.
  public var events: AsyncStream<RedactionEvent> {
    viewer.registry.stream { handle.subscribe(listener: $0) }
  }
}

extension EventRelay: RedactionEventListener where Event == RedactionEvent {
  func onEvent(event: RedactionEvent) {
    continuation.yield(event)
  }

  func onEnd() {
    continuation.finish()
  }
}

extension PDFViewer {
  /// Returns the redaction capability bound to an open document.
  ///
  /// The viewer must have the `redaction` plugin enabled. The capability retains the viewer
  /// and becomes invalid when the document closes.
  ///
  /// - Throws: `PDFError` if the plugin or document scope is unavailable.
  public func redaction(for document: DocumentInfo) async throws(PDFError) -> RedactionCapability {
    let handle = try await bridged { try await host.redaction(documentId: document.id) }
    return RedactionCapability(handle: handle, viewer: self)
  }
}
