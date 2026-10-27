public import EmbedPDFBindings

/// A page and PDFium character index, independent of Swift string indices.
public typealias SelectionTextPosition = EmbedPDFBindings.SelectionTextPosition
/// A half-open range in engine character space, ordered across document pages.
public typealias SelectionTextRange = EmbedPDFBindings.SelectionTextRange
/// A cross-page character range or a single-page start and count to select.
public typealias SelectionInput = EmbedPDFBindings.SelectionInput
/// One oriented visual line of selected text in page coordinates.
public typealias SelectionSegment = EmbedPDFBindings.SelectionSegment
/// A selection boundary positioned on the oriented cell of its boundary glyph.
public typealias SelectionEndpoint = EmbedPDFBindings.SelectionEndpoint
/// Page geometry to which selection-scoped floating UI can attach.
public typealias SelectionAnchor = EmbedPDFBindings.SelectionAnchor
/// The direction in which the selection extends from its starting boundary.
public typealias SelectionDirection = EmbedPDFBindings.SelectionDirection
/// The selected geometry and range information for one page.
public typealias SelectionSnapshotPage = EmbedPDFBindings.SelectionSnapshotPage
/// The current selection, its page geometry, and an input that can restore its range.
public typealias SelectionSnapshot = EmbedPDFBindings.SelectionSnapshot
/// The operation or gesture that caused a selection change.
public typealias SelectionChangeTrigger = EmbedPDFBindings.SelectionChangeTrigger
/// An event describing selection changes, commits, and clears.
public typealias SelectionEvent = EmbedPDFBindings.SelectionEvent
/// An event describing a selection range or geometry change.
public typealias SelectionChangedEvent = EmbedPDFBindings.SelectionChangedEvent
/// An event describing a gesture ending with a selection.
public typealias SelectionCommittedEvent = EmbedPDFBindings.SelectionCommittedEvent
/// An event describing a selection being cleared.
public typealias SelectionClearedEvent = EmbedPDFBindings.SelectionClearedEvent

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

  /// Replaces the selection with the supplied engine-character range or single-page span.
  public func select(_ input: SelectionInput) async throws(PDFError) {
    try await bridged { try await handle.select(input: input) }
  }

  /// Select a search hit's characters.
  public func select(_ hit: SearchHit) async throws(PDFError) {
    try await select(.span(page: hit.page, start: Int64(hit.charStart), count: Int64(hit.charCount)))
  }

  /// Selects text across every page of the document.
  public func selectAll() async throws(PDFError) {
    try await bridged { try await handle.selectAll() }
  }

  /// Selects the text on the supplied page.
  public func selectPage(_ page: PageRef) async throws(PDFError) {
    try await bridged { try await handle.selectPage(page: page) }
  }

  /// Selects the word at a point in intrinsic PDF page coordinates; returns false when no text is selectable.
  public func selectWord(at point: Point, on page: PageRef) async throws(PDFError) -> Bool {
    try await bridged { try await handle.selectWordAt(page: page, point: point) }
  }

  /// Selects the visual line at a point in intrinsic page coordinates; returns false when no
  /// line can be selected.
  public func selectLine(at point: Point, on page: PageRef) async throws(PDFError) -> Bool {
    try await bridged { try await handle.selectLineAt(page: page, point: point) }
  }

  /// Extends the current selection to a point in intrinsic page coordinates on the supplied page.
  public func extend(to point: Point, on page: PageRef) async throws(PDFError) {
    try await bridged { try await handle.extendTo(page: page, point: point) }
  }

  /// Clears the document's text selection.
  public func clear() async throws(PDFError) {
    try await bridged { try await handle.clear() }
  }

  /// Returns whether any text is currently selected.
  public func hasSelection() async throws(PDFError) -> Bool {
    try await bridged { try await handle.hasSelection() }
  }

  /// Returns the current range, page geometry, and selection state for rendering or restoring a selection.
  public func snapshot() async throws(PDFError) -> SelectionSnapshot {
    try await bridged { try await handle.snapshot() }
  }

  /// Returns the current half-open range in PDFium character space, or nil when nothing is selected.
  public func range() async throws(PDFError) -> SelectionTextRange? {
    try await bridged { try await handle.range() }
  }

  /// Returns the page identities carrying selected text.
  public func selectedPages() async throws(PDFError) -> [PageRef] {
    try await bridged { try await handle.selectedPages() }
  }

  /// Returns the selected visual-line segments on the page in intrinsic page coordinates.
  public func segments(on page: PageRef) async throws(PDFError) -> [SelectionSegment] {
    try await bridged { try await handle.segments(page: page) }
  }

  /// Returns selected highlight rectangles on the page in intrinsic page coordinates.
  public func rects(on page: PageRef) async throws(PDFError) -> [Rect] {
    try await bridged { try await handle.rects(page: page) }
  }

  /// Returns the page geometry for attaching selection UI, or nil when no anchor is available.
  public func anchor() async throws(PDFError) -> SelectionAnchor? {
    try await bridged { try await handle.anchor() }
  }

  /// The selected text, pages joined with a newline. Throws
  /// `permission-denied` without `doc.text.copy`, and
  /// `operation-cancelled` when the task is cancelled.
  public func readText() async throws(PDFError) -> String {
    try await bridgedCancellable { token in try await handle.readText(cancel: token) }
  }

  /// Reads text for a half-open engine-character range, leaving the selection alone.
  ///
  /// - Throws: `permission-denied` without `doc.text.copy`, or `operation-cancelled`
  ///   when the task is cancelled.
  public func readText(in range: SelectionTextRange) async throws(PDFError) -> String {
    try await bridgedCancellable { token in try await handle.readTextInRange(range: range, cancel: token) }
  }

  /// Loads the page's text geometry for selection and hit-testing when needed.
  public func ensureLoaded(_ page: PageRef) async throws(PDFError) {
    try await bridged { try await handle.ensureLoaded(page: page) }
  }

  /// Returns whether this page's selection text geometry has been loaded.
  public func isLoaded(_ page: PageRef) async throws(PDFError) -> Bool {
    try await bridged { try await handle.isLoaded(page: page) }
  }

  /// Tests whether an intrinsic page-space point falls over selectable text on the page.
  public func isOverText(_ point: Point, on page: PageRef) async throws(PDFError) -> Bool {
    try await bridged { try await handle.isOverText(page: page, point: point) }
  }

  /// Begins a programmatic selection gesture; pair with ``endGesture()``.
  public func beginGesture() async throws(PDFError) {
    try await bridged { try await handle.beginGesture() }
  }

  /// Starts a selection gesture at an intrinsic page-space point; returns false off text.
  public func beginGesture(at point: Point, on page: PageRef) async throws(PDFError) -> Bool {
    try await bridged { try await handle.beginGestureAt(page: page, point: point) }
  }

  /// Ends the selection gesture, allowing the current selection to be committed.
  public func endGesture() async throws(PDFError) {
    try await bridged { try await handle.endGesture() }
  }

  /// Returns whether a selection gesture is in progress.
  public func isGestureActive() async throws(PDFError) -> Bool {
    try await bridged { try await handle.isGestureActive() }
  }

  /// Changes whether selection highlights are visible without changing the selected range.
  public func setHighlightVisible(_ visible: Bool) async throws(PDFError) {
    try await bridged { try await handle.setHighlightVisible(visible: visible) }
  }

  /// Returns whether selection highlights are currently visible.
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
  /// Returns this document's text selection capability; enable interaction and selection in
  /// the viewer configuration.
  public func selection(for document: DocumentInfo) async throws(PDFError) -> SelectionCapability {
    let handle = try await bridged { try await host.selection(documentId: document.id) }
    return SelectionCapability(handle: handle, viewer: self)
  }
}
