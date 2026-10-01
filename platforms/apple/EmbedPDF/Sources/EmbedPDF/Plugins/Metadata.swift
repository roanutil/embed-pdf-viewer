public import EmbedPDFBindings

public typealias DocumentMetadata = EmbedPDFBindings.DocumentMetadata
public typealias DocumentMetadataTrapped = EmbedPDFBindings.DocumentMetadataTrapped
public typealias MetadataPatch = EmbedPDFBindings.MetadataPatch
public typealias MetadataField = EmbedPDFBindings.MetadataField
public typealias MetadataUpdatedEvent = EmbedPDFBindings.MetadataUpdatedEvent
public typealias ResourceStatus = EmbedPDFBindings.ResourceStatus
public typealias MetadataEvent = EmbedPDFBindings.MetadataEvent

/// A document's Info dictionary, read and edited. Get one with
/// `PDFViewer.metadata(for:)`; the viewer needs `ViewerConfig(metadata: true)`.
/// It keeps the viewer alive. After its document closes, every call throws
/// `instance-closed`.
public struct MetadataCapability: Sendable {
  let handle: MetadataHandle
  let viewer: PDFViewer

  /// The current metadata: `nil` until the first read lands, or while reading
  /// is forbidden (see `status()`).
  public func snapshot() async throws(PDFError) -> DocumentMetadata? {
    try await bridged { try await handle.snapshot() }
  }

  public func status() async throws(PDFError) -> ResourceStatus {
    try await bridged { try await handle.status() }
  }

  public func canEdit() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canEdit() }
  }

  /// Returns once the confirmed change has reached the snapshot and `events`.
  public func update(_ patch: MetadataPatch) async throws(PDFError) -> DocumentMetadata {
    try await bridgedCancellable { token in try await handle.update(patch: patch, cancel: token) }
  }

  public func refresh() async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.refresh(cancel: token) }
  }

  /// Confirmed changes, whoever made them. Finishes when the document closes
  /// (at once if it already has) or the viewer is destroyed.
  public var events: AsyncStream<MetadataEvent> {
    viewer.registry.stream { handle.subscribe(listener: $0) }
  }
}

extension EventRelay: MetadataEventListener where Event == MetadataEvent {
  func onEvent(event: MetadataEvent) {
    continuation.yield(event)
  }

  func onEnd() {
    continuation.finish()
  }
}

extension PDFViewer {
  public func metadata(for document: PDFDocument) async throws(PDFError) -> MetadataCapability {
    let handle = try await bridged { try await host.metadata(documentId: document.id) }
    return MetadataCapability(handle: handle, viewer: self)
  }
}
