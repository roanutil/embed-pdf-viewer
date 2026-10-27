public import EmbedPDFBindings
public import Foundation

// `AnnotationRef`, `PdfLinkTarget` and `PdfDestination` are declared in
// Link.swift.
//
// The plugin speaks the engine's records: a record carries its ref, page,
// subtype, rect, text members and authority typed, and the rest as JSON
// (`AnnotationRecord.fieldsJson`). Create fields, patches, tool defaults and
// selection values are the engine's field bags, passed as JSON objects.
/// The page and tool associated with a creation gesture in progress.
public typealias CreationDraftAnchor = EmbedPDFBindings.CreationDraftAnchor
/// A snapshot of an annotation: its reference, page, subtype, rect, text members and
/// authority typed, and its remaining fields as the engine's JSON (`fieldsJson`).
public typealias AnnotationRecord = EmbedPDFBindings.AnnotationRecord
/// The polygon or polyline being drawn: its page, and the draft as the engine's JSON.
public typealias AnnotationCreationDraft = EmbedPDFBindings.AnnotationCreationDraft
/// One editable property of a tool or selection as a style panel shows it: key, control,
/// label, and the rest of its description as JSON.
public typealias AnnotationProperty = EmbedPDFBindings.AnnotationProperty
/// The pre-rename name of `AnnotationRecord`, kept for one release. It
/// collides with MapKit's SwiftUI `Annotation` (iOS 17) in any file that
/// imports EmbedPDF, SwiftUI and MapKit.
@available(*, deprecated, renamed: "AnnotationRecord")
public typealias Annotation = AnnotationRecord
/// Criteria that restrict an annotation listing.
public typealias AnnotationFilter = EmbedPDFBindings.AnnotationFilter
/// Applied references and per-annotation failures from a batch mutation.
public typealias AnnotationBatchResult = EmbedPDFBindings.AnnotationBatchResult
/// An annotation reference and the error from its failed batch mutation.
public typealias AnnotationBatchFailure = EmbedPDFBindings.AnnotationBatchFailure
/// A confirmed annotation, selection, draft, text-editing or comment change.
public typealias AnnotationEvent = EmbedPDFBindings.AnnotationEvent
/// The annotation reference, page and available record associated with a confirmed change.
public typealias AnnotationChangedEvent = EmbedPDFBindings.AnnotationChangedEvent
/// The reference and page of a deleted annotation.
public typealias AnnotationDeletedEvent = EmbedPDFBindings.AnnotationDeletedEvent
/// Pages whose annotation records were replaced during synchronization.
public typealias AnnotationResyncedPages = EmbedPDFBindings.AnnotationResyncedPages
/// A change to the comment-thread projection.
public typealias AnnotationThreadChange = EmbedPDFBindings.AnnotationThreadChange
/// The shared and mixed property values of the current annotation selection.
public typealias AnnotationSelectionProps = EmbedPDFBindings.AnnotationSelectionProps
/// A text emphasis style that can be toggled on selected annotations.
public typealias AnnotationTextFormat = EmbedPDFBindings.AnnotationTextFormat
/// An authoring tool descriptor and its interaction identifier.
public typealias AnnotationTool = EmbedPDFBindings.AnnotationTool
/// The source bytes and metadata used to prepare a stamp.
public typealias AnnotationStampSource = EmbedPDFBindings.AnnotationStampSource
/// A stamp's target page, page-space location, size and rotation.
public typealias AnnotationStampPlacement = EmbedPDFBindings.AnnotationStampPlacement
/// The bytes and file metadata embedded in an attachment annotation.
public typealias AnnotationAttachmentFile = EmbedPDFBindings.AnnotationAttachmentFile
/// The page-space anchor and text direction for an insertion caret.
public typealias AnnotationTextEndAnchor = EmbedPDFBindings.AnnotationTextEndAnchor
/// Text quadrilaterals grouped by their target page.
public typealias AnnotationPageQuads = EmbedPDFBindings.AnnotationPageQuads
/// A comment projected from an annotation in a thread.
public typealias AnnotationComment = EmbedPDFBindings.AnnotationComment
/// A root annotation and the replies and status associated with it.
public typealias AnnotationCommentThread = EmbedPDFBindings.AnnotationCommentThread
/// The comment operations currently allowed for an annotation.
public typealias AnnotationCommentPermissions = EmbedPDFBindings.AnnotationCommentPermissions
/// The review state assigned to a comment thread.
public typealias AnnotationThreadStatus = EmbedPDFBindings.AnnotationThreadStatus
/// Deleted, skipped and failed entries when removing a comment thread.
public typealias AnnotationThreadDeleteResult = EmbedPDFBindings.AnnotationThreadDeleteResult
/// The comment reference and error from a failed thread deletion.
public typealias AnnotationThreadDeleteFailure = EmbedPDFBindings.AnnotationThreadDeleteFailure
/// The active tool's request for a file to place on a page.
public typealias AnnotationFilePromptRequest = EmbedPDFBindings.AnnotationFilePromptRequest
/// A file-picker result containing bytes and optional filename and MIME type.
public typealias AnnotationPickedFile = EmbedPDFBindings.AnnotationPickedFile
/// The host callback that supplies a file for a placing tool; return no file to decline.
///
/// The asynchronous picker runs off the kernel thread. Hop to the main actor to present
/// UI. Results are discarded if the user switches tools before the picker completes.
public typealias AnnotationFilePicker = EmbedPDFBindings.AnnotationFilePicker

