public import EmbedPDFBindings

public typealias FormSettings = EmbedPDFBindings.FormSettings
public typealias FormSettingsPatch = EmbedPDFBindings.FormSettingsPatch
public typealias FormFocusSettings = EmbedPDFBindings.FormFocusSettings
public typealias FormFocusSettingsPatch = EmbedPDFBindings.FormFocusSettingsPatch
public typealias FormFieldSettings = EmbedPDFBindings.FormFieldSettings
public typealias FormFieldSettingsPatch = EmbedPDFBindings.FormFieldSettingsPatch
public typealias FormFillable = EmbedPDFBindings.FormFillable
public import Foundation

public typealias FormFieldPanelSettings = EmbedPDFBindings.FormFieldPanelSettings
public typealias FormSettingProperty = EmbedPDFBindings.FormSettingProperty
public typealias FormSettingControl = EmbedPDFBindings.FormSettingControl
public typealias FormValidationResult = EmbedPDFBindings.FormValidationResult
public typealias FormFieldRef = EmbedPDFBindings.FormFieldRef
public typealias FormFieldFamily = EmbedPDFBindings.FormFieldFamily
public typealias FormConfig = EmbedPDFBindings.FormConfig
public typealias FormValidation = EmbedPDFBindings.FormValidation
public typealias FormKind = EmbedPDFBindings.FormKind
public typealias FormSnapshot = EmbedPDFBindings.FormSnapshot
public typealias FormField = EmbedPDFBindings.FormField
public typealias FormFieldDetail = EmbedPDFBindings.FormFieldDetail
public typealias FormWidget = EmbedPDFBindings.FormWidget
public typealias FormWidgetRecord = EmbedPDFBindings.FormWidgetRecord
public typealias FormCalculationPosition = EmbedPDFBindings.FormCalculationPosition
public typealias FormToggleWidget = EmbedPDFBindings.FormToggleWidget
public typealias FormChoiceOption = EmbedPDFBindings.FormChoiceOption
public typealias FormChoiceOptionInput = EmbedPDFBindings.FormChoiceOptionInput
public typealias FormValueInput = EmbedPDFBindings.FormValueInput
public typealias FormValueEntry = EmbedPDFBindings.FormValueEntry
public typealias FormFillItem = EmbedPDFBindings.FormFillItem
public typealias FormFillControl = EmbedPDFBindings.FormFillControl
public typealias FormWidgetHit = EmbedPDFBindings.FormWidgetHit
public typealias FormWidgetPlacement = EmbedPDFBindings.FormWidgetPlacement
public typealias FormWidgetAdded = EmbedPDFBindings.FormWidgetAdded
public typealias FormCommitStatus = EmbedPDFBindings.FormCommitStatus
public typealias FormCommitResult = EmbedPDFBindings.FormCommitResult
public typealias FormSetValueStatus = EmbedPDFBindings.FormSetValueStatus
public typealias FormSetValueResult = EmbedPDFBindings.FormSetValueResult
public typealias FormBatchResult = EmbedPDFBindings.FormBatchResult
public typealias FormBatchFailure = EmbedPDFBindings.FormBatchFailure
public typealias FormBatchSkip = EmbedPDFBindings.FormBatchSkip
public typealias FormFilter = EmbedPDFBindings.FormFilter
public typealias FormCreateFieldInput = EmbedPDFBindings.FormCreateFieldInput
public typealias FormFieldPatch = EmbedPDFBindings.FormFieldPatch
public typealias FormFieldPatchKey = EmbedPDFBindings.FormFieldPatchKey
public typealias FormImportResult = EmbedPDFBindings.FormImportResult
public typealias FormActivationResult = EmbedPDFBindings.FormActivationResult
public typealias FormEvent = EmbedPDFBindings.FormEvent
public typealias FormFocusOptions = EmbedPDFBindings.FormFocusOptions
public typealias FormFocusNextOptions = EmbedPDFBindings.FormFocusNextOptions
public typealias FormFocusTarget = EmbedPDFBindings.FormFocusTarget
public typealias FormWidgetEvent = EmbedPDFBindings.FormWidgetEvent

/// A document's AcroForm. Get one with `PDFViewer.form(for:)`; the viewer
/// needs `ViewerConfig(interaction: true, form: FormConfig())`. It keeps the
/// viewer alive. After its document closes, every call throws
/// `instance-closed`.
public struct FormCapability: Sendable {

  /// Current settings shared by every document using this plugin registration.
  public func settings() async throws(PDFError) -> FormSettings {
    try await bridged { try await handle.settings() }
  }

  /// Merge the supplied fields. Clear flags remove nullable values; omission preserves them.
  public func updateSettings(_ patch: FormSettingsPatch) async throws(PDFError) {
    try await bridged { try await handle.updateSettings(patch: patch) }
  }

