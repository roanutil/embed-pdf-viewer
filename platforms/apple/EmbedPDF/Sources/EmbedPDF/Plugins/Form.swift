public import EmbedPDFBindings
public import Foundation

/// The durable reference used to address an AcroForm field.
public typealias FormFieldRef = EmbedPDFBindings.FormFieldRef
/// The family of form control to create.
public typealias FormFieldFamily = EmbedPDFBindings.FormFieldFamily
/// Configuration for AcroForm loading and document-script validation.
public typealias FormConfig = EmbedPDFBindings.FormConfig
/// Whether form commits run document validation and calculation scripts.
public typealias FormValidation = EmbedPDFBindings.FormValidation
/// The form technology identified by the document snapshot.
public typealias FormKind = EmbedPDFBindings.FormKind
/// A snapshot of the document's form fields and form metadata.
public typealias FormSnapshot = EmbedPDFBindings.FormSnapshot
/// A field's identity, flags, typed value and page widgets.
public typealias FormField = EmbedPDFBindings.FormField
/// Read-only, required and export flags on a form field.
public typealias FormFieldFlags = EmbedPDFBindings.FormFieldFlags
/// The type-specific value and settings of a form field.
public typealias FormFieldDetail = EmbedPDFBindings.FormFieldDetail
/// The page-space appearance and annotation identity of a field widget.
public typealias FormWidget = EmbedPDFBindings.FormWidget
/// A checkbox or radio widget's export on-state and checked state.
public typealias FormToggleWidget = EmbedPDFBindings.FormToggleWidget
/// A choice field's display label and export value.
public typealias FormChoiceOption = EmbedPDFBindings.FormChoiceOption
/// The label and value used to create or replace a choice option.
public typealias FormChoiceOptionInput = EmbedPDFBindings.FormChoiceOptionInput
/// A typed text, toggle or choice value to commit to a field.
public typealias FormValueInput = EmbedPDFBindings.FormValueInput
/// A field reference paired with the value to commit in a batch.
public typealias FormValueEntry = EmbedPDFBindings.FormValueEntry
/// The geometry, state and native control descriptor for one page widget.
public typealias FormFillItem = EmbedPDFBindings.FormFillItem
/// The type-specific control a native form overlay should display.
public typealias FormFillControl = EmbedPDFBindings.FormFillControl
/// The field and widget found under a page-space point.
public typealias FormWidgetHit = EmbedPDFBindings.FormWidgetHit
/// The outcome of a form commit, including validation refusal or no change.
public typealias FormCommitStatus = EmbedPDFBindings.FormCommitStatus
/// The status and validation information from committing a field value.
public typealias FormCommitResult = EmbedPDFBindings.FormCommitResult
/// Applied, skipped and failed form entries from a batch operation.
public typealias FormBatchResult = EmbedPDFBindings.FormBatchResult
/// The field and error associated with a failed batch entry.
public typealias FormBatchFailure = EmbedPDFBindings.FormBatchFailure
/// Criteria for listing a subset of AcroForm fields.
public typealias FormFilter = EmbedPDFBindings.FormFilter
/// The family, page-space bounds and initial settings of a new field.
public typealias FormCreateFieldInput = EmbedPDFBindings.FormCreateFieldInput
/// The references created when adding a field and its widget.
public typealias FormCreatedField = EmbedPDFBindings.FormCreatedField
/// Changes to field flags, options and other design properties.
public typealias FormFieldPatch = EmbedPDFBindings.FormFieldPatch
/// A field design property accepted by a patch.
public typealias FormFieldPatchKey = EmbedPDFBindings.FormFieldPatchKey
/// The FDF or XFDF interchange format for form values.
public typealias FormDataFormat = EmbedPDFBindings.FormDataFormat
/// Exported form bytes together with their FDF or XFDF format.
public typealias FormDataExport = EmbedPDFBindings.FormDataExport
/// Whether widget activation committed a form value or dispatched an action.
public typealias FormActivationResult = EmbedPDFBindings.FormActivationResult
/// A confirmed form value, field-tree or loading-state change.
public typealias FormEvent = EmbedPDFBindings.FormEvent

