public import EmbedPDFBindings

public typealias OutlineItemRef = EmbedPDFBindings.OutlineItemRef
public typealias OutlineItem = EmbedPDFBindings.OutlineItem
public typealias OutlineRow = EmbedPDFBindings.OutlineRow
public typealias OutlineSnapshot = EmbedPDFBindings.OutlineSnapshot
public typealias OutlinePosition = EmbedPDFBindings.OutlinePosition
public typealias OutlineItemInput = EmbedPDFBindings.OutlineItemInput
public typealias OutlineItemPatch = EmbedPDFBindings.OutlineItemPatch
public typealias OutlinePatchKey = EmbedPDFBindings.OutlinePatchKey
public typealias OutlineCreateOptions = EmbedPDFBindings.OutlineCreateOptions
public typealias OutlineEvent = EmbedPDFBindings.OutlineEvent

/// Outline reads, navigation and undoable edits for one open document.
public struct OutlineCapability: Sendable {
  let handle: OutlineHandle
  let viewer: PDFViewer

  public func list() async throws(PDFError) -> [OutlineItem] {
    try await bridged { try await handle.list() }
  }

  public func item(_ ref: OutlineItemRef) async throws(PDFError) -> OutlineItem? {
    try await bridged { try await handle.item(item: ref) }
  }

  public func children(of parent: OutlineItemRef? = nil) async throws(PDFError) -> [OutlineItem] {
    try await bridged { try await handle.children(parent: parent) }
  }

  public func rows() async throws(PDFError) -> [OutlineRow] {
    try await bridged { try await handle.rows() }
  }

  public func currentItem() async throws(PDFError) -> OutlineItemRef? {
    try await bridged { try await handle.currentItem() }
  }

  public func selectedItems() async throws(PDFError) -> [OutlineItemRef] {
    try await bridged { try await handle.selectedItems() }
  }

  public func select(_ refs: [OutlineItemRef]) async throws(PDFError) {
    try await bridged { try await handle.select(items: refs) }
  }

  public func status() async throws(PDFError) -> ResourceStatus {
    try await bridged { try await handle.status() }
  }

  public func isIncomplete() async throws(PDFError) -> Bool {
    try await bridged { try await handle.isIncomplete() }
  }

  public func isExpanded(_ ref: OutlineItemRef) async throws(PDFError) -> Bool {
    try await bridged { try await handle.isExpanded(item: ref) }
  }

  public func canEdit() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canEdit() }
  }

  public func currentViewTarget() async throws(PDFError) -> PageLinkTarget? {
    try await bridged { try await handle.currentViewTarget() }
  }

  public func activate(_ ref: OutlineItemRef) async throws(PDFError) -> LinkActivation {
    try await bridged { try await handle.activate(item: ref) }
  }

  public func ensureLoaded() async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.ensureLoaded(cancel: token) }
  }

  public func refresh() async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.refresh(cancel: token) }
  }

  public func create(_ input: OutlineItemInput, options: OutlineCreateOptions = OutlineCreateOptions()) async throws(PDFError) -> OutlineItem {
    try await bridgedCancellable { token in try await handle.create(input: input, options: options, cancel: token) }
  }

  public func update(_ ref: OutlineItemRef, _ patch: OutlineItemPatch) async throws(PDFError) -> OutlineItem {
    try await bridgedCancellable { token in try await handle.update(item: ref, patch: patch, cancel: token) }
  }

  public func move(_ refs: [OutlineItemRef], to parent: OutlineItemRef?, position: OutlinePosition = .end) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.moveItems(items: refs, parent: parent, position: position, cancel: token) }
  }

  public func delete(_ refs: [OutlineItemRef]) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.deleteItems(items: refs, cancel: token) }
  }

  public func expand(_ ref: OutlineItemRef) async throws(PDFError) {
    try await bridged { try await handle.expand(item: ref) }
  }

  public func collapse(_ ref: OutlineItemRef) async throws(PDFError) {
    try await bridged { try await handle.collapse(item: ref) }
  }

  public func toggle(_ ref: OutlineItemRef) async throws(PDFError) {
    try await bridged { try await handle.toggle(item: ref) }
  }

  public func reveal(_ ref: OutlineItemRef) async throws(PDFError) {
    try await bridged { try await handle.reveal(item: ref) }
  }

  public func expandAll() async throws(PDFError) {
    try await bridged { try await handle.expandAll() }
  }

  public func collapseAll() async throws(PDFError) {
    try await bridged { try await handle.collapseAll() }
  }

  public var events: AsyncStream<OutlineEvent> { viewer.registry.stream { handle.subscribe(listener: $0) } }
}

extension EventRelay: OutlineEventListener where Event == OutlineEvent {
  func onEvent(event: OutlineEvent) { continuation.yield(event) }
  func onEnd() { continuation.finish() }
}

extension PDFViewer {
  public func outline(for document: DocumentInfo) async throws(PDFError) -> OutlineCapability {
    let handle = try await bridged { try await host.outline(documentId: document.id) }
    return OutlineCapability(handle: handle, viewer: self)
  }
}