  /// Restore the values supplied at registration, preserving registered host resources.
  public func resetSettings() async throws(PDFError) {
    try await bridged { try await handle.resetSettings() }
  }

  public func validate() async throws(PDFError) -> FormValidationResult {
    try await bridged { try await handle.validate() }
  }

  public func canRestoreAttribution() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canRestoreAttribution() }
  }

  public func canWriteScripts() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canWriteScripts() }
  }

  let handle: FormHandle
  let viewer: PDFViewer

  /// Reveal a fillable field and ask its mounted control for the cursor.
  public func focus(_ field: FormFieldRef, options: FormFocusOptions? = nil) async throws(PDFError) -> Bool {
    try await bridged { try await handle.focus(field: field, options: options) }
  }

  public func focusNext(_ options: FormFocusNextOptions? = nil) async throws(PDFError) -> Bool {
    try await bridged { try await handle.focusNext(options: options) }
  }

  public func focusedField() async throws(PDFError) -> FormField? {
    try await bridged { try await handle.focusedField() }
  }

  public func moveInGroup(_ widget: AnnotationRef, backward: Bool = false) async throws(PDFError) -> Bool {
    try await bridged { try await handle.moveInGroup(widget: widget, backward: backward) }
  }

  /// The target runs on the kernel thread. Arrange the control update without waiting there.
  public func attachFocusTarget(_ widget: AnnotationRef, target: any FormFocusTarget) async throws(PDFError) -> Registration {
    Registration(handle: try await bridged { try await handle.attachFocusTarget(widget: widget, target: target) })
  }

  public func notifyWidgetEvent(_ field: FormFieldRef, widget: AnnotationRef, event: FormWidgetEvent) async throws(PDFError) {
    try await bridged { try await handle.notifyWidgetEvent(field: field, widget: widget, event: event) }
  }

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

  public func widget(_ ref: AnnotationRef) async throws(PDFError) -> FormWidgetRecord? {
    try await bridged { try await handle.widget(widget: ref) }
  }

  /// The field's widget annotation rows, in the field's order.
  public func widgets(of field: FormFieldRef) async throws(PDFError) -> [FormWidgetRecord] {
    try await bridged { try await handle.fieldWidgets(fieldRef: field) }
  }

  /// The field of the single selected widget in annotation design mode.
  public func selectedField() async throws(PDFError) -> FormField? {
    try await bridged { try await handle.selectedField() }
  }

  @discardableResult
  /// Cancellation ends the wait; Rust still reconciles a change already admitted by the kernel.
  public func reorderCalculations(_ fields: [FormFieldRef], to position: FormCalculationPosition) async throws(PDFError) -> [FormFieldRef?] {
    try await bridgedCancellable { token in try await handle.reorderCalculations(fields: fields, position: position, cancel: token) }
  }

  /// The engine's open FormWidgetPatch JSON payload.
  @discardableResult
  public func updateWidget(_ widget: AnnotationRef, _ patchJSON: String) async throws(PDFError) -> FormWidgetRecord {
    try await bridgedCancellable { token in try await handle.updateWidget(widget: widget, patchJson: patchJSON, cancel: token) }
  }

  /// Delete a widget from its page and field; a detached widget has no remaining field.
  @discardableResult
  public func deleteWidget(_ widget: AnnotationRef) async throws(PDFError) -> FormField? {
    try await bridgedCancellable { token in try await handle.deleteWidget(widget: widget, cancel: token) }
  }

  public func value(of ref: FormFieldRef) async throws(PDFError) -> FormValueInput? {
    try await bridged { try await handle.value(fieldRef: ref) }
  }

  /// Every value by full name, as plain data, JSON (`exportValues()`).
  public func exportValues() async throws(PDFError) -> String {
    try await bridged { try await handle.exportValues() }
  }

  /// One control per widget on a page: what a native form overlay draws.
  public func widgets(on page: PageRef) async throws(PDFError) -> [FormFillItem] {
    try await bridged { try await handle.widgets(page: page) }
  }

  /// Every page's widgets come with the form: resolves once it is read.
  public func ensureLoaded(_ page: PageRef) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.ensureLoaded(page: page, cancel: token) }
  }

  public func widget(at point: Point, on page: PageRef) async throws(PDFError) -> FormWidgetHit? {
    try await bridged { try await handle.widgetAt(page: page, x: point.x, y: point.y) }
  }

  public func pageBox(_ page: PageRef) async throws(PDFError) -> Rect? {
    try await bridged { try await handle.pageBox(page: page) }
  }

  /// A text value (`.value`), a checkbox or radio (`.checked`, or `.value`
  /// with a radio's export value) or a choice (`.selectedValues`).
  public func setValue(_ value: FormValueInput, for ref: FormFieldRef) async throws(PDFError)
    -> FormSetValueResult
  {
    try await bridgedCancellable { token in try await handle.setValue(fieldRef: ref, value: value, cancel: token) }
  }

  public func setValues(_ entries: [FormValueEntry]) async throws(PDFError) -> FormBatchResult {
    try await bridgedCancellable { token in try await handle.setValues(entries: entries, cancel: token) }
  }

  /// Plain values by full name, as JSON (what `exportValues()` gives).
  public func importValues(_ valuesJSON: String) async throws(PDFError) -> FormBatchResult {
    try await bridgedCancellable { token in try await handle.importValues(valuesJson: valuesJSON, cancel: token) }
  }

  /// Each field's default, for the fields given, or every field you may
  /// fill in. Resolves the fields it changed.
  @discardableResult
  public func reset(_ fields: [FormFieldRef]? = nil) async throws(PDFError) -> [FormField] {
    try await bridgedCancellable { token in try await handle.reset(fields: fields, cancel: token) }
  }

  /// Family-specific panel descriptors and current values, resolved by Rust.
  public func fieldSettings(_ ref: FormFieldRef) async throws(PDFError) -> FormFieldPanelSettings? {
    try await bridged { try await handle.fieldSettings(fieldRef: ref) }
  }

  /// The engine's FormFieldDraft, including family options and actions/scripts.
  public func create(draftJSON: String) async throws(PDFError) -> FormField {
    try await bridgedCancellable { token in try await handle.createDraft(draftJson: draftJSON, cancel: token) }
  }

  /// The engine's FormFieldPatch. Omission preserves a value; null clears it.
  @discardableResult
  public func update(_ ref: FormFieldRef, patchJSON: String) async throws(PDFError) -> FormField {
    try await bridgedCancellable { token in try await handle.updatePatch(fieldRef: ref, patchJson: patchJSON, cancel: token) }
  }

  public func addWidget(to ref: FormFieldRef, placementJSON: String) async throws(PDFError) -> FormWidgetAdded {
    try await bridgedCancellable { token in try await handle.addWidgetPlacement(fieldRef: ref, placementJson: placementJSON, cancel: token) }
  }

  public func create(_ input: FormCreateFieldInput) async throws(PDFError) -> FormField {
    try await bridgedCancellable { token in try await handle.create(input: input, cancel: token) }
  }

  /// The field in one more place (`addWidget`).
  public func addWidget(to ref: FormFieldRef, _ placement: FormWidgetPlacement) async throws(PDFError)
    -> FormWidgetAdded
  {
    try await bridgedCancellable { token in
      try await handle.addWidget(fieldRef: ref, placement: placement, cancel: token)
    }
  }

  @discardableResult
  public func update(_ ref: FormFieldRef, _ patch: FormFieldPatch) async throws(PDFError) -> FormField {
    try await bridgedCancellable { token in try await handle.update(fieldRef: ref, patch: patch, cancel: token) }
  }

  public func delete(_ ref: FormFieldRef) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.delete(fieldRef: ref, cancel: token) }
  }

  /// One widget out of a field: it stays on its page, in no field.
  @discardableResult
  public func removeWidget(_ widget: AnnotationRef, from ref: FormFieldRef) async throws(PDFError)
    -> FormField
  {
    try await bridgedCancellable { token in
      try await handle.removeWidget(fieldRef: ref, widget: widget, cancel: token)
    }
  }

  /// Fields out as a bundle (`FormBundle`), JSON: every field, or the ones
  /// named.
  public func export(_ fields: [FormFieldRef]? = nil) async throws(PDFError) -> String {
    try await bridgedCancellable { token in try await handle.export(fields: fields, cancel: token) }
  }

  /// A bundle's fields copied into this document. `optionsJSON` is
  /// `FormImportOptions`.
  public func `import`(_ bundleJSON: String, optionsJSON: String = "{}") async throws(PDFError)
    -> FormImportResult
  {
    try await bridgedCancellable { token in
      try await handle.import(bundleJson: bundleJSON, optionsJson: optionsJSON, cancel: token)
    }
  }

  /// Fix a form other apps read differently. Resolves the repair's result as
  /// JSON (`FormRepairResult`).
  @discardableResult
  public func repair(bakeAppearances: Bool = false) async throws(PDFError) -> String {
    try await bridgedCancellable { token in try await handle.repair(bakeAppearances: bakeAppearances, cancel: token) }
  }

  /// A tap on a widget: toggles a checkbox, runs a pushbutton's action.
  public func activate(_ widget: AnnotationRef) async throws(PDFError) -> FormActivationResult {
    try await bridgedCancellable { token in try await handle.activateWidget(widget: widget, cancel: token) }
  }

  public func canRead() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canRead() }
  }

  public func canFill(_ ref: FormFieldRef) async throws(PDFError) -> Bool {
    try await bridged { try await handle.canFill(fieldRef: ref) }
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
  public func form(for document: DocumentInfo) async throws(PDFError) -> FormCapability {
    let handle = try await bridged { try await host.form(documentId: document.id) }
    return FormCapability(handle: handle, viewer: self)
  }
}
