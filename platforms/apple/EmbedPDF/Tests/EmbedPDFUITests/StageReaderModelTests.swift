import CustomDump
import EmbedPDF
import EmbedPDFUI
import Foundation
import Testing

@MainActor
@Suite(.serialized, .timeLimit(.minutes(1)))
struct StageReaderModelTests {
  func model() async throws -> (StageReaderModel, Task<Void, Never>) {
    let viewer = try PDFViewer(ViewerConfig(render: RenderConfig(), stage: StageConfig(frames: false)))
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
}
