import CustomDump
import EmbedPDF
import Testing

extension BaseSuite {
  @Suite struct RenderViewportTests {
    func opened() async throws -> (PDFViewer, PDFDocument, RenderCapability, PageRef) {
      let viewer = try PDFViewer(ViewerConfig(render: RenderConfig()))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("colors"))
      let page = try await viewer.pages(of: document)[0].page
      return (viewer, document, try await viewer.render(in: document), page)
    }

    @Test func renderSourceMatchesItsSourceKey() async throws {
      let (_, _, render, page) = try await opened()
      let a = try await render.sourceKey(page, scale: 2)
      let b = try await render.sourceKey(page, scale: 2, includeAnnotations: true)
      expectNoDifference(a, b)
      let rendered = try await render.renderSource(page, scale: 2)
      #expect(rendered.width > 0)
    }

    @Test func renderSourceCancelsWhenTheTaskIsCancelled() async throws {
      let (_, _, render, page) = try await opened()
      let task = Task { try await render.renderSource(page, scale: 8) }
      task.cancel()
      do {
        _ = try await task.value
      } catch {
        expectNoDifference((error as? PDFError)?.code, "operation-cancelled")
      }
      _ = try await render.renderSource(page, scale: 1)
    }

    @Test func aViewDemandPlansAndDisposes() async throws {
      let (_, _, render, page) = try await opened()
      _ = try await render.paintSettings()
      let view = try await render.viewDemand("main")
      // The page is 80 points wide: the full-page raster meets an 80-pixel
      // demand, so the tile plane stays disengaged whether or not tiles are on.
      try await view.setDemand(page, RenderPageViewDemand(desiredDeviceWidth: 80))
      let plan = try await view.plan(page)
      expectNoDifference(plan.engaged, false)
      expectNoDifference(plan.paint, [])
      let tile = try await view.tileImage(page, key: "no-such-key")
      #expect(tile == nil)
      try await view.release(page)
      try await view.dispose()
    }

    @Test func aClosedDocumentsViewDemandThrows() async throws {
      let (viewer, document, render, page) = try await opened()
      let view = try await render.viewDemand("main")
      try await viewer.close(document)
      await #expect {
        _ = try await view.plan(page)
      } throws: { error in
        (error as? PDFError)?.code == "instance-closed"
      }
    }
  }
}
