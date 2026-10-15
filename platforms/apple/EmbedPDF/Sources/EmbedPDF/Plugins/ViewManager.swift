public import EmbedPDFBindings

public typealias PaneInfo = EmbedPDFBindings.PaneInfo
public typealias PaneEvent = EmbedPDFBindings.PaneEvent
public typealias PaneFocusChangedEvent = EmbedPDFBindings.PaneFocusChangedEvent
public typealias DocumentMovedEvent = EmbedPDFBindings.DocumentMovedEvent
public typealias ViewManagerEvent = EmbedPDFBindings.ViewManagerEvent

/// Panes over the open documents: each pane is a tab strip with one active
/// document, and the panes partition the documents. Get it with
/// `PDFViewer.viewManager()`; the viewer needs
/// `ViewerConfig(viewManager: true)`. One capability for the whole viewer.
/// It keeps the viewer alive, and after `destroy` every call throws
/// `instance-closed`. Documents are `PDFDocument`s, as every other
/// capability takes them; panes are the ids `panes()` lists. No member
/// throws for an id it doesn't know, as on the web. An unknown pane is
/// ignored. `setFocusedPane`, `createPane`, `splitPane` and `addDocument`
/// don't check documents or panes against what's open: a closed document
/// lands as a tab (or an unknown pane as the focus) until the open
/// documents next change.
public struct ViewManagerCapability: Sendable {
  let handle: ViewManagerHandle
  let viewer: PDFViewer

  /// In display order.
  public func panes() async throws(PDFError) -> [PaneInfo] {
    try await bridged { try await handle.panes() }
  }

  public func pane(_ id: String) async throws(PDFError) -> PaneInfo? {
    try await bridged { try await handle.pane(id: id) }
  }

  public func paneOrder() async throws(PDFError) -> [String] {
    try await bridged { try await handle.paneOrder() }
  }

  public func focusedPaneId() async throws(PDFError) -> String? {
    try await bridged { try await handle.focusedPaneId() }
  }

  /// The pane that holds the document, if any.
  public func pane(holding document: PDFDocument) async throws(PDFError) -> String? {
    try await bridged { try await handle.paneOfDocument(documentId: document.id) }
  }

  /// A new pane, focused. Each document moves in from wherever it was.
  public func createPane(documents: [PDFDocument] = []) async throws(PDFError) -> String {
    try await bridged { try await handle.createPane(documentIds: documents.map(\.id)) }
  }

  /// Its documents go to `target`, else the focused pane, else the last one.
  public func removePane(_ id: String, movingDocumentsTo target: String? = nil) async throws(PDFError) {
    try await bridged { try await handle.removePane(id: id, moveDocumentsTo: target) }
  }

  /// Clamped into the order.
  public func movePane(_ id: String, to index: UInt32) async throws(PDFError) {
    try await bridged { try await handle.movePane(id: id, toIndex: index) }
  }

  public func setFocusedPane(_ id: String?) async throws(PDFError) {
    try await bridged { try await handle.setFocusedPane(id: id) }
  }

  /// A new pane showing the document.
  public func splitPane(showing document: PDFDocument) async throws(PDFError) -> String {
    try await bridged { try await handle.splitPane(documentId: document.id) }
  }

  public func setActiveDocument(_ document: PDFDocument?, in paneId: String) async throws(PDFError) {
    try await bridged { try await handle.setActiveDocument(paneId: paneId, documentId: document?.id) }
  }

  /// A document another pane holds moves. `nil` appends.
  public func addDocument(_ document: PDFDocument, to paneId: String, at index: UInt32? = nil)
    async throws(PDFError)
  {
    try await bridged { try await handle.addDocument(paneId: paneId, documentId: document.id, index: index) }
  }

  public func removeDocument(_ document: PDFDocument, from paneId: String) async throws(PDFError) {
    try await bridged { try await handle.removeDocument(paneId: paneId, documentId: document.id) }
  }

  /// Reorder a tab inside its pane.
  public func moveDocument(_ document: PDFDocument, within paneId: String, to index: UInt32)
    async throws(PDFError)
  {
    try await bridged {
      try await handle.moveDocumentWithin(paneId: paneId, documentId: document.id, toIndex: index)
    }
  }

  /// Drag a tab into another pane, where it becomes active.
  public func moveDocument(
    _ document: PDFDocument, from source: String, to destination: String, at index: UInt32? = nil
  ) async throws(PDFError) {
    try await bridged {
      try await handle.moveDocumentBetween(
        fromPaneId: source, toPaneId: destination, documentId: document.id, toIndex: index)
    }
  }

  /// Pane events. Finishes when the viewer is destroyed (at once if it
  /// already has been).
  public var events: AsyncStream<ViewManagerEvent> {
    viewer.registry.stream { handle.subscribe(listener: $0) }
  }
}

extension EventRelay: ViewManagerEventListener where Event == ViewManagerEvent {
  func onEvent(event: ViewManagerEvent) {
    continuation.yield(event)
  }

  func onEnd() {
    continuation.finish()
  }
}

extension PDFViewer {
  public func viewManager() async throws(PDFError) -> ViewManagerCapability {
    let handle = try await bridged { try await host.viewManager() }
    return ViewManagerCapability(handle: handle, viewer: self)
  }
}
