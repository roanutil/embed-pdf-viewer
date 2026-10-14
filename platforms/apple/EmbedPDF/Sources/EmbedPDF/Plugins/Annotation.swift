public import EmbedPDFBindings
public import Foundation

// `AnnotationRef`, `AnnotationRevisionToken`, `PdfLinkTarget` and
// `PdfDestination` are declared in Link.swift.
public typealias AnnotationFlags = EmbedPDFBindings.AnnotationFlags
public typealias AnnotationFlagsPatch = EmbedPDFBindings.AnnotationFlagsPatch
public typealias AnnotationPropKey = EmbedPDFBindings.AnnotationPropKey
public typealias AnnotationProps = EmbedPDFBindings.AnnotationProps
public typealias AnnotationPropsPatch = EmbedPDFBindings.AnnotationPropsPatch
public typealias AnnotationLineEnding = EmbedPDFBindings.AnnotationLineEnding
public typealias AnnotationLineEndings = EmbedPDFBindings.AnnotationLineEndings
public typealias AnnotationLineEndingsPatch = EmbedPDFBindings.AnnotationLineEndingsPatch
public typealias AnnotationBlendMode = EmbedPDFBindings.AnnotationBlendMode
public typealias AnnotationTextAlign = EmbedPDFBindings.AnnotationTextAlign
public typealias AnnotationBorder = EmbedPDFBindings.AnnotationBorder
public typealias AnnotationCallout = EmbedPDFBindings.AnnotationCallout
public typealias CreationDraftAnchor = EmbedPDFBindings.CreationDraftAnchor
public typealias Annotation = EmbedPDFBindings.Annotation
public typealias AnnotationGeometry = EmbedPDFBindings.AnnotationGeometry
public typealias AnnotationGeometryPatch = EmbedPDFBindings.AnnotationGeometryPatch
public typealias AnnotationPatch = EmbedPDFBindings.AnnotationPatch
public typealias CreateAnnotationInput = EmbedPDFBindings.CreateAnnotationInput
public typealias AnnotationFilter = EmbedPDFBindings.AnnotationFilter
public typealias AnnotationHydration = EmbedPDFBindings.AnnotationHydration
public typealias AnnotationBatchResult = EmbedPDFBindings.AnnotationBatchResult
public typealias AnnotationBatchFailure = EmbedPDFBindings.AnnotationBatchFailure
public typealias AnnotationCreateBatchResult = EmbedPDFBindings.AnnotationCreateBatchResult
public typealias AnnotationCreateFailure = EmbedPDFBindings.AnnotationCreateFailure
public typealias AnnotationSnapSettings = EmbedPDFBindings.AnnotationSnapSettings
public typealias AnnotationSnapSettingsPatch = EmbedPDFBindings.AnnotationSnapSettingsPatch
public typealias AnnotationConfig = EmbedPDFBindings.AnnotationConfig
public typealias AnnotationEvent = EmbedPDFBindings.AnnotationEvent
public typealias AnnotationChangedEvent = EmbedPDFBindings.AnnotationChangedEvent
public typealias AnnotationDeletedEvent = EmbedPDFBindings.AnnotationDeletedEvent
public typealias AnnotationResyncedPages = EmbedPDFBindings.AnnotationResyncedPages
public typealias AnnotationThreadChange = EmbedPDFBindings.AnnotationThreadChange
public typealias AnnotationSelectionProps = EmbedPDFBindings.AnnotationSelectionProps
public typealias AnnotationTextFormat = EmbedPDFBindings.AnnotationTextFormat
public typealias AnnotationQuarterTurn = EmbedPDFBindings.AnnotationQuarterTurn
public typealias AnnotationTool = EmbedPDFBindings.AnnotationTool
public typealias AnnotationPropSpec = EmbedPDFBindings.AnnotationPropSpec
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
/// the viewer needs `ViewerConfig(interaction: true, annotation:
/// AnnotationConfig())`. It keeps the viewer alive. After its document
/// closes, every call throws `instance-closed`.
///
/// Pointer editing goes through the interaction hub: forward samples with
/// `InteractionCapability.dispatchPointer` and the plugin's own handlers
/// draw, select and move annotations under the active tool.
public struct AnnotationCapability: Sendable {
  let handle: AnnotationHandle
  let viewer: PDFViewer

