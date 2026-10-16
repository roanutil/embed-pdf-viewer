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

    func opened(frames: Bool = false) async throws -> (PDFViewer, DocumentInfo, StageCapability) {
      let viewer = try PDFViewer(ViewerConfig(stage: StageConfig(frames: frames)))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      let stage = try await viewer.stage(for: document)
      try await stage.setViewportSize(Size(width: 390, height: 844))
      return (viewer, document, stage)
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
      let (_, _, stage) = try await opened(frames: true)
      let events = stage.events
      try await stage.fling(velocityX: 0, velocityY: -3000)
      #expect(try await stage.isMoving())
      for await event in events {
        if case .motionEnded = event { break }
      }
      #expect(try await stage.isMoving() == false)
    }

    @Test func aClosedDocumentsCapabilityThrows() async throws {
      let (viewer, document, stage) = try await opened()
      try await viewer.close(document)
      await #expect {
        _ = try await stage.zoomLevel()
      } throws: { error in
        (error as? PDFError)?.code == "instance-closed"
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
