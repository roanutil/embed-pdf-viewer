public import EmbedPDFBindings
public import Foundation

// `AnnotationRef`, `PdfLinkTarget` and `PdfDestination` are declared in
// Link.swift.
//
// The plugin speaks the engine's records: a record carries its ref, page,
// subtype, rect, text members and authority typed, and the rest as JSON
// (`AnnotationRecord.fieldsJson`). Create fields, patches, tool defaults and
// selection values are the engine's field bags, passed as JSON objects.
public typealias CreationDraftAnchor = EmbedPDFBindings.CreationDraftAnchor
public typealias AnnotationRecord = EmbedPDFBindings.AnnotationRecord
public typealias AnnotationCreationDraft = EmbedPDFBindings.AnnotationCreationDraft
public typealias AnnotationProperty = EmbedPDFBindings.AnnotationProperty
/// The pre-rename name of `AnnotationRecord`, kept for one release. It
/// collides with MapKit's SwiftUI `Annotation` (iOS 17) in any file that
/// imports EmbedPDF, SwiftUI and MapKit.
@available(*, deprecated, renamed: "AnnotationRecord")
public typealias Annotation = AnnotationRecord
public typealias AnnotationFilter = EmbedPDFBindings.AnnotationFilter
public typealias AnnotationBatchResult = EmbedPDFBindings.AnnotationBatchResult
public typealias AnnotationBatchFailure = EmbedPDFBindings.AnnotationBatchFailure
public typealias AnnotationEvent = EmbedPDFBindings.AnnotationEvent
public typealias AnnotationChangedEvent = EmbedPDFBindings.AnnotationChangedEvent
public typealias AnnotationDeletedEvent = EmbedPDFBindings.AnnotationDeletedEvent
public typealias AnnotationResyncedPages = EmbedPDFBindings.AnnotationResyncedPages
public typealias AnnotationThreadChange = EmbedPDFBindings.AnnotationThreadChange
public typealias AnnotationSelectionProps = EmbedPDFBindings.AnnotationSelectionProps
public typealias AnnotationTextFormat = EmbedPDFBindings.AnnotationTextFormat
public typealias AnnotationTool = EmbedPDFBindings.AnnotationTool
public typealias AnnotationStampSource = EmbedPDFBindings.AnnotationStampSource
public typealias AnnotationStampPlacement = EmbedPDFBindings.AnnotationStampPlacement
public typealias AnnotationAttachmentFile = EmbedPDFBindings.AnnotationAttachmentFile
public typealias AnnotationTextEndAnchor = EmbedPDFBindings.AnnotationTextEndAnchor
public typealias AnnotationPageQuads = EmbedPDFBindings.AnnotationPageQuads
public typealias AnnotationComment = EmbedPDFBindings.AnnotationComment
public typealias AnnotationCommentThread = EmbedPDFBindings.AnnotationCommentThread
public typealias AnnotationCommentPermissions = EmbedPDFBindings.AnnotationCommentPermissions
public typealias AnnotationThreadStatus = EmbedPDFBindings.AnnotationThreadStatus
public typealias AnnotationThreadDeleteResult = EmbedPDFBindings.AnnotationThreadDeleteResult
public typealias AnnotationThreadDeleteFailure = EmbedPDFBindings.AnnotationThreadDeleteFailure
public typealias AnnotationFilePromptRequest = EmbedPDFBindings.AnnotationFilePromptRequest
public typealias AnnotationPickedFile = EmbedPDFBindings.AnnotationPickedFile
public typealias AnnotationFilePicker = EmbedPDFBindings.AnnotationFilePicker

/// A document's annotations. Get one with `PDFViewer.annotations(for:)`;
/// the viewer needs `ViewerConfig(interaction: true, annotation: "")`
/// (the settings JSON over the defaults; `""` for the defaults). It keeps the viewer alive. After its document
/// closes, every call throws `instance-closed`.
///
/// Pointer editing goes through the interaction hub: forward samples with
/// `InteractionCapability.dispatchPointer` and the plugin's own handlers
/// draw, select and move annotations under the active tool.
public struct AnnotationCapability: Sendable {
  let handle: AnnotationHandle
  let viewer: PDFViewer