  /// Select, group, rotate and restyle the selected annotations.
  public var selection: AnnotationSelectionCapability {
    AnnotationSelectionCapability(handle: handle.selection())
  }

  /// The authoring tools, their defaults and the snap settings.
  public var tools: AnnotationToolsCapability {
    AnnotationToolsCapability(handle: handle.tools())
  }

  /// Creation drafts, text editing, markup from text, stamps and placement.
  public var drafts: AnnotationDraftsCapability {
    AnnotationDraftsCapability(handle: handle.drafts())
  }

  /// Comment threads and the links attached to annotations.
  public var threads: AnnotationThreadsCapability {
    AnnotationThreadsCapability(handle: handle.threads())
  }

  public func get(_ ref: AnnotationRef) async throws(PDFError) -> Annotation? {
    try await bridged { try await handle.get(annotationRef: ref) }
  }

  public func list(_ filter: AnnotationFilter? = nil) async throws(PDFError) -> [Annotation] {
    try await bridged { try await handle.list(filter: filter) }
  }

  public func selected() async throws(PDFError) -> [Annotation] {
    try await bridged { try await handle.selected() }
  }

  /// The topmost annotation under a page-space point.
  public func hitTest(at point: Point, on page: PageRef) async throws(PDFError) -> AnnotationRef? {
    try await bridged { try await handle.hitTestAt(page: page, point: point) }
  }

  /// `"loading"`, `"ready"`, `"forbidden"` or `"error"`.
  public func status() async throws(PDFError) -> String {
    try await bridged { try await handle.status() }
  }

  public func hydration() async throws(PDFError) -> AnnotationHydration {
    try await bridged { try await handle.hydration() }
  }

  /// Start hydration if it has not run, without waiting for it.
  public func ensureHydrated() async throws(PDFError) {
    try await bridged { try await handle.ensureHydrated() }
  }

