public import EmbedPDFBindings

public typealias MeasurementConfig = EmbedPDFBindings.MeasurementConfig
public typealias MeasurementDefaultScale = EmbedPDFBindings.MeasurementDefaultScale
public typealias PdfMeasurement = EmbedPDFBindings.PdfMeasurement
public typealias PdfNumberFormat = EmbedPDFBindings.PdfNumberFormat
public typealias MeasureFraction = EmbedPDFBindings.MeasureFraction
public typealias MeasureLabelPosition = EmbedPDFBindings.MeasureLabelPosition
public typealias ScalePreset = EmbedPDFBindings.ScalePreset
public typealias MeasurementScaleChangeOptions = EmbedPDFBindings.MeasurementScaleChangeOptions
public typealias MeasurementPageTarget = EmbedPDFBindings.MeasurementPageTarget
public typealias MeasurementPageScale = EmbedPDFBindings.MeasurementPageScale
public typealias MeasurementScaleSource = EmbedPDFBindings.MeasurementScaleSource
public typealias MeasurementErrorInfo = EmbedPDFBindings.MeasurementErrorInfo
public typealias MeasurementReading = EmbedPDFBindings.MeasurementReading
public typealias MeasurementReadout = EmbedPDFBindings.MeasurementReadout
public typealias MeasurementReadoutKind = EmbedPDFBindings.MeasurementReadoutKind
public typealias MeasurementUnavailable = EmbedPDFBindings.MeasurementUnavailable
public typealias MeasurementCalibrationRequest = EmbedPDFBindings.MeasurementCalibrationRequest
public typealias MeasurementCalibrateInput = EmbedPDFBindings.MeasurementCalibrateInput
public typealias MeasurementCalibrateOptions = EmbedPDFBindings.MeasurementCalibrateOptions
public typealias MeasurementKind = EmbedPDFBindings.MeasurementKind
public typealias CreateMeasurementInput = EmbedPDFBindings.CreateMeasurementInput
public typealias ScaleChangeReport = EmbedPDFBindings.ScaleChangeReport
public typealias ScaleRecalibration = EmbedPDFBindings.ScaleRecalibration
public typealias ScaleSkippedAnnotation = EmbedPDFBindings.ScaleSkippedAnnotation
public typealias ScaleSkipReason = EmbedPDFBindings.ScaleSkipReason
public typealias ScaleFailedAnnotation = EmbedPDFBindings.ScaleFailedAnnotation
public typealias MeasurementEvent = EmbedPDFBindings.MeasurementEvent

/// A document's page scales, calibration and measurement readouts. Get one
/// with `PDFViewer.measurement(for:)`; the viewer needs
/// `ViewerConfig(interaction: true, annotation: "", measurement:
/// MeasurementConfig())`. It keeps the viewer alive. After its
/// document closes, every call throws `instance-closed`.
public struct MeasurementCapability: Sendable {
  let handle: MeasurementHandle
  let viewer: PDFViewer

  public func canMeasure(_ page: PageRef) async throws(PDFError) -> Bool {
    try await bridged { try await handle.canMeasure(page: page) }
  }

  /// `ready` is false until `ensureLoaded(_:)` has read the page's viewports.
  public func pageScale(_ page: PageRef) async throws(PDFError) -> MeasurementPageScale {
    try await bridged { try await handle.pageScale(page: page) }
  }

  public func isBusy() async throws(PDFError) -> Bool {
    try await bridged { try await handle.isBusy() }
  }

  public func lastReports() async throws(PDFError) -> [ScaleChangeReport] {
    try await bridged { try await handle.lastReports() }
  }

  public func presets() async throws(PDFError) -> [ScalePreset] {
    try await bridged { try await handle.presets() }
  }

  public func units() async throws(PDFError) -> [String] {
    try await bridged { try await handle.units() }
  }

  public func areaUnits() async throws(PDFError) -> [String] {
    try await bridged { try await handle.areaUnits() }
  }

  /// A measurement annotation's value in its own scale. `annotationJSON`
  /// is the engine's `AnnotationRef` record.
  public func readout(annotationJSON: String) async throws(PDFError) -> MeasurementReading {
    try await bridged { try await handle.readout(annotationJson: annotationJSON) }
  }

  public func measureDistance(on page: PageRef, from: Point, to: Point) async throws(PDFError)
    -> MeasurementReading
  {
    try await bridged { try await handle.measureDistance(page: page, from: from, to: to) }
  }