  // Each sub-capability keeps the viewer alive, as this one does, so one
  // held on its own still answers. Nothing the viewer owns holds a
  // capability, so this makes no cycle; a handler the kernel keeps (a file
  // picker) that captures one does, as it would capturing this one.

  /// Select, group, rotate and restyle the selected annotations.
  public var selection: AnnotationSelectionCapability {
    AnnotationSelectionCapability(handle: handle.selection(), viewer: viewer)
  }

  /// The authoring tools, their defaults and the snap settings.
  public var tools: AnnotationToolsCapability {
    AnnotationToolsCapability(handle: handle.tools(), viewer: viewer)
  }

  /// Creation drafts, text editing, markup from text, stamps and placement.
  public var drafts: AnnotationDraftsCapability {
    AnnotationDraftsCapability(handle: handle.drafts(), viewer: viewer)
  }

  /// Comment threads and the links attached to annotations.
  public var threads: AnnotationThreadsCapability {
    AnnotationThreadsCapability(handle: handle.threads(), viewer: viewer)
  }

  public func get(_ ref: AnnotationRef) async throws(PDFError) -> AnnotationRecord? {
    try await bridged { try await handle.get(annotationRef: ref) }
  }

  public func list(_ filter: AnnotationFilter? = nil) async throws(PDFError) -> [AnnotationRecord] {
    try await bridged { try await handle.list(filter: filter) }
  }

  public func selected() async throws(PDFError) -> [AnnotationRecord] {
    try await bridged { try await handle.selected() }
  }

  /// The topmost annotation under a page-space point (`getAt`).
  public func hitTest(at point: Point, on page: PageRef) async throws(PDFError) -> AnnotationRecord? {
    try await bridged { try await handle.hitTestAt(page: page, point: point) }
  }

  /// Whether a change of this annotation is still on its way to the engine.
  public func isPending(_ ref: AnnotationRef) async throws(PDFError) -> Bool {
    try await bridged { try await handle.isPending(annotationRef: ref) }
  }

  /// The records mirror's status, as metadata, form and signature report
  /// it.
  public func status() async throws(PDFError) -> ResourceStatus {
    try await bridged { try await handle.status() }
  }

  /// Re-read every annotation from the engine and wait for it.
  public func refresh() async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.refresh(cancel: token) }
  }

  /// Every load and page re-read in flight has settled.
  public func whenSynced() async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.whenSynced(cancel: token) }
  }

  /// Resolves with the created annotation's ref once the engine confirmed
  /// it. `fieldsJSON` is the engine's field bag for the subtype
  /// (`{"subtype": "square", "box": …}`). `tool` picks the tool whose
  /// defaults it starts from; `select` selects it afterwards.
  public func create(
    _ fieldsJSON: String, on page: PageRef, tool: String? = nil, select: Bool? = nil
  ) async throws(PDFError) -> AnnotationRef {
    try await bridgedCancellable { token in
      try await handle.create(page: page, fieldsJson: fieldsJSON, tool: tool, select: select, cancel: token)
    }
  }

  /// `patchJSON` is the engine's field bag.
  public func update(_ ref: AnnotationRef, _ patchJSON: String) async throws(PDFError) {
    try await bridgedCancellable { token in
      try await handle.update(annotationRef: ref, patchJson: patchJSON, cancel: token)
    }
  }

  public func delete(_ ref: AnnotationRef) async throws(PDFError) {
    try await bridgedCancellable { token in
      try await handle.delete(annotationRef: ref, cancel: token)
    }
  }

  /// `positionJSON` is `"front"`, `"back"`, `"forward"`, `"backward"` (each
  /// a JSON string, quoted) or `{"above": ref}` / `{"below": ref}`.
  public func reorder(_ refs: [AnnotationRef], to positionJSON: String) async throws(PDFError) {
    try await bridgedCancellable { token in
      try await handle.reorder(refs: refs, positionJson: positionJSON, cancel: token)
    }
  }

  public func canRead() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canRead() }
  }

  public func canCreate() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canCreate() }
  }

  public func canUpdate(_ ref: AnnotationRef) async throws(PDFError) -> Bool {
    try await bridged { try await handle.canUpdate(annotationRef: ref) }
  }

  public func canDelete(_ ref: AnnotationRef) async throws(PDFError) -> Bool {
    try await bridged { try await handle.canDelete(annotationRef: ref) }
  }

  /// Install the file picker a placing tool (a stamp, an attachment) asks
  /// for its file. `nil` removes it; without one, such a tool declines the
  /// click. A file picked after the user switched tools is dropped.
  public func setFilePicker(_ picker: (any AnnotationFilePicker)?) async throws(PDFError) {
    try await bridged { try await handle.setFilePicker(picker: picker) }
  }

  /// Confirmed changes, whoever made them, and selection, draft, editing
  /// and comment-thread changes. Finishes when the document closes (at
  /// once if it already has) or the viewer is destroyed.
  public var events: AsyncStream<AnnotationEvent> {
    viewer.registry.stream { handle.subscribe(listener: $0) }
  }
}