  /// Re-read every annotation and wait for it.
  public func refresh() async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.refresh(cancel: token) }
  }

  public func reload(_ page: PageRef) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.reloadPage(page: page, cancel: token) }
  }

  /// Resolves once the engine confirmed the annotation.
  public func create(_ input: CreateAnnotationInput) async throws(PDFError) -> AnnotationRef {
    try await bridgedCancellable { token in try await handle.create(input: input, cancel: token) }
  }

  public func create(_ inputs: [CreateAnnotationInput]) async throws(PDFError)
    -> AnnotationCreateBatchResult
  {
    try await bridgedCancellable { token in
      try await handle.createMany(inputs: inputs, cancel: token)
    }
  }

  public func update(_ ref: AnnotationRef, _ patch: AnnotationPatch) async throws(PDFError) {
    try await bridgedCancellable { token in
      try await handle.update(annotationRef: ref, patch: patch, cancel: token)
    }
  }

  /// One patch applied to every ref, best effort per ref.
  public func update(_ refs: [AnnotationRef], _ patch: AnnotationPatch) async throws(PDFError)
    -> AnnotationBatchResult
  {
    try await bridgedCancellable { token in
      try await handle.updateMany(refs: refs, patch: patch, cancel: token)
    }
  }

  public func setRotation(_ ref: AnnotationRef, degrees: Double) async throws(PDFError) {
    try await bridgedCancellable { token in
      try await handle.setRotation(annotationRef: ref, degrees: degrees, cancel: token)
    }
  }

  public func rotate(_ ref: AnnotationRef, by delta: Double) async throws(PDFError) {
    try await bridgedCancellable { token in
      try await handle.rotateBy(annotationRef: ref, delta: delta, cancel: token)
    }
  }

  public func delete(_ ref: AnnotationRef) async throws(PDFError) {
    try await bridgedCancellable { token in
      try await handle.delete(annotationRef: ref, cancel: token)
    }
  }

  public func delete(_ refs: [AnnotationRef]) async throws(PDFError) -> AnnotationBatchResult {
    try await bridgedCancellable { token in try await handle.deleteMany(refs: refs, cancel: token) }
  }

  public func canRead() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canRead() }
  }

  public func canCreate() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canCreate() }
  }

  public func canEdit(_ ref: AnnotationRef) async throws(PDFError) -> Bool {
    try await bridged { try await handle.canEdit(annotationRef: ref) }
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

  public func select(_ refs: [AnnotationRef], adding: Bool = false) async throws(PDFError) {
    try await bridged { try await handle.select(refs: refs, add: adding) }
  }

  /// Every annotation, or every one on `page`.
  public func selectAll(on page: PageRef? = nil) async throws(PDFError) {
    try await bridged { try await handle.selectAll(page: page) }
  }

  public func select(in rect: Rect, on page: PageRef, adding: Bool = false) async throws(PDFError)
  {
    try await bridged { try await handle.selectInRect(page: page, rect: rect, add: adding) }
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

  public func update(_ patch: AnnotationPropsPatch) async throws(PDFError) -> AnnotationBatchResult
  {
    try await bridgedCancellable { token in try await handle.update(patch: patch, cancel: token) }
  }

  public func updateFlags(_ patch: AnnotationFlagsPatch) async throws(PDFError)
    -> AnnotationBatchResult
  {
    try await bridgedCancellable { token in
      try await handle.updateFlags(patch: patch, cancel: token)
    }
  }

  public func delete() async throws(PDFError) -> AnnotationBatchResult {
    try await bridgedCancellable { token in try await handle.delete(cancel: token) }
  }

  public func rotate(_ turn: AnnotationQuarterTurn) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.rotateBy(turn: turn, cancel: token) }
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

  /// Activate one through `InteractionCapability.activateTool(_:)`.
  public func tools() async throws(PDFError) -> [AnnotationTool] {
    try await bridged { try await handle.tools() }
  }

  public func tool(_ id: String) async throws(PDFError) -> AnnotationTool? {
    try await bridged { try await handle.tool(id: id) }
  }

  public func defaults(for toolID: String) async throws(PDFError) -> AnnotationProps {
    try await bridged { try await handle.defaults(toolId: toolID) }
  }

  public func setDefaults(_ patch: AnnotationPropsPatch, for toolID: String) async throws(PDFError)
  {
    try await bridged { try await handle.setDefaults(toolId: toolID, patch: patch) }
  }

  public func propSpecs(for toolID: String) async throws(PDFError) -> [AnnotationPropSpec] {
    try await bridged { try await handle.propSpecs(toolId: toolID) }
  }

  public func snapSettings() async throws(PDFError) -> AnnotationSnapSettings {
    try await bridged { try await handle.snapSettings() }
  }

  public func updateSnapSettings(_ patch: AnnotationSnapSettingsPatch) async throws(PDFError) {
    try await bridged { try await handle.updateSnapSettings(patch: patch) }
  }
}

public struct AnnotationDraftsCapability: Sendable {
  let handle: AnnotationDraftsHandle

  public func creationDraft() async throws(PDFError) -> CreationDraftAnchor? {
    try await bridged { try await handle.creationDraft() }
  }

  public func hasCreationDraft() async throws(PDFError) -> Bool {
    try await bridged { try await handle.hasCreationDraft() }
  }

  /// Commit the polygon or polyline being drawn. `nil` when there was
  /// nothing to finish.
  public func finishCreationDraft() async throws(PDFError) -> AnnotationRef? {
    try await bridgedCancellable { token in try await handle.finishCreationDraft(cancel: token) }
  }

  public func finishInkDraft() async throws(PDFError) {
    try await bridged { try await handle.finishInkDraft() }
  }

  public func cancelCreationDraft() async throws(PDFError) {
    try await bridged { try await handle.cancelCreationDraft() }
  }

  /// Drop every gesture in progress: drafts, ghosts, placement previews.
  public func cancel() async throws(PDFError) {
    try await bridged { try await handle.cancel() }
  }

