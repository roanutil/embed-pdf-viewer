import CoreGraphics
import CustomDump
import EmbedPDF
import Testing

extension BaseSuite {
  @Suite struct RenderSnapshotTests {
    @Test func metadataChangesAndOwnedPixelsSurviveClearAndClose() async throws {
      let viewer = try PDFViewer(ViewerConfig(render: RenderConfig(), interaction: true))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("colors"))
      let page = try await viewer.pages(of: document)[0].page
      let render = try await viewer.render(in: document)
      let interaction = try await viewer.interaction(for: document)
      #expect(try await render.canSnapshot())
      #expect(try await render.snapshot() == nil)
      var changes = render.snapshotChanges.makeAsyncIterator()
      try await interaction.activateTool("snapshot")
      let from = Point(x: 20, y: 0)
      let to = Point(x: 60, y: 20)
      interaction.dispatchPointer(PointerSample(phase: .down, viewport: from,
        page: PointerPageHit(page: page, point: from, scale: 1, rotation: 90), modifiers: Modifiers()))
      interaction.dispatchPointer(PointerSample(phase: .up, viewport: to,
        page: PointerPageHit(page: page, point: to, scale: 1, rotation: 90), modifiers: Modifiers()))
      let snapshot = try #require(try await render.snapshot())
      expectNoDifference(snapshot.page, page)
      expectNoDifference(snapshot.rect, Rect(x: 20, y: 0, width: 40, height: 20))
      let snapshotBox = try await render.snapshotBox(on: page)
      expectNoDifference(snapshotBox, snapshot.rect)
      let changed = try #require(await changes.next())
      let captured = try #require(changed)
      expectNoDifference(captured.rect, snapshot.rect)
      await expectCancelledBeforeItStarts { try await snapshot.image() }
      let pixels = try await captured.image()
      expectNoDifference(pixels.width, 40)
      expectNoDifference(pixels.height, 80)
      expectNoDifference(pixels.scale, 2)
      let before = try #require(pixels.image.dataProvider?.data)
      try await render.clearSnapshot()
      #expect(try await render.snapshot() == nil)
      let cleared = await changes.next()
      #expect(cleared != nil)
      #expect(cleared! == nil)
      try await viewer.close(document)
      #expect(await changes.next() == nil)
      await #expect { _ = try await snapshot.image() } throws: { ($0 as? PDFError)?.code == .instanceClosed }
      try await viewer.destroy()
      #expect(pixels.image.dataProvider?.data == before)
    }

    @Test func croppedAndRotatedCGImageKeepsScaleAndColors() async throws {
      let (viewer, _, page, render) = try await RenderTests().opened()
      let image = try await render.renderPage(page, options: RenderPageOptions(
        rect: Rect(x: 20, y: 0, width: 40, height: 20), rotation: .clockwise90, scale: 2))
      expectNoDifference(image.width, 40)
      expectNoDifference(image.height, 80)
      expectNoDifference(image.scale, 2)
      try await viewer.destroy()
      let pixels = try RenderTests().drawn(image.image)
      expectNoDifference(pixels.at(x: 20, y: 20), [0, 255, 0, 255])
      expectNoDifference(pixels.at(x: 20, y: 60), [0, 0, 255, 255])
    }
  }
}
