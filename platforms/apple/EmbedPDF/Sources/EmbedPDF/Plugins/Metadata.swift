public import EmbedPDFBindings

/// The document's PDF Info dictionary values; date strings use ISO 8601.
public typealias DocumentMetadata = EmbedPDFBindings.DocumentMetadata
/// The PDF Info dictionary's trapping status.
public typealias DocumentMetadataTrapped = EmbedPDFBindings.DocumentMetadataTrapped
/// Metadata fields to set, clear, or leave unchanged.
///
/// Nil text fields are unchanged; fields named in clear are removed. A field cannot be both set
/// and cleared. Within custom, a nil value removes that custom key.
public typealias MetadataPatch = EmbedPDFBindings.MetadataPatch
/// A document metadata field that can be identified in updates.
public typealias MetadataField = EmbedPDFBindings.MetadataField
/// An event describing a confirmed metadata update.
public typealias MetadataUpdatedEvent = EmbedPDFBindings.MetadataUpdatedEvent
/// The loading, availability, or failure state of a plugin resource.
public typealias ResourceStatus = EmbedPDFBindings.ResourceStatus
/// An event describing confirmed metadata changes.
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

  /// Returns the metadata resource's current loading, availability, or failure state.
  public func status() async throws(PDFError) -> ResourceStatus {
    try await bridged { try await handle.status() }
  }

  /// Checks whether the session currently permits changing document metadata (`doc.metadata.modify`).
  public func canUpdate() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canUpdate() }
  }

  /// Starts reading the metadata unless it is already loaded or loading.
  ///
  /// Nothing reads the Info dictionary until this or ``refresh()`` asks.
  /// Cancelling throws here; the shared read still lands.
  public func ensureLoaded() async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.ensureLoaded(cancel: token) }
  }

  /// Updates the document metadata and returns once the confirmed change reaches the snapshot and events.
  ///
  /// Cancelling stops waiting for confirmation; an engine write already sent can still take effect.
  /// - Throws: `invalid-input` when a patch both sets and clears a field, or the engine or
  ///   permission error reported by the update.
  public func update(_ patch: MetadataPatch) async throws(PDFError) -> DocumentMetadata {
    try await bridgedCancellable { token in try await handle.update(patch: patch, cancel: token) }
  }

  /// Reloads metadata from the engine, sharing a read already in flight.
  ///
  /// Task cancellation only skips a read that has not started.
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
  /// Returns this document's metadata capability; enable metadata in the viewer configuration
  /// before requesting it.
  public func metadata(for document: DocumentInfo) async throws(PDFError) -> MetadataCapability {
    let handle = try await bridged { try await host.metadata(documentId: document.id) }
    return MetadataCapability(handle: handle, viewer: self)
  }
}