  public func setContents(_ text: String, of ref: AnnotationRef) async throws(PDFError) {
    try await bridgedCancellable { token in
      try await handle.setContents(annotationRef: ref, text: text, cancel: token)
    }
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

  public func endTextEdit() async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.endTextEdit(cancel: token) }
  }

  public func editingRef() async throws(PDFError) -> AnnotationRef? {
    try await bridged { try await handle.editingRef() }
  }

  public func editingID() async throws(PDFError) -> String? {
    try await bridged { try await handle.editingId() }
  }

  /// Keystroke-level text, kept as a draft until `endTextEdit`.
  public func draftContents(_ text: String, of ref: AnnotationRef) async throws(PDFError) {
    try await bridged { try await handle.draftContents(annotationRef: ref, text: text) }
  }

  /// `range` is in UTF-16 code units, as `NSRange` is. `nil` clears it.
  public func setTextSelection(_ range: Range<Int>?, in ref: AnnotationRef) async throws(PDFError)
  {
    try await bridged {
      try await handle.setTextSelection(
        annotationRef: ref, start: range.map { UInt32($0.lowerBound) },
        end: range.map { UInt32($0.upperBound) })
    }
  }

  /// Highlight, underline, squiggly or strikeout over text quads. The new
  /// annotation arrives on `AnnotationCapability.events`.
  public func createMarkup(
    _ subtype: String, quads: [TextQuad], on page: PageRef, preset: String? = nil
  ) async throws(PDFError) {
    try await bridged {
      try await handle.createMarkup(page: page, subtype: subtype, quads: quads, preset: preset)
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
    quads: [TextQuad], caretAt anchor: AnnotationTextEndAnchor, on page: PageRef,
    preset: String? = nil
  ) async throws(PDFError) {
    try await bridged {
      try await handle.createReplaceText(page: page, quads: quads, anchor: anchor, preset: preset)
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

  /// Mark the current text selection (needs `ViewerConfig.selection`),
  /// one annotation per page. Empty when nothing is selected. `clear`
  /// drops the text selection afterwards.
  public func createFromSelection(
    _ subtype: String, preset: String? = nil, clear: Bool? = nil
  ) async throws(PDFError) -> [AnnotationRef] {
    try await bridgedCancellable { token in
      try await handle.createFromSelection(
        subtype: subtype, preset: preset, clear: clear, cancel: token)
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

  /// Place a stamp at a point, without arming.
  public func placeStamp(_ source: AnnotationStampSource, at placement: AnnotationStampPlacement)
    async throws(PDFError) -> AnnotationRef
  {
    try await bridgedCancellable { token in
      try await handle.placeStamp(source: source, placement: placement, cancel: token)
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
    tool toolID: String, at point: Point, on page: PageRef, displayRotation: Double? = nil
  ) async throws(PDFError) {
    try await bridged {
      try await handle.hoverGhostAt(
        toolId: toolID, page: page, point: point, displayRotation: displayRotation)
    }
  }

  public func clearGhost() async throws(PDFError) {
    try await bridged { try await handle.clearGhost() }
  }

  public func setPlacementPreview(tool toolID: String, bounds: Rect, on page: PageRef)
    async throws(PDFError)
  {
    try await bridged {
      try await handle.setPlacementPreview(toolId: toolID, page: page, bounds: bounds)
    }
  }

  public func clearPlacementPreview() async throws(PDFError) {
    try await bridged { try await handle.clearPlacementPreview() }
  }

  /// Click-place with the active tool (a note, a stamp). `false` when the
  /// tool places nothing on click.
  public func place(at point: Point, on page: PageRef, displayRotation: Double? = nil)
    async throws(PDFError) -> Bool
  {
    try await bridged {
      try await handle.placeAt(page: page, point: point, displayRotation: displayRotation)
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

  /// The link attached to an annotation (a shape that acts as a link).
  public func link(_ ref: AnnotationRef) async throws(PDFError) -> PdfLinkTarget? {
    try await bridged { try await handle.link(annotationRef: ref) }
  }

  public func setLink(_ target: PdfLinkTarget, on ref: AnnotationRef) async throws(PDFError) {
    try await bridgedCancellable { token in
      try await handle.setLink(annotationRef: ref, target: target, cancel: token)
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
  public func annotations(for document: PDFDocument) async throws(PDFError) -> AnnotationCapability
  {
    let handle = try await bridged { try await host.annotation(documentId: document.id) }
    return AnnotationCapability(handle: handle, viewer: self)
  }
}
