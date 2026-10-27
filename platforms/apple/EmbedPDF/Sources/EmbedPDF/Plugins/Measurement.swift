public import EmbedPDFBindings

/// Default page scale and presets offered by measurement tools.
public typealias MeasurementConfig = EmbedPDFBindings.MeasurementConfig
/// The metric, imperial or explicit fallback scale for uncalibrated pages.
public typealias MeasurementDefaultScale = EmbedPDFBindings.MeasurementDefaultScale
/// A PDF measurement dictionary describing ratios, units and number formats.
public typealias PdfMeasurement = EmbedPDFBindings.PdfMeasurement
/// Unit conversion and number formatting for a measurement axis.
public typealias PdfNumberFormat = EmbedPDFBindings.PdfNumberFormat
/// The fraction presentation used in a measurement number format.
public typealias MeasureFraction = EmbedPDFBindings.MeasureFraction
/// The position of a unit label relative to a measurement value.
public typealias MeasureLabelPosition = EmbedPDFBindings.MeasureLabelPosition
/// A named paper-to-real-world ratio offered in a scale picker.
public typealias ScalePreset = EmbedPDFBindings.ScalePreset
/// Controls whether changing a page scale recalculates its measurement annotations.
public typealias MeasurementScaleChangeOptions = EmbedPDFBindings.MeasurementScaleChangeOptions
/// One page, a list of pages or every page to receive a scale change.
public typealias MeasurementPageTarget = EmbedPDFBindings.MeasurementPageTarget
/// A page's resolved scale, its source, loading readiness and persistence support.
public typealias MeasurementPageScale = EmbedPDFBindings.MeasurementPageScale
/// Whether the resolved scale is owned by EmbedPDF, foreign or a fallback.
public typealias MeasurementScaleSource = EmbedPDFBindings.MeasurementScaleSource
/// The error details associated with a failed measurement operation.
public typealias MeasurementErrorInfo = EmbedPDFBindings.MeasurementErrorInfo
/// A calculated readout or the reason a measurement is unavailable.
public typealias MeasurementReading = EmbedPDFBindings.MeasurementReading
/// A measurement's numeric value, units, kind and formatted display text.
public typealias MeasurementReadout = EmbedPDFBindings.MeasurementReadout
/// The distance, perimeter or area represented by a readout.
public typealias MeasurementReadoutKind = EmbedPDFBindings.MeasurementReadoutKind
/// The reason a measurement cannot currently produce a value.
public typealias MeasurementUnavailable = EmbedPDFBindings.MeasurementUnavailable
/// Two captured page-space points awaiting a known real-world distance.
public typealias MeasurementCalibrationRequest = EmbedPDFBindings.MeasurementCalibrationRequest
/// A page-space segment and its known real-world length and unit.
public typealias MeasurementCalibrateInput = EmbedPDFBindings.MeasurementCalibrateInput
/// The target pages and recalculation policy for a calibration.
public typealias MeasurementCalibrateOptions = EmbedPDFBindings.MeasurementCalibrateOptions
/// The distance, perimeter or area annotation to create.
public typealias MeasurementKind = EmbedPDFBindings.MeasurementKind
/// A target page and page-space points defining a measurement annotation.
public typealias CreateMeasurementInput = EmbedPDFBindings.CreateMeasurementInput
/// The scale-write and annotation-recalculation results for one page.
public typealias ScaleChangeReport = EmbedPDFBindings.ScaleChangeReport
/// The annotations updated, skipped or failed while recalculating a page.
public typealias ScaleRecalibration = EmbedPDFBindings.ScaleRecalibration
/// A measurement annotation left unchanged and the reason it was skipped.
public typealias ScaleSkippedAnnotation = EmbedPDFBindings.ScaleSkippedAnnotation
/// Why recalculation left a measurement annotation unchanged.
public typealias ScaleSkipReason = EmbedPDFBindings.ScaleSkipReason
/// A measurement annotation and its recalculation error.
public typealias ScaleFailedAnnotation = EmbedPDFBindings.ScaleFailedAnnotation
/// A scale change or calibration lifecycle event.
public typealias MeasurementEvent = EmbedPDFBindings.MeasurementEvent

/// A document's page scales, calibration and measurement readouts. Get one
/// with `PDFViewer.measurement(for:)`; the viewer needs
/// `ViewerConfig(interaction: true, annotation: "", measurement:
/// MeasurementConfig())`. It keeps the viewer alive. After its
/// document closes, every call throws `instance-closed`.
public struct MeasurementCapability: Sendable {
  let handle: MeasurementHandle
  let viewer: PDFViewer

  /// Returns whether annotation creation is allowed and the page's measurement scale is ready.
  public func canMeasure(_ page: PageRef) async throws(PDFError) -> Bool {
    try await bridged { try await handle.canMeasure(page: page) }
  }

  /// `ready` is false until `ensureLoaded(_:)` has read the page's viewports.
  public func pageScale(_ page: PageRef) async throws(PDFError) -> MeasurementPageScale {
    try await bridged { try await handle.pageScale(page: page) }
  }

  /// Returns whether a scale change or calibration operation is in progress.
  public func isBusy() async throws(PDFError) -> Bool {
    try await bridged { try await handle.isBusy() }
  }

  /// Returns the per-page reports from the most recent scale change.
  public func lastReports() async throws(PDFError) -> [ScaleChangeReport] {
    try await bridged { try await handle.lastReports() }
  }

  /// Returns the configured scale presets offered by the scale picker.
  public func presets() async throws(PDFError) -> [ScalePreset] {
    try await bridged { try await handle.presets() }
  }

  /// Returns the supported length-unit identifiers accepted by scale and calibration operations.
  public func units() async throws(PDFError) -> [String] {
    try await bridged { try await handle.units() }
  }

