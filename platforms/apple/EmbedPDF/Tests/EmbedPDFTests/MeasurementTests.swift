import CustomDump
import EmbedPDF
import Testing

extension BaseSuite {
  @Suite struct MeasurementTests {
    func opened(_ fixture: String = "report") async throws
      -> (PDFViewer, PDFDocument, MeasurementCapability, PageRef)
    {
      let viewer = try PDFViewer(
        ViewerConfig(interaction: true, annotation: AnnotationConfig(), measurement: MeasurementConfig()))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data(fixture))
      let page = try await viewer.pages(of: document)[0].page
      return (viewer, document, try await viewer.measurement(for: document), page)
    }

    @Test func ensureLoadedMakesThePageReady() async throws {
      let (_, _, measurement, page) = try await opened()
      try await measurement.ensureLoaded(page)
      let scale = try await measurement.pageScale(page)
      expectNoDifference(scale.ready, true)
      expectNoDifference(scale.source, .default)
      expectNoDifference(scale.persistent, true)
    }

    @Test func aPresetIsWrittenAndReadsBack() async throws {
      let (_, _, measurement, page) = try await opened()
      let preset = try #require(try await measurement.presets().first)
      let reports = try await measurement.setPreset(.page(page: page), presetId: preset.id)
      expectNoDifference(reports.count, 1)
      let scale = try await measurement.pageScale(page)
      expectNoDifference(scale.source, .owned)
    }

    @Test func aForeignScaleIsReadAndKept() async throws {
      let (_, _, measurement, page) = try await opened("measure-foreign-viewport")
      try await measurement.ensureLoaded(page)
      let foreign = try await measurement.pageScale(page)
      expectNoDifference(foreign.source, .foreign)
      expectNoDifference(foreign.measure?.ratio, "1 in = 1 m")
      let preset = try #require(try await measurement.presets().first)
      _ = try await measurement.setPreset(.page(page: page), presetId: preset.id)
      let owned = try await measurement.pageScale(page).source
      expectNoDifference(owned, .owned)
      _ = try await measurement.clearScale(.page(page: page))
      let cleared = try await measurement.pageScale(page).source
      expectNoDifference(cleared, .foreign)
    }

    @Test func measureDistanceUsesThePageScale() async throws {
      let (_, _, measurement, page) = try await opened("measure-foreign-viewport")
      try await measurement.ensureLoaded(page)
      let reading = try await measurement.measureDistance(
        on: page, from: Point(x: 0, y: 0), to: Point(x: 72, y: 0))
      guard case let .value(readout) = reading else {
        Issue.record("expected a readout, got \(reading)")
        return
      }
      expectNoDifference(readout.kind, .distance)
      #expect(readout.value > 0)
    }

    @Test func calibratingAZeroLengthThrows() async throws {
      let (_, _, measurement, page) = try await opened()
      await #expect {
        _ = try await measurement.calibrate(
          MeasurementCalibrateInput(page: page, from: Point(x: 5, y: 5), to: Point(x: 5, y: 5), value: 1, unit: "m"))
      } throws: { error in
        (error as? PDFError)?.code == "invalid-input"
      }
    }

    @Test func aCreatedMeasurementIsAnAnnotation() async throws {
      let (viewer, document, measurement, page) = try await opened()
      // A measurement needs the page's scale: `not-ready` until it loads.
      try await measurement.ensureLoaded(page)
      let ref = try await measurement.createMeasurement(
        CreateMeasurementInput(kind: .distance, page: page, points: [Point(x: 10, y: 10), Point(x: 82, y: 10)]))
      let annotations = try await viewer.annotations(for: document)
      let found = try #require(try await annotations.get(ref))
      expectNoDifference(found.subtype, "line")
      // `epdf-engine`'s line create writes neither `/IT /LineDimension` nor
      // `/Measure` yet, so the read-back line isn't a dimension and its
      // readout is `.notDimension`. When the engine writes them, this reads
      // a value.
      let reading = try await measurement.readout(ref)
      expectNoDifference(reading, .unavailable(reason: .notDimension))
    }

    @Test func aClosedDocumentsCapabilityThrows() async throws {
      let (viewer, document, measurement, page) = try await opened()
      try await viewer.close(document)
      await #expect {
        _ = try await measurement.pageScale(page)
      } throws: { error in
        let error = error as? PDFError
        return error?.code == "instance-closed" && error?.name == "PluginError"
      }
    }

    @Test func droppingTheStreamUnsubscribes() async throws {
      let (_, _, measurement, page) = try await opened()
      let task = Task { for await _ in measurement.events {} }
      try await Task.sleep(for: .milliseconds(20))
      task.cancel()
      _ = await task.value
      let preset = try #require(try await measurement.presets().first)
      _ = try await measurement.setPreset(.page(page: page), presetId: preset.id)
    }

    @Test func setScaleCancelsWhenTheTaskIsCancelled() async throws {
      let (_, _, measurement, page) = try await opened()
      let preset = try #require(try await measurement.presets().first)
      let task = Task { try await measurement.setPreset(.page(page: page), presetId: preset.id) }
      task.cancel()
      do {
        _ = try await task.value
      } catch {
        expectNoDifference((error as? PDFError)?.code, "operation-cancelled")
      }
      _ = try await measurement.pageScale(page)
    }

    @Test func ensureLoadedCancelsWhenTheTaskIsCancelled() async throws {
      let (_, _, measurement, page) = try await opened()
      let task = Task { try await measurement.ensureLoaded(page) }
      task.cancel()
      do {
        try await task.value
      } catch {
        expectNoDifference((error as? PDFError)?.code, "operation-cancelled")
      }
    }

    @Test func aScaleChangeIsObserved() async throws {
      let (_, _, measurement, page) = try await opened()
      let events = measurement.events
      let preset = try #require(try await measurement.presets().first)
      _ = try await measurement.setPreset(.page(page: page), presetId: preset.id)
      for await event in events {
        guard case let .scaleChanged(changed, _) = event else { continue }
        expectNoDifference(changed, page)
        break
      }
    }
  }
}