/// A document's annotations. Get one with `PDFViewer.annotations(for:)`;
/// the viewer needs `ViewerConfig(interaction: true, annotation: "")`
/// (the settings JSON over the defaults; `""` for the defaults). It keeps
/// the viewer alive. After its document closes, every call throws
/// `instance-closed`.
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

  /// The authoring tools and their defaults.
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

  /// Returns the loaded record for `ref`, or `nil` when it is absent from the annotation plane.
  public func get(_ ref: AnnotationRef) async throws(PDFError) -> AnnotationRecord? {
    try await bridged { try await handle.get(annotationRef: ref) }
  }

  /// Lists loaded annotations matching `filter`; `nil` includes every loaded annotation.
  public func list(_ filter: AnnotationFilter? = nil) async throws(PDFError) -> [AnnotationRecord] {
    try await bridged { try await handle.list(filter: filter) }
  }

  /// Returns the loaded records for the current annotation selection.
  public func selected() async throws(PDFError) -> [AnnotationRecord] {
    try await bridged { try await handle.selected() }
  }

  /// Returns the topmost annotation under a page-space point, or `nil` over none.
  public func hitTest(at point: Point, on page: PageRef) async throws(PDFError) -> AnnotationRecord? {
    try await bridged { try await handle.hitTestAt(page: page, point: point) }
  }

  /// Returns whether a change of this annotation is still on its way to the engine.
  public func isPending(_ ref: AnnotationRef) async throws(PDFError) -> Bool {
    try await bridged { try await handle.isPending(annotationRef: ref) }
  }

  /// Returns the annotation records' loading, availability, or failure state, as metadata,
  /// form and signature report theirs.
  public func status() async throws(PDFError) -> ResourceStatus {
    try await bridged { try await handle.status() }
  }

  /// Re-reads every annotation from the engine and waits for it.
  public func refresh() async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.refresh(cancel: token) }
  }

  /// Waits until every load and page re-read in flight has settled.
  public func whenSynced() async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.whenSynced(cancel: token) }
  }

  /// Creates an annotation and returns its reference once the engine confirms it.
  ///
  /// - Parameters:
  ///   - fieldsJSON: The engine's field bag for the subtype, such as
  ///     `{"subtype": "square", "box": …}`.
  ///   - page: The page to receive the annotation.
  ///   - tool: The tool whose defaults the annotation starts from; `nil` uses the subtype's.
  ///   - select: Whether to select the annotation afterwards.
  public func create(
    _ fieldsJSON: String, on page: PageRef, tool: String? = nil, select: Bool? = nil
  ) async throws(PDFError) -> AnnotationRef {
    try await bridgedCancellable { token in
      try await handle.create(page: page, fieldsJson: fieldsJSON, tool: tool, select: select, cancel: token)
    }
  }

  /// Applies a partial mutation to one annotation and waits for engine confirmation.
  ///
  /// - Parameters:
  ///   - ref: The annotation to change.
  ///   - patchJSON: The engine's field bag; omitted fields keep their values.
  public func update(_ ref: AnnotationRef, _ patchJSON: String) async throws(PDFError) {
    try await bridgedCancellable { token in
      try await handle.update(annotationRef: ref, patchJson: patchJSON, cancel: token)
    }
  }

  /// Deletes one annotation and waits for engine confirmation.
  public func delete(_ ref: AnnotationRef) async throws(PDFError) {
    try await bridgedCancellable { token in
      try await handle.delete(annotationRef: ref, cancel: token)
    }
  }

  /// Moves annotations in their page's paint order.
  ///
  /// - Parameters:
  ///   - refs: The annotations to move.
  ///   - positionJSON: `"front"`, `"back"`, `"forward"`, `"backward"` (each a
  ///     quoted JSON string) or `{"above": ref}` / `{"below": ref}`.
  public func reorder(_ refs: [AnnotationRef], to positionJSON: String) async throws(PDFError) {
    try await bridgedCancellable { token in
      try await handle.reorder(refs: refs, positionJson: positionJSON, cancel: token)
    }
  }

  /// Returns whether `doc.annotate.read` is currently granted.
  public func canRead() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canRead() }
  }

  /// Returns whether annotation creation is currently permitted.
  public func canCreate() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canCreate() }
  }

  /// Returns whether the current authority and annotation flags allow changing `ref`.
  public func canUpdate(_ ref: AnnotationRef) async throws(PDFError) -> Bool {
    try await bridged { try await handle.canUpdate(annotationRef: ref) }
  }

  /// Returns whether the current authority and annotation flags allow deleting `ref`.
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