  /// Returns the supported area-unit identifiers accepted by `setAreaUnit`.
  public func areaUnits() async throws(PDFError) -> [String] {
    try await bridged { try await handle.areaUnits() }
  }

  /// Calculates a measurement annotation's readout in its own scale, or returns the reason
  /// it is unavailable.
  ///
  /// - Parameter annotationJSON: The engine's `AnnotationRef` record as JSON, such as the
  ///   string ``createMeasurement(_:)`` returns.
  public func readout(annotationJSON: String) async throws(PDFError) -> MeasurementReading {
    try await bridged { try await handle.readout(annotationJson: annotationJSON) }
  }

  /// Calculates the real-world distance between two page-space points using the page's resolved scale.
  ///
  /// Returns an unavailable reading when a suitable measurement scale cannot be resolved.
  public func measureDistance(on page: PageRef, from: Point, to: Point) async throws(PDFError)
    -> MeasurementReading
  {
    try await bridged { try await handle.measureDistance(page: page, from: from, to: to) }
  }

  /// Calculates the real-world area of a page-space polygon using the page's resolved scale.
  ///
  /// Returns an unavailable reading when the geometry or scale cannot produce an area.
  public func measureArea(on page: PageRef, vertices: [Point]) async throws(PDFError)
    -> MeasurementReading
  {
    try await bridged { try await handle.measureArea(page: page, vertices: vertices) }
  }

  /// Returns the captured segment awaiting a known length, or `nil` when no calibration input is pending.
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

  /// Derives a page scale from a segment and its known real-world length, then reports each target page's outcome.
  ///
  /// Throws `invalid-input` for a zero-length segment. The options can widen the change beyond the input page.
  public func calibrate(
    _ input: MeasurementCalibrateInput,
    options: MeasurementCalibrateOptions = MeasurementCalibrateOptions()
  ) async throws(PDFError) -> [ScaleChangeReport] {
    try await bridgedCancellable { token in
      try await handle.calibrate(input: input, options: options, cancel: token)
    }
  }

  /// Changes the length unit on target pages and returns their scale-write and recalculation outcomes.
  ///
  /// Use an identifier returned by `units()`.
  public func setUnit(
    _ pages: MeasurementPageTarget, unit: String, options: MeasurementScaleChangeOptions = MeasurementScaleChangeOptions()
  ) async throws(PDFError) -> [ScaleChangeReport] {
    try await bridgedCancellable { token in
      try await handle.setUnit(pages: pages, unit: unit, options: options, cancel: token)
    }
  }

  /// Changes the area unit on target pages and returns their scale-write and recalculation outcomes.
  ///
  /// Use an identifier returned by `areaUnits()`.
  public func setAreaUnit(
    _ pages: MeasurementPageTarget, unit: String, options: MeasurementScaleChangeOptions = MeasurementScaleChangeOptions()
  ) async throws(PDFError) -> [ScaleChangeReport] {
    try await bridgedCancellable { token in
      try await handle.setAreaUnit(pages: pages, unit: unit, options: options, cancel: token)
    }
  }

  /// Changes measurement number precision on target pages and returns per-page outcomes.
  ///
  /// - Parameters:
  ///   - pages: The pages whose configured number precision to change.
  ///   - precision: A power of ten from `1` through `1_000_000_000`;
  ///     for example, `100` displays two decimal places. Other values are invalid.
  ///   - options: Controls recalculation of existing measurements after the scale change.
  public func setPrecision(
    _ pages: MeasurementPageTarget, precision: Double,
    options: MeasurementScaleChangeOptions = MeasurementScaleChangeOptions()
  ) async throws(PDFError) -> [ScaleChangeReport] {
    try await bridgedCancellable { token in
      try await handle.setPrecision(pages: pages, precision: precision, options: options, cancel: token)
    }
  }

  /// Applies the preset identified by `presetId` to target pages and returns per-page outcomes.
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

  /// Activates the calibration interaction tool to capture a page-space segment.
  ///
  /// Does nothing without calibration authority. Once both points are captured,
  /// the tool returns to the pointer and `calibrationRequest()` supplies the segment.
  public func startCalibration() async throws(PDFError) {
    try await bridged { try await handle.startCalibration() }
  }

  /// Clears the pending calibration request.
  public func dismissCalibration() async throws(PDFError) {
    try await bridged { try await handle.dismissCalibration() }
  }

  /// Creates a distance, perimeter or area annotation through the annotation plugin.
  ///
  /// Load the page scale with `ensureLoaded(_:)` first; an unavailable scale throws `not-ready`.
  ///
  /// - Returns: The new annotation's `AnnotationRef` record as the engine's JSON; pass it to
  ///   ``readout(annotationJSON:)``.
  public func createMeasurement(_ input: CreateMeasurementInput) async throws(PDFError) -> String {
    try await bridgedCancellable { token in
      try await handle.createMeasurement(input: input, cancel: token)
    }
  }

  /// Returns whether `doc.annotate.modify` permits writing page measurement scales.
  public func canCalibrate() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canCalibrate() }
  }

  /// Loads the page's measurement viewports and waits until its scale can be resolved.
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
  /// Returns the measurement capability bound to an open document.
  ///
  /// The viewer must have the `measurement` plugin enabled. The capability retains the viewer
  /// and becomes invalid when the document closes.
  ///
  /// - Throws: `PDFError` if the plugin or document scope is unavailable.
  public func measurement(for document: DocumentInfo) async throws(PDFError) -> MeasurementCapability {
    let handle = try await bridged { try await host.measurement(documentId: document.id) }
    return MeasurementCapability(handle: handle, viewer: self)
  }
}