/// A document's AcroForm. Get one with `PDFViewer.form(for:)`; the viewer
/// needs `ViewerConfig(interaction: true, form: FormConfig())`. It keeps the
/// viewer alive. After its document closes, every call throws
/// `instance-closed`.
public struct FormCapability: Sendable {
  let handle: FormHandle
  let viewer: PDFViewer

  /// Returns the last loaded form snapshot, or `nil` before a snapshot is available.
  public func snapshot() async throws(PDFError) -> FormSnapshot? {
    try await bridged { try await handle.snapshot() }
  }

  /// Returns the form loading state, including permission denial and read failure.
  public func status() async throws(PDFError) -> ResourceStatus {
    try await bridged { try await handle.status() }
  }

  /// Re-reads the field tree from the engine and waits for the refresh to finish.
  public func refresh() async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.refresh(cancel: token) }
  }

  /// Returns the loaded field identified by `ref`, or `nil` if absent.
  public func field(_ ref: FormFieldRef) async throws(PDFError) -> FormField? {
    try await bridged { try await handle.field(fieldRef: ref) }
  }

  /// Returns the field owning a widget annotation, or `nil` if no field owns it.
  public func field(forWidget widget: AnnotationRef) async throws(PDFError) -> FormField? {
    try await bridged { try await handle.fieldForWidget(widget: widget) }
  }

  /// Lists loaded fields matching `filter`; `nil` includes all fields.
  public func fields(_ filter: FormFilter? = nil) async throws(PDFError) -> [FormField] {
    try await bridged { try await handle.fields(filter: filter) }
  }

  /// Returns a field's current typed value, or `nil` for an unknown field.
  public func value(of ref: FormFieldRef) async throws(PDFError) -> FormValueInput? {
    try await bridged { try await handle.value(fieldRef: ref) }
  }

  /// One control per widget on a page: what a native form overlay draws.
  public func fillItems(on page: PageRef) async throws(PDFError) -> [FormFillItem] {
    try await bridged { try await handle.fillItems(page: page) }
  }

  /// Returns the native overlay control for an annotation object number, or `nil` if it is unknown.
  public func fillItem(_ annotObjectNumber: UInt32) async throws(PDFError) -> FormFillItem? {
    try await bridged { try await handle.fillItem(annotObjectNumber: annotObjectNumber) }
  }

  /// Returns the native form overlay items for widgets on `page`.
  public func widgets(on page: PageRef) async throws(PDFError) -> [FormFillItem] {
    try await bridged { try await handle.widgets(page: page) }
  }

  /// Starts loading page widget annotations if needed, without waiting for that load to finish.
  public func ensureLoaded(_ page: PageRef) async throws(PDFError) {
    try await bridged { try await handle.ensureLoaded(page: page) }
  }

  /// Hit-tests a page-space point and returns the widget and field there, or `nil`.
  public func widget(at point: Point, on page: PageRef) async throws(PDFError) -> FormWidgetHit? {
    try await bridged { try await handle.widgetAt(page: page, x: point.x, y: point.y) }
  }

  /// Returns the cached page-space bounds used by the form overlay, or `nil` if unavailable.
  public func pageBox(_ page: PageRef) async throws(PDFError) -> Rect? {
    try await bridged { try await handle.pageBox(page: page) }
  }

  /// Commits a typed field value and returns whether it was applied, unchanged or refused by validation.
  public func setValue(_ value: FormValueInput, for ref: FormFieldRef) async throws(PDFError) -> FormCommitResult {
    try await bridgedCancellable { token in try await handle.setValue(fieldRef: ref, value: value, cancel: token) }
  }

  /// Commits text to a text field and returns the commit and validation outcome.
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

  /// Commits choice export values to a choice field and returns the commit outcome.
  public func setChoice(_ values: [String], for ref: FormFieldRef) async throws(PDFError) -> FormCommitResult {
    try await bridgedCancellable { token in try await handle.setChoice(fieldRef: ref, values: values, cancel: token) }
  }

  /// Commits field-reference/value pairs and reports individual outcomes.
  public func setValues(_ entries: [FormValueEntry]) async throws(PDFError) -> FormBatchResult {
    try await bridgedCancellable { token in try await handle.setValues(entries: entries, cancel: token) }
  }

  /// Commits values keyed by field name and reports individual outcomes.
  public func importValues(_ values: [String: FormValueInput]) async throws(PDFError) -> FormBatchResult {
    try await bridgedCancellable { token in try await handle.importValues(values: values, cancel: token) }
  }

  /// Restores one field's default value.
  public func reset(_ ref: FormFieldRef) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.reset(fieldRef: ref, cancel: token) }
  }

  /// Restores defaults for all fields or a specified subset.
  ///
  /// - Parameters:
  ///   - fields: Fields to reset; `nil` targets every field.
  ///   - excluding: When `true`, resets fields outside the supplied list.
  /// - Returns: Applied, skipped and failed field resets.
  public func resetAll(_ fields: [FormFieldRef]? = nil, excluding: Bool = false) async throws(PDFError) -> FormBatchResult {
    try await bridgedCancellable { token in
      try await handle.resetAll(fields: fields, exclude: excluding, cancel: token)
    }
  }

  /// Creates a field and its widget from the supplied design settings and returns their references.
  public func createField(_ input: FormCreateFieldInput) async throws(PDFError) -> FormCreatedField {
    try await bridgedCancellable { token in try await handle.createField(input: input, cancel: token) }
  }

  /// Changes the supplied design properties of a field.
  public func updateField(_ ref: FormFieldRef, _ patch: FormFieldPatch) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.updateField(fieldRef: ref, patch: patch, cancel: token) }
  }

  /// Deletes a field and its widgets from the form.
  public func deleteField(_ ref: FormFieldRef) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.deleteField(fieldRef: ref, cancel: token) }
  }

  /// Detaches a widget annotation from a field.
  public func detachWidget(_ widget: AnnotationRef, from ref: FormFieldRef) async throws(PDFError) {
    try await bridgedCancellable { token in
      try await handle.detachWidget(fieldRef: ref, widget: widget, cancel: token)
    }
  }

  /// Attaches a widget annotation to a field.
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

  /// Relinks inconsistent form fields and page widgets.
  ///
  /// - Parameter bakeAppearances: When `true`, generates appearance streams for
  ///   every widget so the document no longer needs `/NeedAppearances`. Defaults to `false`.
  public func repair(bakeAppearances: Bool = false) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.repair(bakeAppearances: bakeAppearances, cancel: token) }
  }

  /// A tap on a widget: toggles a checkbox, runs a pushbutton's action.
  public func activate(_ widget: AnnotationRef) async throws(PDFError) -> FormActivationResult {
    try await bridgedCancellable { token in try await handle.activateWidget(widget: widget, cancel: token) }
  }

  /// Returns whether `doc.forms.read` is currently granted.
  public func canRead() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canRead() }
  }

  /// Returns whether `doc.forms.fill` is currently granted.
  public func canFill() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canFill() }
  }

  /// Returns whether `doc.forms.modify` is currently granted.
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
  /// Returns the form capability bound to an open document.
  ///
  /// The viewer must have the `form` plugin enabled. The capability retains the viewer
  /// and becomes invalid when the document closes.
  ///
  /// - Throws: `PDFError` if the plugin or document scope is unavailable.
  public func form(for document: DocumentInfo) async throws(PDFError) -> FormCapability {
    let handle = try await bridged { try await host.form(documentId: document.id) }
    return FormCapability(handle: handle, viewer: self)
  }
}