/// Selection and batch styling for one document's annotations.
///
/// Obtain it from `AnnotationCapability.selection`. It retains the viewer and shares
/// the parent capability's document lifetime and permission checks.
public struct AnnotationSelectionCapability: Sendable {
  let handle: AnnotationSelectionHandle
  let viewer: PDFViewer

  /// Selects `refs`, replacing the current selection unless `adding` is `true`.
  public func select(_ refs: [AnnotationRef], adding: Bool = false) async throws(PDFError) {
    try await bridged { try await handle.select(refs: refs, add: adding) }
  }

  /// Every annotation, or every one on `page`.
  public func selectAll(on page: PageRef? = nil) async throws(PDFError) {
    try await bridged { try await handle.selectAll(page: page) }
  }

  /// Selects annotations within a page-space rectangle, replacing the selection.
  public func select(in rect: Rect, on page: PageRef) async throws(PDFError) {
    try await bridged { try await handle.selectInRect(page: page, rect: rect) }
  }

  /// Clears the annotation selection without deleting annotations.
  public func clear() async throws(PDFError) {
    try await bridged { try await handle.clear() }
  }

  /// Returns the references in the current annotation selection.
  public func refs() async throws(PDFError) -> [AnnotationRef] {
    try await bridged { try await handle.refs() }
  }

  /// Returns the common and mixed property values for the current selection.
  public func props() async throws(PDFError) -> AnnotationSelectionProps {
    try await bridged { try await handle.props() }
  }

  /// Applies one patch to every selected annotation and reports individual outcomes.
  ///
  /// - Parameter patchJSON: The engine's field bag; each annotation takes the fields its
  ///   kind has.
  public func update(_ patchJSON: String) async throws(PDFError) -> AnnotationBatchResult {
    try await bridgedCancellable { token in try await handle.update(patchJson: patchJSON, cancel: token) }
  }

  /// Deletes selected annotations and reports individual outcomes.
  public func delete() async throws(PDFError) -> AnnotationBatchResult {
    try await bridgedCancellable { token in try await handle.delete(cancel: token) }
  }

  /// Adds `degrees` to each selected annotation's rotation.
  public func rotate(by degrees: Double) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.rotateBy(degrees: degrees, cancel: token) }
  }

  /// Resets the selected annotations' rotations.
  public func resetRotation() async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.resetRotation(cancel: token) }
  }

  /// Toggles a text-format flag on the selected annotations.
  public func toggle(_ format: AnnotationTextFormat) async throws(PDFError) {
    try await bridgedCancellable { token in
      try await handle.toggleTextFormat(format: format, cancel: token)
    }
  }

  /// Groups the selected annotations for subsequent selection and manipulation.
  public func group() async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.group(cancel: token) }
  }

  /// Removes grouping from the selected annotation groups.
  public func ungroup() async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.ungroup(cancel: token) }
  }

  /// Returns whether the current selection can be grouped.
  public func canGroup() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canGroup() }
  }

  /// Returns whether the current selection contains groups that can be ungrouped.
  public func canUngroup() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canUngroup() }
  }
}

/// Authoring tool descriptors and defaults for one document.
///
/// Obtain it from `AnnotationCapability.tools`. Changes to defaults affect future creations.
/// It retains the viewer and is invalid after the document closes.
public struct AnnotationToolsCapability: Sendable {
  let handle: AnnotationToolsHandle
  let viewer: PDFViewer

  /// Activate one through `InteractionCapability.activateTool(_:)`.
  public func tools() async throws(PDFError) -> [AnnotationTool] {
    try await bridged { try await handle.tools() }
  }

  /// Returns the authoring tool with `id`, or `nil` for an unknown identifier.
  public func tool(_ id: String) async throws(PDFError) -> AnnotationTool? {
    try await bridged { try await handle.tool(id: id) }
  }