public struct AnnotationSelectionCapability: Sendable {
  let handle: AnnotationSelectionHandle
  let viewer: PDFViewer

  public func select(_ refs: [AnnotationRef], adding: Bool = false) async throws(PDFError) {
    try await bridged { try await handle.select(refs: refs, add: adding) }
  }

  /// Every annotation, or every one on `page`.
  public func selectAll(on page: PageRef? = nil) async throws(PDFError) {
    try await bridged { try await handle.selectAll(page: page) }
  }

  public func select(in rect: Rect, on page: PageRef) async throws(PDFError) {
    try await bridged { try await handle.selectInRect(page: page, rect: rect) }
  }

  public func clear() async throws(PDFError) {
    try await bridged { try await handle.clear() }
  }

  public func refs() async throws(PDFError) -> [AnnotationRef] {
    try await bridged { try await handle.refs() }
  }

  public func props() async throws(PDFError) -> AnnotationSelectionProps {
    try await bridged { try await handle.props() }
  }

  /// `patchJSON` is the engine's field bag; each member takes the fields
  /// its kind has.
  public func update(_ patchJSON: String) async throws(PDFError) -> AnnotationBatchResult {
    try await bridgedCancellable { token in try await handle.update(patchJson: patchJSON, cancel: token) }
  }

  public func delete() async throws(PDFError) -> AnnotationBatchResult {
    try await bridgedCancellable { token in try await handle.delete(cancel: token) }
  }

  public func rotate(by degrees: Double) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.rotateBy(degrees: degrees, cancel: token) }
  }

  public func resetRotation() async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.resetRotation(cancel: token) }
  }

  public func toggle(_ format: AnnotationTextFormat) async throws(PDFError) {
    try await bridgedCancellable { token in
      try await handle.toggleTextFormat(format: format, cancel: token)
    }
  }

  public func group() async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.group(cancel: token) }
  }

  public func ungroup() async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.ungroup(cancel: token) }
  }

  public func canGroup() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canGroup() }
  }

  public func canUngroup() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canUngroup() }
  }
}

public struct AnnotationToolsCapability: Sendable {
  let handle: AnnotationToolsHandle
  let viewer: PDFViewer

  /// Activate one through `InteractionCapability.activateTool(_:)`.
  public func tools() async throws(PDFError) -> [AnnotationTool] {
    try await bridged { try await handle.tools() }
  }

  public func tool(_ id: String) async throws(PDFError) -> AnnotationTool? {
    try await bridged { try await handle.tool(id: id) }
  }

  /// The engine's field bag a new annotation from this tool starts with,
  /// as JSON.
  public func defaults(for toolID: String) async throws(PDFError) -> String {
    try await bridged { try await handle.defaults(toolId: toolID) }
  }

  /// `changesJSON` is a field bag merged over the tool's defaults.
  public func updateDefaults(_ changesJSON: String, for toolID: String) async throws(PDFError) {
    try await bridged { try await handle.updateDefaults(toolId: toolID, changesJson: changesJSON) }
  }

