public import EmbedPDFBindings
public import Foundation

public typealias FormFieldRef = EmbedPDFBindings.FormFieldRef
public typealias FormFieldFamily = EmbedPDFBindings.FormFieldFamily
public typealias FormConfig = EmbedPDFBindings.FormConfig
public typealias FormValidation = EmbedPDFBindings.FormValidation
public typealias FormKind = EmbedPDFBindings.FormKind
public typealias FormSnapshot = EmbedPDFBindings.FormSnapshot
public typealias FormField = EmbedPDFBindings.FormField
public typealias FormFieldFlags = EmbedPDFBindings.FormFieldFlags
public typealias FormFieldDetail = EmbedPDFBindings.FormFieldDetail
public typealias FormWidget = EmbedPDFBindings.FormWidget
public typealias FormToggleWidget = EmbedPDFBindings.FormToggleWidget
public typealias FormChoiceOption = EmbedPDFBindings.FormChoiceOption
public typealias FormChoiceOptionInput = EmbedPDFBindings.FormChoiceOptionInput
public typealias FormValueInput = EmbedPDFBindings.FormValueInput
public typealias FormValueEntry = EmbedPDFBindings.FormValueEntry
public typealias FormFillItem = EmbedPDFBindings.FormFillItem
public typealias FormFillControl = EmbedPDFBindings.FormFillControl
public typealias FormWidgetHit = EmbedPDFBindings.FormWidgetHit
public typealias FormCommitStatus = EmbedPDFBindings.FormCommitStatus
public typealias FormCommitResult = EmbedPDFBindings.FormCommitResult
public typealias FormBatchResult = EmbedPDFBindings.FormBatchResult
public typealias FormBatchFailure = EmbedPDFBindings.FormBatchFailure
public typealias FormFilter = EmbedPDFBindings.FormFilter
public typealias FormCreateFieldInput = EmbedPDFBindings.FormCreateFieldInput
public typealias FormCreatedField = EmbedPDFBindings.FormCreatedField
public typealias FormFieldPatch = EmbedPDFBindings.FormFieldPatch
public typealias FormFieldPatchKey = EmbedPDFBindings.FormFieldPatchKey
public typealias FormDataFormat = EmbedPDFBindings.FormDataFormat
public typealias FormDataExport = EmbedPDFBindings.FormDataExport
public typealias FormActivationResult = EmbedPDFBindings.FormActivationResult
public typealias FormEvent = EmbedPDFBindings.FormEvent

/// A document's AcroForm. Get one with `PDFViewer.form(for:)`; the viewer
/// needs `ViewerConfig(interaction: true, form: FormConfig())`. It keeps the
/// viewer alive. After its document closes, every call throws
/// `instance-closed`.
public struct FormCapability: Sendable {
  let handle: FormHandle
  let viewer: PDFViewer

  public func snapshot() async throws(PDFError) -> FormSnapshot? {
    try await bridged { try await handle.snapshot() }
  }

  public func status() async throws(PDFError) -> ResourceStatus {
    try await bridged { try await handle.status() }
  }

