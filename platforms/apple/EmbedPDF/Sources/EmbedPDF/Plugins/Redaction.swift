public import EmbedPDFBindings
import Foundation

public typealias RedactionConfig = EmbedPDFBindings.RedactionConfig
public typealias RedactionOverlayText = EmbedPDFBindings.RedactionOverlayText
public typealias RedactionMark = EmbedPDFBindings.RedactionMark
public typealias RedactionMarkKind = EmbedPDFBindings.RedactionMarkKind
public typealias RedactionCollateral = EmbedPDFBindings.RedactionCollateral
public typealias RedactionLabelPatch = EmbedPDFBindings.RedactionLabelPatch
public typealias RedactionUnmarkResult = EmbedPDFBindings.RedactionUnmarkResult
public typealias RedactionSkipped = EmbedPDFBindings.RedactionSkipped
public typealias RedactionUnmarkFailure = EmbedPDFBindings.RedactionUnmarkFailure
public typealias RedactionApplyResult = EmbedPDFBindings.RedactionApplyResult
public typealias RedactionApplyScope = EmbedPDFBindings.RedactionApplyScope
public typealias RedactionPageResult = EmbedPDFBindings.RedactionPageResult
public typealias RedactionPageStatus = EmbedPDFBindings.RedactionPageStatus
public typealias RedactionEvent = EmbedPDFBindings.RedactionEvent

/// A document's redactions: mark regions, review them, apply them. Get one
/// with `PDFViewer.redaction(for:)`; the viewer needs `ViewerConfig(
/// interaction: true, annotation: "", redaction: RedactionConfig())`. It keeps the viewer alive. After its document
/// closes, every call throws `instance-closed`.
public struct RedactionCapability: Sendable {
  let handle: RedactionHandle
  let viewer: PDFViewer

  public func pending(on page: PageRef? = nil) async throws(PDFError) -> [RedactionMark] {
    try await bridged { try await handle.pending(page: page) }
  }

  public func pendingItem(_ ref: AnnotationRef) async throws(PDFError) -> RedactionMark? {
    try await bridged { try await handle.pendingItem(annotationRef: ref) }
  }

  public func pendingCount(on page: PageRef? = nil) async throws(PDFError) -> UInt32 {
    try await bridged { try await handle.pendingCount(page: page) }
  }

  /// What applying `refs` (every pending mark when `nil`) would also remove.
  public func estimateCollateral(_ refs: [AnnotationRef]? = nil) async throws(PDFError) -> RedactionCollateral {
    try await bridged { try await handle.estimateCollateral(refs: refs) }
  }

  public func markSelection() async throws(PDFError) -> [RedactionMark] {
    try await bridgedCancellable { token in try await handle.markSelection(cancel: token) }
  }

  public func markArea(_ bounds: Rect, on page: PageRef) async throws(PDFError) -> RedactionMark {
    try await bridgedCancellable { token in try await handle.markArea(page: page, bounds: bounds, cancel: token) }
  }

  public func markPage(_ page: PageRef) async throws(PDFError) -> RedactionMark {
    try await bridgedCancellable { token in try await handle.markPage(page: page, cancel: token) }
  }

  public func markMatches(_ query: SearchQuery, on pages: [PageRef]? = nil) async throws(PDFError) -> [RedactionMark] {
    try await bridgedCancellable { token in try await handle.markMatches(query: query, pages: pages, cancel: token) }
  }

  public func unmark(_ refs: [AnnotationRef]) async throws(PDFError) -> RedactionUnmarkResult {
    try await bridgedCancellable { token in try await handle.unmark(refs: refs, cancel: token) }
  }

  public func clearPending() async throws(PDFError) -> RedactionUnmarkResult {
    try await bridgedCancellable { token in try await handle.clearPending(cancel: token) }
  }

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

  public func canMark() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canMark() }
  }

  public func canApply() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canApply() }
  }

  public func canUnmark(_ ref: AnnotationRef) async throws(PDFError) -> Bool {
    try await bridged { try await handle.canUnmark(annotationRef: ref) }
  }

  public func canUpdateLabel(_ ref: AnnotationRef) async throws(PDFError) -> Bool {
    try await bridged { try await handle.canUpdateLabel(annotationRef: ref) }
  }

  public func isApplying() async throws(PDFError) -> Bool {
    try await bridged { try await handle.isApplying() }
  }

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
  public func redaction(for document: DocumentInfo) async throws(PDFError) -> RedactionCapability {
    let handle = try await bridged { try await host.redaction(documentId: document.id) }
    return RedactionCapability(handle: handle, viewer: self)
  }
}