  /// What the tool's style panel shows.
  public func properties(for toolID: String) async throws(PDFError) -> AnnotationSelectionProps {
    try await bridged { try await handle.properties(toolId: toolID) }
  }
}

public struct AnnotationDraftsCapability: Sendable {
  let handle: AnnotationDraftsHandle
  let viewer: PDFViewer

  /// The polygon or polyline being drawn.
  public func creationDraft() async throws(PDFError) -> AnnotationCreationDraft? {
    try await bridged { try await handle.creationDraft() }
  }

  /// Commit the polygon or polyline being drawn. `nil` when there was
  /// nothing to finish.
  public func finishCreationDraft() async throws(PDFError) -> AnnotationRef? {
    try await bridgedCancellable { token in try await handle.finishCreationDraft(cancel: token) }
  }

  public func finishInkDraft() async throws(PDFError) {
    try await bridged { try await handle.finishInkDraft() }
  }

  /// Drop every gesture in progress: drafts, ghosts, placement previews.
  public func cancel() async throws(PDFError) {
    try await bridged { try await handle.cancel() }
  }

  public func beginTextEdit(_ ref: AnnotationRef) async throws(PDFError) {
    try await bridged { try await handle.beginTextEdit(annotationRef: ref) }
  }

  /// Start editing the free text under a page-space point. `scale`,
  /// `rotation` and `zoom` describe the view the point came from. `false`
  /// when no free text is there.
  public func beginTextEdit(
    at point: Point, on page: PageRef, scale: Double? = nil, rotation: Double? = nil,
    zoom: Double? = nil
  ) async throws(PDFError) -> Bool {
    try await bridged {
      try await handle.beginTextEditAt(
        page: page, point: point, scale: scale, rotation: rotation, zoom: zoom)
    }
  }

