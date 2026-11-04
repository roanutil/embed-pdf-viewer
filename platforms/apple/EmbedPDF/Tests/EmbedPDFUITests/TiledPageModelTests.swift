import EmbedPDF
@testable import EmbedPDFUI
import Foundation
import Testing

@MainActor
@Suite(.serialized, .timeLimit(.minutes(1)))
struct TiledPageModelTests {
  private func withModel(_ body: (TiledPageModel, PageInfo) async throws -> Void) async throws {
    let viewer = try PDFViewer(ViewerConfig(render: RenderConfig()))
    try await viewer.start()
    do {
      let url = try #require(Bundle.module.url(forResource: "report", withExtension: "pdf", subdirectory: "Fixtures"))
      let document = try await viewer.open(Data(contentsOf: url))
      let page = try await viewer.pages(of: document)[0]
      let render = try await viewer.render(in: document)
      let demand = try await render.viewDemand("c8-pages")
      let model = TiledPageModel(render: render, demand: demand, page: page.page)
      do {
        try await body(model, page)
      } catch {
        await model.release()
        try? await demand.dispose()
        throw error
      }
      await model.release()
      try await demand.dispose()
    } catch {
      try? await viewer.destroy()
      throw error
    }
    try await viewer.destroy()
  }

  @Test func smallDemandOnlyNeedsTheBase() async throws {
    try await withModel { model, page in
      await model.update(deviceScale: 256 / page.width, unrotatedWidth: page.width, visibleRect: nil)
      #expect(model.base != nil)
      #expect(model.tiles.isEmpty)
    }
  }

  @Test func visibleDeepDemandAddsDetailOverTheBudgetedBase() async throws {
    try await withModel { model, page in
      let visible = Rect(x: 0, y: 0, width: page.width / 2, height: page.height / 4)
      await model.update(deviceScale: 8, unrotatedWidth: page.width, visibleRect: visible)
      let base = try #require(model.base)
      #expect(base.width <= 640)
      #expect(!model.tiles.isEmpty)
      for tile in model.tiles {
        #expect(tile.rect.x < visible.width + 1)
        #expect(tile.rect.y < visible.height + 1)
      }
    }
  }

  @Test func offscreenDemandRetiresTilesAndReturningRestoresThem() async throws {
    try await withModel { model, page in
      let visible = Rect(x: 0, y: 0, width: page.width / 2, height: page.height / 4)
      await model.update(deviceScale: 8, unrotatedWidth: page.width, visibleRect: visible)
      #expect(!model.tiles.isEmpty)
      await model.update(deviceScale: 256 / page.width, unrotatedWidth: page.width,
                         visibleRect: Rect(x: 0, y: 0, width: 0, height: 0))
      #expect(model.tiles.isEmpty)
      let base = try #require(model.base)
      #expect(base.width <= 256)
      await model.update(deviceScale: 8, unrotatedWidth: page.width, visibleRect: visible)
      #expect(!model.tiles.isEmpty)
    }
  }

  @Test func releaseDropsTilesAndThePageCanAppearAgain() async throws {
    try await withModel { model, page in
      let visible = Rect(x: 0, y: 0, width: page.width / 2, height: page.height / 4)
      await model.update(deviceScale: 8, unrotatedWidth: page.width, visibleRect: visible)
      let base = try #require(model.base)
      #expect(!model.tiles.isEmpty)
      await model.release()
      #expect(model.tiles.isEmpty)
      #expect(model.base === base)
      await model.update(deviceScale: 8, unrotatedWidth: page.width, visibleRect: visible)
      #expect(!model.tiles.isEmpty)
    }
  }

  @Test func paintAcknowledgementsDoNotDropTheCurrentTiles() async throws {
    try await withModel { model, page in
      let visible = Rect(x: 0, y: 0, width: page.width / 2, height: page.height / 4)
      let update = Task { await model.update(deviceScale: 8, unrotatedWidth: page.width, visibleRect: visible) }
      let clock = ContinuousClock()
      let deadline = clock.now + .seconds(5)
      while model.tiles.isEmpty && clock.now < deadline { try await Task.sleep(for: .milliseconds(5)) }
      for tile in model.tiles { await model.tileDisplayed(tile.id) }
      await update.value
      let keys = model.tiles.map(\.id)
      #expect(!keys.isEmpty)
      #expect(Set(keys).count == keys.count)
      await model.update(deviceScale: 8, unrotatedWidth: page.width, visibleRect: visible)
      #expect(model.tiles.map(\.id) == keys)
    }
  }
}
