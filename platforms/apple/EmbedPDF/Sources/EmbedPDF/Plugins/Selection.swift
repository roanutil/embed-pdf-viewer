public import EmbedPDFBindings

public typealias SelectionTextPosition = EmbedPDFBindings.SelectionTextPosition
public typealias SelectionTextRange = EmbedPDFBindings.SelectionTextRange
public typealias SelectionInput = EmbedPDFBindings.SelectionInput
public typealias SelectionSegment = EmbedPDFBindings.SelectionSegment
public typealias SelectionEndpoint = EmbedPDFBindings.SelectionEndpoint
public typealias SelectionAnchor = EmbedPDFBindings.SelectionAnchor
public typealias SelectionDirection = EmbedPDFBindings.SelectionDirection
public typealias SelectionSnapshotPage = EmbedPDFBindings.SelectionSnapshotPage
public typealias SelectionSnapshot = EmbedPDFBindings.SelectionSnapshot
public typealias SelectionEvent = EmbedPDFBindings.SelectionEvent
public typealias SelectionChangedEvent = EmbedPDFBindings.SelectionChangedEvent
public typealias SelectionCommittedEvent = EmbedPDFBindings.SelectionCommittedEvent

/// A document's text selection: select by range, span, word, line or
/// page; read its geometry and its text. Get one with
/// `PDFViewer.selection(for:)`; the viewer needs
/// `ViewerConfig(interaction: true, selection: true)`. It keeps the viewer
/// alive. After its document closes, every call throws `instance-closed`.
///
/// Indices are PDFium character indices, the space search hits use, not
/// offsets into a Swift `String`.
public struct SelectionCapability: Sendable {
  let handle: SelectionHandle
  let viewer: PDFViewer

  /// Whether selection geometry is served (`doc.text.select`).
  public func canSelect() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canSelect() }
  }

  /// Whether text extraction is served (`doc.text.copy`).
  public func canCopy() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canCopy() }
  }

  public func select(_ input: SelectionInput) async throws(PDFError) {
    try await bridged { try await handle.select(input: input) }
  }

  /// Select a search hit's characters.
  public func select(_ hit: SearchHit) async throws(PDFError) {
    try await select(.span(page: hit.page, start: Int64(hit.start), count: Int64(hit.count)))
  }

  public func selectAll() async throws(PDFError) {
    try await bridged { try await handle.selectAll() }
  }

  public func selectPage(_ page: PageRef) async throws(PDFError) {
    try await bridged { try await handle.selectPage(page: page) }
  }

  /// False when the point has no selectable text.
  public func selectWord(at point: Point, on page: PageRef) async throws(PDFError) -> Bool {
    try await bridged { try await handle.selectWordAt(page: page, point: point) }
  }

  public func selectLine(at point: Point, on page: PageRef) async throws(PDFError) -> Bool {
    try await bridged { try await handle.selectLineAt(page: page, point: point) }
  }

  public func extend(to point: Point, on page: PageRef) async throws(PDFError) {
    try await bridged { try await handle.extendTo(page: page, point: point) }
  }

  public func clear() async throws(PDFError) {
    try await bridged { try await handle.clear() }
  }

  public func hasSelection() async throws(PDFError) -> Bool {
    try await bridged { try await handle.hasSelection() }
  }

  public func snapshot() async throws(PDFError) -> SelectionSnapshot {
    try await bridged { try await handle.snapshot() }
  }

  public func range() async throws(PDFError) -> SelectionTextRange? {
    try await bridged { try await handle.range() }
  }

  public func selectedPages() async throws(PDFError) -> [PageRef] {
    try await bridged { try await handle.selectedPages() }
  }

  public func segments(on page: PageRef) async throws(PDFError) -> [SelectionSegment] {
    try await bridged { try await handle.segments(page: page) }
  }

  public func rects(on page: PageRef) async throws(PDFError) -> [Rect] {
    try await bridged { try await handle.rects(page: page) }
  }

  public func anchor() async throws(PDFError) -> SelectionAnchor? {
    try await bridged { try await handle.anchor() }
  }

  /// The selected text, pages joined with a newline. Throws
  /// `permission-denied` without `doc.text.copy`, and
  /// `operation-cancelled` when the task is cancelled.
  public func readText() async throws(PDFError) -> String {
    try await bridgedCancellable { token in try await handle.readText(cancel: token) }
  }

  /// Text for any range, leaving the selection alone.
  public func readText(in range: SelectionTextRange) async throws(PDFError) -> String {
    try await bridgedCancellable { token in try await handle.readTextInRange(range: range, cancel: token) }
  }

  public func ensureLoaded(_ page: PageRef) async throws(PDFError) {
    try await bridged { try await handle.ensureLoaded(page: page) }
  }

  public func isLoaded(_ page: PageRef) async throws(PDFError) -> Bool {
    try await bridged { try await handle.isLoaded(page: page) }
  }

  public func isOverText(_ point: Point, on page: PageRef) async throws(PDFError) -> Bool {
    try await bridged { try await handle.isOverText(page: page, point: point) }
  }

  public func beginGesture() async throws(PDFError) {
    try await bridged { try await handle.beginGesture() }
  }

  /// False off text.
  public func beginGesture(at point: Point, on page: PageRef) async throws(PDFError) -> Bool {
    try await bridged { try await handle.beginGestureAt(page: page, point: point) }
  }

  public func endGesture() async throws(PDFError) {
    try await bridged { try await handle.endGesture() }
  }

  /// Whether a selecting gesture is in flight.
  public func isSelecting() async throws(PDFError) -> Bool {
    try await bridged { try await handle.isSelecting() }
  }

  public func setHighlightVisible(_ visible: Bool) async throws(PDFError) {
    try await bridged { try await handle.setHighlightVisible(visible: visible) }
  }

  public func isHighlightVisible() async throws(PDFError) -> Bool {
    try await bridged { try await handle.isHighlightVisible() }
  }

  /// Finishes when the document closes (at once if it already has) or the
  /// viewer is destroyed.
  public var events: AsyncStream<SelectionEvent> {
    viewer.registry.stream { handle.subscribe(listener: $0) }
  }
}

extension EventRelay: SelectionEventListener where Event == SelectionEvent {
  func onEvent(event: SelectionEvent) {
    continuation.yield(event)
  }

  func onEnd() {
    continuation.finish()
  }
}

extension PDFViewer {
  public func selection(for document: DocumentInfo) async throws(PDFError) -> SelectionCapability {
    let handle = try await bridged { try await host.selection(documentId: document.id) }
    return SelectionCapability(handle: handle, viewer: self)
  }
}