  /// Send what was typed and stop editing.
  public func endTextEdit() async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.endTextEdit(cancel: token) }
  }

  public func editingRef() async throws(PDFError) -> AnnotationRef? {
    try await bridged { try await handle.editingRef() }
  }

  /// Keystroke-level text, sent once typing pauses or at `endTextEdit`.
  public func draftContents(_ text: String, of ref: AnnotationRef) async throws(PDFError) {
    try await bridged { try await handle.draftContents(annotationRef: ref, text: text) }
  }

  /// `range` is in UTF-16 code units, as `NSRange` is. `nil` clears it.
  /// A range below zero or past `UInt32.max` throws `invalid-input`.
  public func setTextSelection(_ range: Range<Int>?, in ref: AnnotationRef) async throws(PDFError)
  {
    var start: UInt32?
    var end: UInt32?
    if let range {
      guard let lower = UInt32(exactly: range.lowerBound), let upper = UInt32(exactly: range.upperBound)
      else {
        throw PDFError(
          name: "RangeError", code: "invalid-input",
          detail: "text selection \(range) is outside 0..<\(UInt32.max)")
      }
      (start, end) = (lower, upper)
    }
    try await bridged {
      try await handle.setTextSelection(annotationRef: ref, start: start, end: end)
    }
  }

  /// Highlight, underline, squiggly or strikeout over text quads
  /// (`quadsJSON`, the engine's `Quad[]`). `preset` picks a tool's defaults
  /// other than the subtype's own. The new annotation arrives on
  /// `AnnotationCapability.events`.
  public func createMarkup(
    _ subtype: String, quadsJSON: String, on page: PageRef, preset: String? = nil
  ) async throws(PDFError) {
    try await bridged {
      try await handle.createMarkup(page: page, subtype: subtype, quadsJson: quadsJSON, preset: preset)
    }
  }

  /// An insert caret at the end of a run of text.
  public func createCaret(at anchor: AnnotationTextEndAnchor, on page: PageRef)
    async throws(PDFError)
  {
    try await bridged { try await handle.createCaret(page: page, anchor: anchor) }
  }

  /// A strikeout over `quads` paired with an insert caret at `anchor`.
  public func createReplaceText(
    quadsJSON: String, caretAt anchor: AnnotationTextEndAnchor, on page: PageRef,
    preset: String? = nil
  ) async throws(PDFError) {
    try await bridged {
      try await handle.createReplaceText(page: page, quadsJson: quadsJSON, anchor: anchor, preset: preset)
    }
  }

  public func previewMarkup(
    _ subtype: String, pages: [AnnotationPageQuads], preset: String? = nil
  ) async throws(PDFError) {
    try await bridged {
      try await handle.previewMarkup(subtype: subtype, pages: pages, preset: preset)
    }
  }

  public func clearMarkupPreview() async throws(PDFError) {
    try await bridged { try await handle.clearMarkupPreview() }
  }

  /// Mark the current text selection (needs `ViewerConfig.selection`) with
  /// a markup tool, one annotation per page. Empty when nothing is
  /// selected. `clear` (default `true`) drops the text selection
  /// afterwards.
  public func createFromSelection(tool toolID: String, clear: Bool? = nil)
    async throws(PDFError) -> [AnnotationRef]
  {
    try await bridgedCancellable { token in
      try await handle.createFromSelection(toolId: toolID, clear: clear, cancel: token)
    }
  }

  /// Arm a stamp: the next click on a page places it.
  public func armStamp(_ source: AnnotationStampSource, targetWidth: Double? = nil)
    async throws(PDFError)
  {
    try await bridgedCancellable { token in
      try await handle.armStamp(source: source, targetWidth: targetWidth, cancel: token)
    }
  }

  public func disarmStamp() async throws(PDFError) {
    try await bridged { try await handle.disarmStamp() }
  }

  public func hasArmedStamp() async throws(PDFError) -> Bool {
    try await bridged { try await handle.hasArmedStamp() }
  }

  /// Place a stamp centred on a point, without arming.
  public func placeStamp(
    _ source: AnnotationStampSource, at placement: AnnotationStampPlacement,
    targetWidth: Double? = nil
  ) async throws(PDFError) -> AnnotationRef {
    try await bridgedCancellable { token in
      try await handle.placeStamp(
        source: source, placement: placement, targetWidth: targetWidth, cancel: token)
    }
  }

  /// `false` when no stamp is armed. The new annotation arrives on
  /// `AnnotationCapability.events`.
  public func placeArmedStamp(at point: Point, on page: PageRef, displayRotation: Double? = nil)
    async throws(PDFError) -> Bool
  {
    try await bridged {
      try await handle.placeArmedStamp(page: page, point: point, displayRotation: displayRotation)
    }
  }

  /// Ask the file picker for the active tool's file and place it at
  /// `point`. `false` when the active tool takes no picked file or no
  /// picker is installed.
  public func requestStamp(at point: Point, on page: PageRef, displayRotation: Double? = nil)
    async throws(PDFError) -> Bool
  {
    try await bridged {
      try await handle.requestStampAt(page: page, point: point, displayRotation: displayRotation)
    }
  }

  public func hoverGhost(
    tool toolID: String, at point: Point, on page: PageRef, displayRotation: Double? = nil,
    zoom: Double? = nil
  ) async throws(PDFError) {
    try await bridged {
      try await handle.hoverGhostAt(
        toolId: toolID, page: page, point: point, displayRotation: displayRotation, zoom: zoom)
    }
  }

  public func clearGhost() async throws(PDFError) {
    try await bridged { try await handle.clearGhost() }
  }

  /// A sibling's drag-to-place gesture with one of this plugin's tools.
  public func previewPlacement(tool toolID: String, from: Point, to: Point, on page: PageRef)
    async throws(PDFError)
  {
    try await bridged {
      try await handle.previewPlacement(toolId: toolID, page: page, from: from, to: to)
    }
  }

  public func clearPlacementPreview() async throws(PDFError) {
    try await bridged { try await handle.clearPlacementPreview() }
  }

  /// Click-place with the active tool (a note, a stamp). `false` when the
  /// tool places nothing on click.
  public func place(
    at point: Point, on page: PageRef, displayRotation: Double? = nil, zoom: Double? = nil
  ) async throws(PDFError) -> Bool {
    try await bridged {
      try await handle.placeAt(page: page, point: point, displayRotation: displayRotation, zoom: zoom)
    }
  }

  public func createAttachment(_ file: AnnotationAttachmentFile, at point: Point, on page: PageRef)
    async throws(PDFError) -> AnnotationRef
  {
    try await bridgedCancellable { token in
      try await handle.createAttachment(page: page, point: point, file: file, cancel: token)
    }
  }

  /// An attachment annotation's embedded bytes.
  public func readAttachment(_ ref: AnnotationRef) async throws(PDFError) -> Data {
    try await bridgedCancellable { token in
      try await handle.readAttachment(annotationRef: ref, cancel: token)
    }
  }
}