  /// Returns the engine's field bag, as JSON, that new annotations created with `toolID` start with.
  public func defaults(for toolID: String) async throws(PDFError) -> String {
    try await bridged { try await handle.defaults(toolId: toolID) }
  }

  /// Merges a field bag over the defaults for future annotations created with `toolID`.
  ///
  /// - Parameters:
  ///   - changesJSON: The engine's field bag to merge.
  ///   - toolID: The tool whose defaults change.
  public func updateDefaults(_ changesJSON: String, for toolID: String) async throws(PDFError) {
    try await bridged { try await handle.updateDefaults(toolId: toolID, changesJson: changesJSON) }
  }

  /// Returns the property values a style panel shows for `toolID`.
  public func properties(for toolID: String) async throws(PDFError) -> AnnotationSelectionProps {
    try await bridged { try await handle.properties(toolId: toolID) }
  }
}

/// Transient annotation creation, text editing and placement state for one document.
///
/// Obtain it from `AnnotationCapability.drafts`. Preview and draft operations are distinct
/// from committing an annotation. It retains the viewer and is invalid after document closure.
public struct AnnotationDraftsCapability: Sendable {
  let handle: AnnotationDraftsHandle
  let viewer: PDFViewer

  /// Returns the polygon or polyline being drawn, or `nil` when no draft is active.
  public func creationDraft() async throws(PDFError) -> AnnotationCreationDraft? {
    try await bridged { try await handle.creationDraft() }
  }

  /// Commit the polygon or polyline being drawn. `nil` when there was
  /// nothing to finish.
  public func finishCreationDraft() async throws(PDFError) -> AnnotationRef? {
    try await bridgedCancellable { token in try await handle.finishCreationDraft(cancel: token) }
  }

  /// Finishes the current ink drawing gesture.
  public func finishInkDraft() async throws(PDFError) {
    try await bridged { try await handle.finishInkDraft() }
  }

  /// Drop every gesture in progress: drafts, ghosts, placement previews.
  public func cancel() async throws(PDFError) {
    try await bridged { try await handle.cancel() }
  }

  /// Begins the free-text editing session for `ref`.
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

  /// Ends the active free-text editing session and commits what was typed.
  public func endTextEdit() async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.endTextEdit(cancel: token) }
  }

  /// Returns the annotation being edited, or `nil` outside a text-editing session.
  public func editingRef() async throws(PDFError) -> AnnotationRef? {
    try await bridged { try await handle.editingRef() }
  }

  /// Keystroke-level text, sent to the engine once typing pauses or at `endTextEdit`.
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

  /// Shows temporary markup over the supplied page-space text quads without creating annotations.
  public func previewMarkup(
    _ subtype: String, pages: [AnnotationPageQuads], preset: String? = nil
  ) async throws(PDFError) {
    try await bridged {
      try await handle.previewMarkup(subtype: subtype, pages: pages, preset: preset)
    }
  }

  /// Removes temporary markup previews without changing annotations.
  public func clearMarkupPreview() async throws(PDFError) {
    try await bridged { try await handle.clearMarkupPreview() }
  }

  /// Mark the current text selection (needs `ViewerConfig.selection`) with
  /// the markup tool `toolID`, one annotation per page. Empty when nothing is
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

  /// Clears the prepared stamp so subsequent clicks do not place it.
  public func disarmStamp() async throws(PDFError) {
    try await bridged { try await handle.disarmStamp() }
  }

  /// Returns whether a prepared stamp is awaiting placement.
  public func hasArmedStamp() async throws(PDFError) -> Bool {
    try await bridged { try await handle.hasArmedStamp() }
  }

  /// Place a stamp centred on a point, without arming.
  ///
  /// - Parameters:
  ///   - source: The stamp's bytes and metadata.
  ///   - placement: The target page, centre point, size and rotation.
  ///   - targetWidth: Width in page-space points; `nil` uses the source's intrinsic size.
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

  /// Updates a tool's hover preview at a page-space point.
  public func hoverGhost(
    tool toolID: String, at point: Point, on page: PageRef, displayRotation: Double? = nil,
    zoom: Double? = nil
  ) async throws(PDFError) {
    try await bridged {
      try await handle.hoverGhostAt(
        toolId: toolID, page: page, point: point, displayRotation: displayRotation, zoom: zoom)
    }
  }

  /// Removes the current hover preview.
  public func clearGhost() async throws(PDFError) {
    try await bridged { try await handle.clearGhost() }
  }

  /// Shows provisional placement bounds for a sibling plugin's drag-to-place gesture with
  /// the tool `toolID`.
  public func previewPlacement(tool toolID: String, from: Point, to: Point, on page: PageRef)
    async throws(PDFError)
  {
    try await bridged {
      try await handle.previewPlacement(toolId: toolID, page: page, from: from, to: to)
    }
  }

  /// Removes provisional placement bounds.
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

  /// Embeds `file` in a new attachment annotation at a page-space point and returns its reference.
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

