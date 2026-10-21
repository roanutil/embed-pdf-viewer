import CustomDump
import EmbedPDF
import EmbedPDFUI
import Foundation
import Testing

@MainActor
@Suite(.serialized, .timeLimit(.minutes(1)))
struct TiledPageModelTests {
  func model() async throws -> (TiledPageModel, PageInfo, RenderViewDemand) {
    let viewer = try PDFViewer(ViewerConfig(render: RenderConfig()))
    try await viewer.start()
    let url = try #require(Bundle.module.url(forResource: "report", withExtension: "pdf", subdirectory: "Fixtures"))
    let document = try await viewer.open(try Data(contentsOf: url))
    let page = try await viewer.pages(of: document)[0]
    let render = try await viewer.render(in: document)
    let demand = try await render.viewDemand("pages")
    return (TiledPageModel(render: render, demand: demand, page: page.page), page, demand)
  }

  @Test func aDemandUnderTheBudgetPaintsOnlyTheBase() async throws {
    let (model, page, demand) = try await model()
    await model.update(deviceScale: 256 / page.width, unrotatedWidth: page.width, visibleRect: nil)
    #expect(model.base != nil)
    expectNoDifference(model.tiles, [])
    try await demand.dispose()
  }

  @Test func aDeepDemandPaintsTilesOverTheBudgetedBase() async throws {
    let (model, page, demand) = try await model()
    let visible = Rect(x: 0, y: 0, width: page.width / 2, height: page.height / 4)
    await model.update(deviceScale: 8, unrotatedWidth: page.width, visibleRect: visible)
    let base = try #require(model.base)
    #expect(base.width <= 640)
    #expect(!model.tiles.isEmpty)
    for tile in model.tiles {
      #expect(tile.rect.x < visible.width + 1)
      #expect(tile.rect.y < visible.height + 1)
    }
    try await demand.dispose()
  }

  @Test func releaseDropsTheTiles() async throws {
    let (model, page, demand) = try await model()
    await model.update(
      deviceScale: 8, unrotatedWidth: page.width,
      visibleRect: Rect(x: 0, y: 0, width: page.width / 2, height: page.height / 4))
    #expect(!model.tiles.isEmpty)
    await model.release()
    expectNoDifference(model.tiles, [])
    try await demand.dispose()
  }
}
