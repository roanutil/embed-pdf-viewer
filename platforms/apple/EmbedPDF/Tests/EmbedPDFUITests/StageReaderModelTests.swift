import CustomDump
import EmbedPDF
import EmbedPDFUI
import Foundation
import Testing

@MainActor
@Suite(.serialized, .timeLimit(.minutes(1)))
struct StageReaderModelTests {
  func model() async throws -> (StageReaderModel, Task<Void, Never>) {
    let viewer = try PDFViewer(ViewerConfig(render: RenderConfig(), stage: StageConfig()))
    try await viewer.start()
    let url = try #require(Bundle.module.url(forResource: "report", withExtension: "pdf", subdirectory: "Fixtures"))
    let document = try await viewer.open(try Data(contentsOf: url))
    let model = StageReaderModel(
      stage: try await viewer.stage(for: document), render: try await viewer.render(in: document))
    let task = Task { await model.task() }
    return (model, task)
  }

  @Test func sizingShowsPages() async throws {
    let (model, task) = try await model()
    defer { task.cancel() }
    await model.sizeChanged(CGSize(width: 390, height: 844), displayScale: 3)
    #expect(!model.visiblePages.isEmpty)
    expectNoDifference(model.currentPageIndex, 0)
  }

  @Test func draggingScrollsAndUpdatesTheVisiblePages() async throws {
    let (model, task) = try await model()
    defer { task.cancel() }
    await model.sizeChanged(CGSize(width: 390, height: 844), displayScale: 3)
    let before = model.camera
    await model.dragChanged(translation: CGSize(width: 0, height: -400))
    await model.dragEnded(velocity: .zero)
    #expect(model.camera != before)
  }

  @Test func resizingKeepsTheCurrentPage() async throws {
    let (model, task) = try await model()
    defer { task.cancel() }
    await model.sizeChanged(CGSize(width: 390, height: 844), displayScale: 3)
    for _ in 0..<12 {
      await model.dragChanged(translation: CGSize(width: 0, height: -800))
      await model.dragEnded(velocity: .zero)
    }
    let page = model.currentPageIndex
    #expect(page > 0)
    await model.sizeChanged(CGSize(width: 844, height: 390), displayScale: 3)
    expectNoDifference(model.currentPageIndex, page)
    #expect(!model.visiblePages.isEmpty)
  }

  /// SwiftUI delivers a tap's change and end back to back, each in its own
  /// task. The gesture bracket must still open before it closes, or the
  /// stage keeps an elastic overscroll it never springs back from.
  @Test func backToBackGestureCallsKeepTheBracketOrdered() async throws {
    let (model, task) = try await model()
    defer { task.cancel() }
    await model.sizeChanged(CGSize(width: 390, height: 844), displayScale: 3)
    let resting = try await model.stage.camera()
    for _ in 0..<20 {
      let change = Task { await model.dragChanged(translation: CGSize(width: 0, height: 300)) }
      let end = Task { await model.dragEnded(velocity: .zero) }
      await change.value
      await end.value
    }
    let camera = try await model.stage.camera()
    expectNoDifference(camera, resting)
  }

  /// Until the reader draws tiles, a deep zoom would render each page whole
  /// at the zoomed scale: 7200 by 9600 pixels for a 600-point page at zoom 4
  /// on a 3x screen. The scale it renders at is capped by pixel count.
  @Test func aDeepZoomRendersAtACappedScale() {
    let capped = StageReaderModel.renderScale(12, contentWidth: 600, contentHeight: 800)
    #expect(capped < 12)
    #expect(600 * capped * 800 * capped <= StageReaderModel.maxPagePixels * 1.000001)
    expectNoDifference(StageReaderModel.renderScale(2, contentWidth: 600, contentHeight: 800), 2)
  }
}