/// Comment threads and annotation link targets for one document.
///
/// Obtain it from `AnnotationCapability.threads`. Mutations use the current comment or
/// annotation authority. It retains the viewer and is invalid after document closure.
public struct AnnotationThreadsCapability: Sendable {
  let handle: AnnotationThreadsHandle
  let viewer: PDFViewer

  /// Returns the comment-thread projection of the loaded annotations.
  public func threads() async throws(PDFError) -> [AnnotationCommentThread] {
    try await bridged { try await handle.threads() }
  }

  /// The thread any member of it belongs to.
  public func thread(_ ref: AnnotationRef) async throws(PDFError) -> AnnotationCommentThread? {
    try await bridged { try await handle.thread(annotationRef: ref) }
  }

  /// Creates a reply in the thread containing `ref` and returns the reply annotation reference.
  public func reply(to ref: AnnotationRef, _ text: String) async throws(PDFError) -> AnnotationRef
  {
    try await bridgedCancellable { token in
      try await handle.reply(annotationRef: ref, text: text, cancel: token)
    }
  }

  /// Replaces the text of the comment represented by `ref`.
  public func setText(_ text: String, of ref: AnnotationRef) async throws(PDFError) {
    try await bridgedCancellable { token in
      try await handle.setText(annotationRef: ref, text: text, cancel: token)
    }
  }

  /// Sets the review status of the thread containing `ref`.
  public func setStatus(_ status: AnnotationThreadStatus, of ref: AnnotationRef)
    async throws(PDFError)
  {
    try await bridgedCancellable { token in
      try await handle.setStatus(annotationRef: ref, status: status, cancel: token)
    }
  }

  /// Sets the comment's marked state to `marked`.
  public func setMarked(_ marked: Bool, _ ref: AnnotationRef) async throws(PDFError) {
    try await bridgedCancellable { token in
      try await handle.setMarked(annotationRef: ref, marked: marked, cancel: token)
    }
  }

  /// Deletes the comment annotation identified by `ref`.
  public func delete(_ ref: AnnotationRef) async throws(PDFError) {
    try await bridgedCancellable { token in
      try await handle.delete(annotationRef: ref, cancel: token)
    }
  }

  /// Deletes the thread containing `ref` and reports removed, skipped and failed comments.
  public func deleteThread(_ ref: AnnotationRef) async throws(PDFError)
    -> AnnotationThreadDeleteResult
  {
    try await bridgedCancellable { token in
      try await handle.deleteThread(annotationRef: ref, cancel: token)
    }
  }

  /// Returns the operations currently permitted on the comment represented by `ref`.
  public func permissions(_ ref: AnnotationRef) async throws(PDFError)
    -> AnnotationCommentPermissions
  {
    try await bridged { try await handle.permissions(annotationRef: ref) }
  }

  /// The link attached to an annotation (a shape that acts as a link), as
  /// the engine's `LinkTarget` JSON, or `nil` when it has none.
  public func link(_ ref: AnnotationRef) async throws(PDFError) -> String? {
    try await bridged { try await handle.link(annotationRef: ref) }
  }

  /// Attaches a target to an annotation so it acts as a link.
  ///
  /// - Parameters:
  ///   - targetJSON: The engine's `LinkTarget` JSON.
  ///   - ref: The annotation to receive the link.
  public func setLink(_ targetJSON: String, on ref: AnnotationRef) async throws(PDFError) {
    try await bridgedCancellable { token in
      try await handle.setLink(annotationRef: ref, targetJson: targetJSON, cancel: token)
    }
  }

  /// Removes the link target attached to an annotation.
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
  /// Returns the annotation capability bound to an open document.
  ///
  /// The viewer must have the `annotation` plugin enabled. The capability retains the viewer
  /// and becomes invalid when the document closes.
  ///
  /// - Throws: `PDFError` if the plugin or document scope is unavailable.
  public func annotations(for document: DocumentInfo) async throws(PDFError) -> AnnotationCapability
  {
    let handle = try await bridged { try await host.annotation(documentId: document.id) }
    return AnnotationCapability(handle: handle, viewer: self)
  }
}
