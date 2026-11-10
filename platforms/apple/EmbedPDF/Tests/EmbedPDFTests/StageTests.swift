import CustomDump
import EmbedPDF
import Testing

final class SavedView: StageInitialViewProvider, @unchecked Sendable {
  let view: StageViewState
  init(_ view: StageViewState) { self.view = view }
  func initialView() throws -> StageViewState? { view }
}

extension BaseSuite {
  @Suite struct StageTests {
    let instant = StageGoToOptions(behavior: .instant)

    // The native host's clock always has frames, so motion tweens; the
    // tests that need a jump ask for `.instant`.
    func opened() async throws -> (PDFViewer, DocumentInfo, StageCapability) {
      let viewer = try PDFViewer(ViewerConfig(stage: StageConfig()))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      let stage = try await viewer.stage(for: document)
      try await stage.setViewportSize(Size(width: 390, height: 844))
      return (viewer, document, stage)
    }

    @Test func nativeZoomBoxTracksViewportPointsAndCancels() async throws {
      let (_, _, stage) = try await opened()
      let before = try await stage.camera()
      try await stage.beginZoomDrag(at: Point(x: 70, y: 90))
      #expect(try await stage.zoomBox() == nil)
      try await stage.moveZoomDrag(to: Point(x: 170, y: 190))
      let box = try #require(try await stage.zoomBox())
      #expect(abs(box.x - 70) < 0.000001 && abs(box.y - 90) < 0.000001)
      #expect(abs(box.width - 100) < 0.000001 && abs(box.height - 100) < 0.000001)
      try await stage.cancelZoomDrag()
      #expect(try await stage.zoomBox() == nil)
      let afterCancel = try await stage.camera()
      expectNoDifference(afterCancel, before)
      #expect(try await !stage.currentPages().isEmpty)
    }

    @Test func aSizedStageShowsTheFirstPage() async throws {
      let (_, _, stage) = try await opened()
      let visible = try await stage.visiblePages()
      expectNoDifference(visible.first?.pageIndex, 0)
      #expect(visible.first.map { $0.transform.renderScale > 0 } == true)
    }

    @Test func navigationIsObserved() async throws {
      let (_, _, stage) = try await opened()
      let events = stage.events
      try await stage.goToPageIndex(5, options: instant)
      for await event in events {
        guard case let .pageChanged(event) = event else { continue }
        expectNoDifference(event.pageIndex, 5)
        break
      }
      let current = try await stage.currentPageIndex()
      expectNoDifference(current, 5)
    }

    @Test func fitWidthThenZoomIn() async throws {
      let (_, _, stage) = try await opened()
      try await stage.fitWidth()
      let mode = try await stage.zoomMode()
      expectNoDifference(mode, .mode(mode: .fitWidth))
      let fitted = try await stage.zoomLevel()
      try await stage.zoomIn()
      #expect(try await stage.zoomLevel() > fitted)
    }

    @Test func aPanGestureMovesTheCamera() async throws {
      let (_, _, stage) = try await opened()
      let before = try await stage.camera()
      try await stage.beginGesture()
      for _ in 0..<20 { stage.panBy(dx: 0, dy: -10) }
      try await stage.endGesture()
      let after = try await stage.camera()
      #expect(after.y != before.y)
    }

    @Test func aSavedViewRestoresOnTheNextOpen() async throws {
      let (viewer, _, stage) = try await opened()
      try await stage.goToPageIndex(7, options: instant)
      let saved = try await stage.viewState()
      let second = try await viewer.open(Fixture.data("report"))
      let next = try await viewer.stage(for: second)
      _ = try await next.provideInitialView(SavedView(saved), priority: 10)
      try await next.setViewportSize(Size(width: 390, height: 844))
      let current = try await next.currentPageIndex()
      expectNoDifference(current, 7)
    }

    @Test func aFlingMovesThenSettles() async throws {
      let (_, _, stage) = try await opened()
      let events = stage.events
      try await stage.fling(velocityX: 0, velocityY: -3000)
      #expect(try await stage.isMoving())
      for await event in events {
        if case .motionEnded = event { break }
      }
      #expect(try await stage.isMoving() == false)
    }