  public func measureArea(on page: PageRef, vertices: [Point]) async throws(PDFError)
    -> MeasurementReading
  {
    try await bridged { try await handle.measureArea(page: page, vertices: vertices) }
  }

  public func calibrationRequest() async throws(PDFError) -> MeasurementCalibrationRequest? {
    try await bridged { try await handle.calibrationRequest() }
  }

  /// One report per page. Cancelling the task stops waiting; a page write
  /// already sent still lands.
  public func setScale(
    _ pages: MeasurementPageTarget, measure: PdfMeasurement,
    options: MeasurementScaleChangeOptions = MeasurementScaleChangeOptions()
  ) async throws(PDFError) -> [ScaleChangeReport] {
    try await bridgedCancellable { token in
      try await handle.setScale(pages: pages, measure: measure, options: options, cancel: token)
    }
  }

  public func calibrate(
    _ input: MeasurementCalibrateInput,
    options: MeasurementCalibrateOptions = MeasurementCalibrateOptions()
  ) async throws(PDFError) -> [ScaleChangeReport] {
    try await bridgedCancellable { token in
      try await handle.calibrate(input: input, options: options, cancel: token)
    }
  }

  public func setUnit(
    _ pages: MeasurementPageTarget, unit: String, options: MeasurementScaleChangeOptions = MeasurementScaleChangeOptions()
  ) async throws(PDFError) -> [ScaleChangeReport] {
    try await bridgedCancellable { token in
      try await handle.setUnit(pages: pages, unit: unit, options: options, cancel: token)
    }
  }

  public func setAreaUnit(
    _ pages: MeasurementPageTarget, unit: String, options: MeasurementScaleChangeOptions = MeasurementScaleChangeOptions()
  ) async throws(PDFError) -> [ScaleChangeReport] {
    try await bridgedCancellable { token in
      try await handle.setAreaUnit(pages: pages, unit: unit, options: options, cancel: token)
    }
  }

  public func setPrecision(
    _ pages: MeasurementPageTarget, precision: Double,
    options: MeasurementScaleChangeOptions = MeasurementScaleChangeOptions()
  ) async throws(PDFError) -> [ScaleChangeReport] {
    try await bridgedCancellable { token in
      try await handle.setPrecision(pages: pages, precision: precision, options: options, cancel: token)
    }
  }

  public func setPreset(
    _ pages: MeasurementPageTarget, presetId: String,
    options: MeasurementScaleChangeOptions = MeasurementScaleChangeOptions()
  ) async throws(PDFError) -> [ScaleChangeReport] {
    try await bridgedCancellable { token in
      try await handle.setPreset(pages: pages, presetId: presetId, options: options, cancel: token)
    }
  }

  /// Removes EmbedPDF's scale; another producer's stays.
  public func clearScale(
    _ pages: MeasurementPageTarget, options: MeasurementScaleChangeOptions = MeasurementScaleChangeOptions()
  ) async throws(PDFError) -> [ScaleChangeReport] {
    try await bridgedCancellable { token in
      try await handle.clearScale(pages: pages, options: options, cancel: token)
    }
  }

  public func startCalibration() async throws(PDFError) {
    try await bridged { try await handle.startCalibration() }
  }

  public func dismissCalibration() async throws(PDFError) {
    try await bridged { try await handle.dismissCalibration() }
  }

  /// Create a measurement annotation through the annotation plugin.
  /// Resolves the new annotation's `AnnotationRef` record as the engine's
  /// JSON.
  public func createMeasurement(_ input: CreateMeasurementInput) async throws(PDFError) -> String {
    try await bridgedCancellable { token in
      try await handle.createMeasurement(input: input, cancel: token)
    }
  }

  public func canCalibrate() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canCalibrate() }
  }

  public func ensureLoaded(_ page: PageRef) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.ensureLoaded(page: page, cancel: token) }
  }

  /// Finishes when the document closes (at once if it already has) or the
  /// viewer is destroyed.
  public var events: AsyncStream<MeasurementEvent> {
    viewer.registry.stream { handle.subscribe(listener: $0) }
  }
}

extension EventRelay: MeasurementEventListener where Event == MeasurementEvent {
  func onEvent(event: MeasurementEvent) {
    continuation.yield(event)
  }

  func onEnd() {
    continuation.finish()
  }
}

extension PDFViewer {
  public func measurement(for document: DocumentInfo) async throws(PDFError) -> MeasurementCapability {
    let handle = try await bridged { try await host.measurement(documentId: document.id) }
    return MeasurementCapability(handle: handle, viewer: self)
  }
}