  public func refresh() async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.refresh(cancel: token) }
  }

  public func field(_ ref: FormFieldRef) async throws(PDFError) -> FormField? {
    try await bridged { try await handle.field(fieldRef: ref) }
  }

  public func field(forWidget widget: AnnotationRef) async throws(PDFError) -> FormField? {
    try await bridged { try await handle.fieldForWidget(widget: widget) }
  }

  public func fields(_ filter: FormFilter? = nil) async throws(PDFError) -> [FormField] {
    try await bridged { try await handle.fields(filter: filter) }
  }

  public func value(of ref: FormFieldRef) async throws(PDFError) -> FormValueInput? {
    try await bridged { try await handle.value(fieldRef: ref) }
  }

  /// One control per widget on a page: what a native form overlay draws.
  public func fillItems(on page: PageRef) async throws(PDFError) -> [FormFillItem] {
    try await bridged { try await handle.fillItems(page: page) }
  }

  public func fillItem(_ annotObjectNumber: UInt32) async throws(PDFError) -> FormFillItem? {
    try await bridged { try await handle.fillItem(annotObjectNumber: annotObjectNumber) }
  }

  public func widgets(on page: PageRef) async throws(PDFError) -> [FormFillItem] {
    try await bridged { try await handle.widgets(page: page) }
  }

  public func ensureLoaded(_ page: PageRef) async throws(PDFError) {
    try await bridged { try await handle.ensureLoaded(page: page) }
  }

  public func widget(at point: Point, on page: PageRef) async throws(PDFError) -> FormWidgetHit? {
    try await bridged { try await handle.widgetAt(page: page, x: point.x, y: point.y) }
  }

  public func pageBox(_ page: PageRef) async throws(PDFError) -> Rect? {
    try await bridged { try await handle.pageBox(page: page) }
  }

  public func setValue(_ value: FormValueInput, for ref: FormFieldRef) async throws(PDFError) -> FormCommitResult {
    try await bridgedCancellable { token in try await handle.setValue(fieldRef: ref, value: value, cancel: token) }
  }

  public func setText(_ text: String, for ref: FormFieldRef) async throws(PDFError) -> FormCommitResult {
    try await bridgedCancellable { token in try await handle.setText(fieldRef: ref, text: text, cancel: token) }
  }

  /// A checkbox or radio by its on-state (`FormToggleWidget.onState`);
  /// `nil` clears the group.
  public func setChecked(_ onState: String?, for ref: FormFieldRef) async throws(PDFError) -> FormCommitResult {
    try await bridgedCancellable { token in
      try await handle.setChecked(fieldRef: ref, onState: onState, cancel: token)
    }
  }

  public func setChoice(_ values: [String], for ref: FormFieldRef) async throws(PDFError) -> FormCommitResult {
    try await bridgedCancellable { token in try await handle.setChoice(fieldRef: ref, values: values, cancel: token) }
  }

  public func setValues(_ entries: [FormValueEntry]) async throws(PDFError) -> FormBatchResult {
    try await bridgedCancellable { token in try await handle.setValues(entries: entries, cancel: token) }
  }

  public func importValues(_ values: [String: FormValueInput]) async throws(PDFError) -> FormBatchResult {
    try await bridgedCancellable { token in try await handle.importValues(values: values, cancel: token) }
  }

  public func reset(_ ref: FormFieldRef) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.reset(fieldRef: ref, cancel: token) }
  }

  public func resetAll(_ fields: [FormFieldRef]? = nil, excluding: Bool = false) async throws(PDFError) -> FormBatchResult {
    try await bridgedCancellable { token in
      try await handle.resetAll(fields: fields, exclude: excluding, cancel: token)
    }
  }

  public func createField(_ input: FormCreateFieldInput) async throws(PDFError) -> FormCreatedField {
    try await bridgedCancellable { token in try await handle.createField(input: input, cancel: token) }
  }

  public func updateField(_ ref: FormFieldRef, _ patch: FormFieldPatch) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.updateField(fieldRef: ref, patch: patch, cancel: token) }
  }

  public func deleteField(_ ref: FormFieldRef) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.deleteField(fieldRef: ref, cancel: token) }
  }

  public func detachWidget(_ widget: AnnotationRef, from ref: FormFieldRef) async throws(PDFError) {
    try await bridgedCancellable { token in
      try await handle.detachWidget(fieldRef: ref, widget: widget, cancel: token)
    }
  }

  public func attachWidget(_ widget: AnnotationRef, to ref: FormFieldRef) async throws(PDFError) {
    try await bridgedCancellable { token in
      try await handle.attachWidget(fieldRef: ref, widget: widget, cancel: token)
    }
  }

  /// `nil` is XFDF.
  public func exportData(_ format: FormDataFormat? = nil) async throws(PDFError) -> FormDataExport {
    try await bridgedCancellable { token in try await handle.exportData(format: format, cancel: token) }
  }

  /// FDF or XFDF; `nil` sniffs the format from the bytes.
  public func importData(_ bytes: Data, format: FormDataFormat? = nil) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.importData(bytes: bytes, format: format, cancel: token) }
  }

  public func repair(bakeAppearances: Bool = false) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.repair(bakeAppearances: bakeAppearances, cancel: token) }
  }

  /// A tap on a widget: toggles a checkbox, runs a pushbutton's action.
  public func activate(_ widget: AnnotationRef) async throws(PDFError) -> FormActivationResult {
    try await bridgedCancellable { token in try await handle.activateWidget(widget: widget, cancel: token) }
  }

  public func canRead() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canRead() }
  }

  public func canFill() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canFill() }
  }

  public func canDesign() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canDesign() }
  }

  /// Confirmed changes. Finishes when the document closes (at once if it
  /// already has) or the viewer is destroyed.
  public var events: AsyncStream<FormEvent> {
    viewer.registry.stream { handle.subscribe(listener: $0) }
  }
}

extension EventRelay: FormEventListener where Event == FormEvent {
  func onEvent(event: FormEvent) {
    continuation.yield(event)
  }

  func onEnd() {
    continuation.finish()
  }
}

extension PDFViewer {
  public func form(for document: PDFDocument) async throws(PDFError) -> FormCapability {
    let handle = try await bridged { try await host.form(documentId: document.id) }
    return FormCapability(handle: handle, viewer: self)
  }
}