public struct AnnotationThreadsCapability: Sendable {
  let handle: AnnotationThreadsHandle
  let viewer: PDFViewer

  public func threads() async throws(PDFError) -> [AnnotationCommentThread] {
    try await bridged { try await handle.threads() }
  }

  /// The thread any member of it belongs to.
  public func thread(_ ref: AnnotationRef) async throws(PDFError) -> AnnotationCommentThread? {
    try await bridged { try await handle.thread(annotationRef: ref) }
  }

  public func reply(to ref: AnnotationRef, _ text: String) async throws(PDFError) -> AnnotationRef
  {
    try await bridgedCancellable { token in
      try await handle.reply(annotationRef: ref, text: text, cancel: token)
    }
  }

  public func setText(_ text: String, of ref: AnnotationRef) async throws(PDFError) {
    try await bridgedCancellable { token in
      try await handle.setText(annotationRef: ref, text: text, cancel: token)
    }
  }

  public func setStatus(_ status: AnnotationThreadStatus, of ref: AnnotationRef)
    async throws(PDFError)
  {
    try await bridgedCancellable { token in
      try await handle.setStatus(annotationRef: ref, status: status, cancel: token)
    }
  }

  public func setMarked(_ marked: Bool, _ ref: AnnotationRef) async throws(PDFError) {
    try await bridgedCancellable { token in
      try await handle.setMarked(annotationRef: ref, marked: marked, cancel: token)
    }
  }

  public func delete(_ ref: AnnotationRef) async throws(PDFError) {
    try await bridgedCancellable { token in
      try await handle.delete(annotationRef: ref, cancel: token)
    }
  }

  public func deleteThread(_ ref: AnnotationRef) async throws(PDFError)
    -> AnnotationThreadDeleteResult
  {
    try await bridgedCancellable { token in
      try await handle.deleteThread(annotationRef: ref, cancel: token)
    }
  }

  public func permissions(_ ref: AnnotationRef) async throws(PDFError)
    -> AnnotationCommentPermissions
  {
    try await bridged { try await handle.permissions(annotationRef: ref) }
  }

  /// The link attached to an annotation (a shape that acts as a link), as
  /// the engine's `LinkTarget` JSON.
  public func link(_ ref: AnnotationRef) async throws(PDFError) -> String? {
    try await bridged { try await handle.link(annotationRef: ref) }
  }

  /// `targetJSON` is the engine's `LinkTarget`.
  public func setLink(_ targetJSON: String, on ref: AnnotationRef) async throws(PDFError) {
    try await bridgedCancellable { token in
      try await handle.setLink(annotationRef: ref, targetJson: targetJSON, cancel: token)
    }
  }

  public func clearLink(_ ref: AnnotationRef) async throws(PDFError) {
    try await bridgedCancellable { token in
      try await handle.clearLink(annotationRef: ref, cancel: token)
    }
  }
}

extension EventRelay: AnnotationEventListener where Event == AnnotationEvent {
  func onEvent(event: AnnotationEvent) {
    continuation.yield(event)
  }

  func onEnd() {
    continuation.finish()
  }
}

extension PDFViewer {
  public func annotations(for document: DocumentInfo) async throws(PDFError) -> AnnotationCapability
  {
    let handle = try await bridged { try await host.annotation(documentId: document.id) }
    return AnnotationCapability(handle: handle, viewer: self)
  }
}