    /// A click with the zoom tool: a box that never reaches its threshold.
    func clickZoom(_ end: (StageCapability) async throws -> Void) async throws -> (before: Double, after: Double) {
      let (_, _, stage) = try await opened()
      let before = try await stage.zoomLevel()
      try await stage.beginZoomDrag(at: Point(x: 195, y: 400))
      try await end(stage)
      return (before, try await stage.zoomLevel())
    }

    @Test func endZoomDragLeftOutZoomsIn() async throws {
      let omitted = try await clickZoom { try await $0.endZoomDrag() }
      let zoomIn = try await clickZoom { try await $0.endZoomDrag(zoomOut: false) }
      let zoomOut = try await clickZoom { try await $0.endZoomDrag(zoomOut: true) }
      #expect(omitted.after > omitted.before)
      expectNoDifference(omitted.after, zoomIn.after)
      #expect(zoomOut.after < zoomOut.before)
    }

    /// The camera inside a gesture after dragging past the top of the
    /// document, where only an elastic gesture leaves the clamp.
    func overscrolled(_ begin: (StageCapability) async throws -> Void) async throws -> Camera {
      let (_, _, stage) = try await opened()
      try await begin(stage)
      for _ in 0..<20 { stage.panBy(dx: 0, dy: 30) }
      let camera = try await stage.camera()
      try await stage.endGesture()
      return camera
    }

    @Test func beginGestureLeftOutIsNotElastic() async throws {
      let omitted = try await overscrolled { try await $0.beginGesture() }
      let clamped = try await overscrolled { try await $0.beginGesture(elastic: false) }
      let elastic = try await overscrolled { try await $0.beginGesture(elastic: true) }
      expectNoDifference(omitted, clamped)
      #expect(elastic != clamped)
    }

    /// The cursor over the gap beside the first page, with the pointer tool.
    func gapCursor(_ register: (StageCapability) async throws -> Registration) async throws -> String {
      let viewer = try PDFViewer(ViewerConfig(interaction: true, stage: StageConfig()))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      let stage = try await viewer.stage(for: document)
      try await stage.setViewportSize(Size(width: 390, height: 844))
      let interaction = try await viewer.interaction(for: document)
      let registration = try await register(stage)
      let lens = try await stage.lensId()
      // A move nobody captured hovers; `page: nil` is a gap.
      interaction.dispatchPointer(
        PointerSample(phase: .move, viewport: Point(x: 2, y: 2), modifiers: Modifiers(), pointerType: .mouse, source: lens))
      let cursor = try await interaction.cursor()
      registration.unregister()
      return cursor
    }

    @Test func registerScrollHandlerLeftOutPansOverGaps() async throws {
      let omitted = try await gapCursor { try await $0.registerScrollHandler() }
      let fallback = try await gapCursor { try await $0.registerScrollHandler(panFallback: true) }
      let without = try await gapCursor { try await $0.registerScrollHandler(panFallback: false) }
      expectNoDifference(omitted, "grab")
      expectNoDifference(fallback, "grab")
      #expect(without != "grab")
    }

    @available(*, deprecated)
    @Test func theBoolOverloadsPassNilThrough() async throws {
      let unset: Bool? = nil
      let zoom = try await clickZoom { try await $0.endZoomDrag(zoomOut: unset) }
      #expect(zoom.after > zoom.before)
      let gesture = try await overscrolled { try await $0.beginGesture(elastic: unset) }
      let omitted = try await overscrolled { try await $0.beginGesture() }
      expectNoDifference(gesture, omitted)
      let cursor = try await gapCursor { try await $0.registerScrollHandler(panFallback: unset) }
      expectNoDifference(cursor, "grab")
    }

    @Test func aClosedDocumentsCapabilityThrows() async throws {
      let (viewer, document, stage) = try await opened()
      try await viewer.close(document)
      await #expect {
        _ = try await stage.zoomLevel()
      } throws: { error in
        (error as? PDFError)?.code == .instanceClosed
      }
    }

    @Test func droppingTheStreamUnsubscribes() async throws {
      let (_, _, stage) = try await opened()
      let task = Task { for await _ in stage.events {} }
      try await Task.sleep(for: .milliseconds(20))
      task.cancel()
      _ = await task.value
      try await stage.zoomIn()
    }
  }
}
