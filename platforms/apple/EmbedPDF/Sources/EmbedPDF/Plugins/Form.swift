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
/// Where a field's new widget goes; without bounds, next in line after the field's widgets.
public typealias FormWidgetPlacement = EmbedPDFBindings.FormWidgetPlacement
/// The field after adding a widget, and the new widget's row as the engine's JSON.
public typealias FormWidgetAdded = EmbedPDFBindings.FormWidgetAdded
/// The outcome of a form commit, including validation refusal or no change.
public typealias FormCommitStatus = EmbedPDFBindings.FormCommitStatus
/// The status and validation information from committing a field value.
public typealias FormCommitResult = EmbedPDFBindings.FormCommitResult
/// Whether a value write was applied, left the field unchanged, or was rejected by validation.
public typealias FormSetValueStatus = EmbedPDFBindings.FormSetValueStatus
/// The field as it is after a value write, and what the write did.
public typealias FormSetValueResult = EmbedPDFBindings.FormSetValueResult
/// Applied, skipped and failed form entries from a batch operation.
public typealias FormBatchResult = EmbedPDFBindings.FormBatchResult
/// The field and error associated with a failed batch entry.
public typealias FormBatchFailure = EmbedPDFBindings.FormBatchFailure
/// The field name and reason for a skipped batch entry.
public typealias FormBatchSkip = EmbedPDFBindings.FormBatchSkip
/// Criteria for listing a subset of AcroForm fields.
public typealias FormFilter = EmbedPDFBindings.FormFilter
/// The family, page-space bounds and initial settings of a new field.
public typealias FormCreateFieldInput = EmbedPDFBindings.FormCreateFieldInput
/// Changes to field flags, options and other design properties.
public typealias FormFieldPatch = EmbedPDFBindings.FormFieldPatch
/// A field design property accepted by a patch.
public typealias FormFieldPatchKey = EmbedPDFBindings.FormFieldPatchKey
/// The fields a bundle import created, its reference map and dropped items as JSON.
public typealias FormImportResult = EmbedPDFBindings.FormImportResult
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

  /// Returns every field value keyed by full name, as plain-data JSON.
  public func exportValues() async throws(PDFError) -> String {
    try await bridged { try await handle.exportValues() }
  }

  /// One control per widget on a page: what a native form overlay draws.
  /// One control per widget on a page: what a native form overlay draws.
  public func widgets(on page: PageRef) async throws(PDFError) -> [FormFillItem] {
    try await bridged { try await handle.widgets(page: page) }
  }

  /// Waits until the form, which carries every page's widgets, has been read.
  public func ensureLoaded(_ page: PageRef) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.ensureLoaded(page: page, cancel: token) }
  }

  /// Hit-tests a page-space point and returns the widget and field there, or `nil`.
  public func widget(at point: Point, on page: PageRef) async throws(PDFError) -> FormWidgetHit? {
    try await bridged { try await handle.widgetAt(page: page, x: point.x, y: point.y) }
  }

  /// Returns the cached page-space bounds used by the form overlay, or `nil` if unavailable.
  public func pageBox(_ page: PageRef) async throws(PDFError) -> Rect? {
    try await bridged { try await handle.pageBox(page: page) }
  }

  /// Commits a typed field value and returns the field as it is now, with whether the value
  /// was applied, unchanged or rejected by validation.
  ///
  /// - Parameters:
  ///   - value: A text value (`.value`), a checkbox or radio (`.checked`, or `.value`
  ///     with a radio's export value) or a choice (`.selectedValues`).
  ///   - ref: The field to write.
  public func setValue(_ value: FormValueInput, for ref: FormFieldRef) async throws(PDFError)
    -> FormSetValueResult
  {
    try await bridgedCancellable { token in try await handle.setValue(fieldRef: ref, value: value, cancel: token) }
  }

  /// Commits field-reference/value pairs and reports individual outcomes.
  public func setValues(_ entries: [FormValueEntry]) async throws(PDFError) -> FormBatchResult {
    try await bridgedCancellable { token in try await handle.setValues(entries: entries, cancel: token) }
  }

  /// Commits plain values keyed by full field name and reports individual outcomes.
  ///
  /// - Parameter valuesJSON: A JSON object of values by full name, as ``exportValues()`` returns.
  public func importValues(_ valuesJSON: String) async throws(PDFError) -> FormBatchResult {
    try await bridgedCancellable { token in try await handle.importValues(valuesJson: valuesJSON, cancel: token) }
  }

  /// Restores default values and returns the fields it changed.
  ///
  /// - Parameter fields: Fields to reset; `nil` targets every field the session may fill.
  @discardableResult
  public func reset(_ fields: [FormFieldRef]? = nil) async throws(PDFError) -> [FormField] {
    try await bridgedCancellable { token in try await handle.reset(fields: fields, cancel: token) }
  }

  /// Creates a field and its widget from the supplied design settings and returns the field.
  public func create(_ input: FormCreateFieldInput) async throws(PDFError) -> FormField {
    try await bridgedCancellable { token in try await handle.create(input: input, cancel: token) }
  }

  /// Adds another widget for an existing field and returns the field with the new widget.
  public func addWidget(to ref: FormFieldRef, _ placement: FormWidgetPlacement) async throws(PDFError)
    -> FormWidgetAdded
  {
    try await bridgedCancellable { token in
      try await handle.addWidget(fieldRef: ref, placement: placement, cancel: token)
    }
  }

  /// Changes the supplied design properties of a field and returns the updated field.
  @discardableResult
  public func update(_ ref: FormFieldRef, _ patch: FormFieldPatch) async throws(PDFError) -> FormField {
    try await bridgedCancellable { token in try await handle.update(fieldRef: ref, patch: patch, cancel: token) }
  }

  /// Deletes a field and its widgets from the form.
  public func delete(_ ref: FormFieldRef) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.delete(fieldRef: ref, cancel: token) }
  }

  /// Removes a widget annotation from a field and returns the updated field.
  ///
  /// The widget stays on its page, in no field.
  @discardableResult
  public func removeWidget(_ widget: AnnotationRef, from ref: FormFieldRef) async throws(PDFError)
    -> FormField
  {
    try await bridgedCancellable { token in
      try await handle.removeWidget(fieldRef: ref, widget: widget, cancel: token)
    }
  }

  /// Exports fields as a `FormBundle` JSON document.
  ///
  /// - Parameter fields: Fields to export; `nil` exports every field.
  public func export(_ fields: [FormFieldRef]? = nil) async throws(PDFError) -> String {
    try await bridgedCancellable { token in try await handle.export(fields: fields, cancel: token) }
  }

  /// Copies a bundle's fields into this document.
  ///
  /// - Parameters:
  ///   - bundleJSON: A `FormBundle` JSON document, as ``export(_:)`` returns.
  ///   - optionsJSON: `FormImportOptions` as JSON.
  public func `import`(_ bundleJSON: String, optionsJSON: String = "{}") async throws(PDFError)
    -> FormImportResult
  {
    try await bridgedCancellable { token in
      try await handle.import(bundleJson: bundleJSON, optionsJson: optionsJSON, cancel: token)
    }
  }

  /// Relinks inconsistent form fields and page widgets that other applications read differently.
  ///
  /// - Parameter bakeAppearances: When `true`, generates appearance streams for
  ///   every widget so the document no longer needs `/NeedAppearances`. Defaults to `false`.
  /// - Returns: The repair's result as JSON (`FormRepairResult`).
  @discardableResult
  public func repair(bakeAppearances: Bool = false) async throws(PDFError) -> String {
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

  /// Returns whether the session may fill `ref` (`doc.forms.fill`); `false` for a field the form doesn't have.
  public func canFill(_ ref: FormFieldRef) async throws(PDFError) -> Bool {
    try await bridged { try await handle.canFill(fieldRef: ref) }
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
