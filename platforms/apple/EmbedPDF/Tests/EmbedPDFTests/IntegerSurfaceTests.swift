import CustomDump
import EmbedPDF
import Foundation
import Testing

/// Expects `body` to throw `invalid-input` without reaching the engine.
private func expectInvalidInput(
  _ body: () async throws -> Void, sourceLocation: SourceLocation = #_sourceLocation
) async {
  await #expect(sourceLocation: sourceLocation) {
    try await body()
  } throws: { error in
    (error as? PDFError)?.code == .invalidInput
  }
}

/// Indexes the engine carries as `UInt32` that no `Int` argument may pass.
private let outsideUInt32 = [-1, Int(UInt32.max) + 1, Int.max]

extension BaseSuite {
  @Suite struct IntegerSurfaceTests {
    @Test func aValueOutsideTheWireTypeThrowsInvalidInput() throws {
      for value in outsideUInt32 {
        #expect {
          _ = try value.toWire("index", as: UInt32.self)
        } throws: { error in
          let error = error as? PDFError
          return error?.code == .invalidInput && error?.detail == "index \(value) is outside 0...4294967295"
        }
      }
      expectNoDifference(try 0.toWire("index", as: UInt32.self), 0)
      expectNoDifference(try Int(UInt32.max).toWire("index", as: UInt32.self), UInt32.max)
      expectNoDifference(try Int.max.toWire("index", as: Int64.self), Int64.max)
      expectNoDifference(try (-1).toWire("index", as: Int64.self), -1)
    }

    @Test func aResultPastIntThrowsInvalidState() throws {
      #expect {
        _ = try UInt64.max.toInt("count")
      } throws: { error in
        (error as? PDFError)?.code == .invalidState
      }
      expectNoDifference(try UInt32.max.toInt("count"), 4_294_967_295)
      expectNoDifference(try Int64(-1).toInt("index"), -1)
    }

    @Test func viewerPagesAndTabsTakeAndGiveInt() async throws {
      let viewer = try PDFViewer(ViewerConfig())
      try await viewer.start()
      let first = try await viewer.open(Fixture.data("report"))
      let second = try await viewer.open(Fixture.data("colors"), activate: false)
      let count: Int = try await viewer.documentCount()
      expectNoDifference(count, 2)
      // The engine's page index is an `Int64`, so every `Int` reaches it:
      // one below zero or past the last page has no page.
      #expect(try await viewer.page(at: -1, in: first) == nil)
      #expect(try await viewer.page(at: Int.max, in: first) == nil)
      let page = try #require(try await viewer.page(at: 0, in: first))
      let index: Int = try await viewer.index(of: page.page, in: first)
      expectNoDifference(index, 0)
      let missing: Int = try await viewer.index(of: page.page, in: second)
      expectNoDifference(missing, -1)
      // A tab position is clamped into the order at both ends.
      try await viewer.move(first, to: Int.max)
      #expect(try await viewer.documentOrder() == [second.id, first.id])
      try await viewer.move(first, to: -1)
      #expect(try await viewer.documentOrder() == [first.id, second.id])
      try await viewer.move(first, to: 0)
      #expect(try await viewer.documentOrder() == [first.id, second.id])
      try await viewer.destroy()
    }

    @Test func searchHitIndexesAreInt() async throws {
      let viewer = try PDFViewer(ViewerConfig(search: SearchConfig()))
      try await viewer.start()
      let search = try await viewer.search(in: try await viewer.open(Fixture.data("report")))
      _ = try await search.search(SearchQuery(text: "the"))
      let count: Int = try await search.hitCount()
      #expect(try await search.hits().count == count)
      for value in outsideUInt32 {
        await expectInvalidInput { _ = try await search.goToHit(value) }
      }
      let active: Int? = try await search.activeHitIndex()
      expectNoDifference(active, 0)
      _ = try await search.goToHit(1)
      #expect(try await search.activeHitIndex() == 1)
      _ = try await search.goToHit(0)
      #expect(try await search.activeHitIndex() == 0)
      // The largest index the engine takes still wraps modulo the hit count.
      _ = try await search.goToHit(Int(UInt32.max))
      #expect(try await search.activeHitIndex() == Int(UInt32.max) % count)
      try await search.clear()
      #expect(try await search.activeHitIndex() == nil)
      try await viewer.destroy()
    }

    @Test func stagePageIndexesAreInt() async throws {
      let viewer = try PDFViewer(ViewerConfig(stage: StageConfig()))
      try await viewer.start()
      let stage = try await viewer.stage(for: try await viewer.open(Fixture.data("report")))
      try await stage.setViewportSize(Size(width: 390, height: 844))
      let instant = StageGoToOptions(behavior: .instant)
      let count: Int = try await stage.pageCount()
      expectNoDifference(count, 14)
      try await stage.goToPageIndex(3, options: instant)
      for value in outsideUInt32 {
        await expectInvalidInput { try await stage.goToPageIndex(value, options: instant) }
        await expectInvalidInput { try await stage.revealIndex(value) }
      }
      let current: Int = try await stage.currentPageIndex()
      expectNoDifference(current, 3)
      try await stage.goToPageIndex(0, options: instant)
      #expect(try await stage.currentPageIndex() == 0)
      try await stage.revealIndex(0)
      try await viewer.destroy()
    }

    @Test func paneAndTabIndexesAreInt() async throws {
      let viewer = try PDFViewer(ViewerConfig(viewManager: true))
      try await viewer.start()
      let manager = try await viewer.viewManager()
      let report = try await viewer.open(Fixture.data("report"))
      let colors = try await viewer.open(Fixture.data("colors"))
      let second = try await manager.createPane()
      for value in outsideUInt32 {
        await expectInvalidInput { try await manager.movePane(second, to: value) }
        await expectInvalidInput { try await manager.addDocument(colors, to: second, at: value) }
        await expectInvalidInput { try await manager.moveDocument(report, within: "pane-1", to: value) }
        await expectInvalidInput { try await manager.moveDocument(report, from: "pane-1", to: second, at: value) }
      }
      #expect(try await manager.paneOrder() == ["pane-1", second])
      #expect(try await manager.pane("pane-1")?.documentIds == [report.id, colors.id])
      try await manager.movePane(second, to: 0)
      #expect(try await manager.paneOrder() == [second, "pane-1"])
      try await manager.moveDocument(colors, within: "pane-1", to: 0)
      #expect(try await manager.pane("pane-1")?.documentIds == [colors.id, report.id])
      try await manager.addDocument(colors, to: second, at: 0)
      #expect(try await manager.pane(second)?.documentIds == [colors.id])
      try await manager.moveDocument(report, from: "pane-1", to: second, at: 0)
      #expect(try await manager.pane(second)?.documentIds == [report.id, colors.id])
      try await viewer.destroy()
    }

    @Test func aSignatureIndexOutsideUInt32ThrowsInvalidInput() async throws {
      let viewer = try PDFViewer(
        ViewerConfig(interaction: true, annotation: "", form: FormConfig(), signature: SignatureConfig()))
      try await viewer.start()
      let signatures = try await viewer.signatures(for: try await viewer.open(Fixture.data("signed_two_approvals")))
      _ = try await signatures.refresh()
      for value in outsideUInt32 {
        await expectInvalidInput { _ = try await signatures.analyzeChanges(since: value, until: .persisted) }
      }
      let analysis = try await signatures.analyzeChanges(since: 0, until: .persisted)
      expectNoDifference(analysis.laterRevisions, 1)
      try await viewer.destroy()
    }

    @Test func aBatchConcurrencyOutsideUInt32ThrowsInvalidInput() async throws {
      let viewer = try PDFViewer(ViewerConfig(render: RenderConfig()))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("colors"))
      let render = try await viewer.render(in: document)
      let pages = try await viewer.pages(of: document).map(\.page)
      let options = RenderPageOptions(width: 30)
      for value in outsideUInt32 {
        await expectInvalidInput { _ = try await render.renderPages(pages, options: options, concurrency: value) }
      }
      // The engine runs at least one render at a time.
      let batch = try await render.renderPages(pages, options: options, concurrency: 0)
      expectNoDifference(batch.applied.map(\.page), pages)
      try await viewer.destroy()
    }

    @Test func aPendingRedactionCountIsInt() async throws {
      let viewer = try PDFViewer(ViewerConfig(interaction: true, annotation: "", redaction: RedactionConfig()))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      try await viewer.annotations(for: document).refresh()
      let redaction = try await viewer.redaction(for: document)
      let page = try await viewer.pages(of: document)[0].page
      _ = try await redaction.markArea(Rect(x: 72, y: 72, width: 200, height: 20), on: page)
      let count: Int = try await redaction.pendingCount(on: page)
      expectNoDifference(count, 1)
      try await viewer.destroy()
    }

    /// The fixed-width overloads keep old call sites compiling and pass
    /// their value straight to the engine.
    @available(*, deprecated)
    @Test func theFixedWidthOverloadsStillAnswer() async throws {
      let viewer = try PDFViewer(
        ViewerConfig(search: SearchConfig(), render: RenderConfig(), stage: StageConfig(), viewManager: true))
      try await viewer.start()
      let first = try await viewer.open(Fixture.data("report"))
      let second = try await viewer.open(Fixture.data("colors"), activate: false)
      let documents: UInt64 = try await viewer.documentCount()
      expectNoDifference(documents, 2)
      let page = try #require(try await viewer.page(at: 0 as Int64, in: first))
      let index: Int64 = try await viewer.index(of: page.page, in: first)
      expectNoDifference(index, 0)
      try await viewer.move(first, to: 1 as Int64)
      #expect(try await viewer.documentOrder() == [second.id, first.id])

      let search = try await viewer.search(in: first)
      _ = try await search.search(SearchQuery(text: "the"))
      _ = try await search.goToHit(1 as UInt32)
      let active: UInt32? = try await search.activeHitIndex()
      expectNoDifference(active, 1)
      let hits: UInt32 = try await search.hitCount()
      #expect(try await search.hitCount() == Int(hits))

      let stage = try await viewer.stage(for: first)
      try await stage.setViewportSize(Size(width: 390, height: 844))
      try await stage.goToPageIndex(2 as UInt32, options: StageGoToOptions(behavior: .instant))
      let current: UInt32 = try await stage.currentPageIndex()
      expectNoDifference(current, 2)
      try await stage.revealIndex(0 as UInt32)
      let pages: UInt32 = try await stage.pageCount()
      expectNoDifference(pages, 14)

      let manager = try await viewer.viewManager()
      let pane = try await manager.createPane()
      try await manager.movePane(pane, to: 0 as UInt32)
      #expect(try await manager.paneOrder() == [pane, "pane-1"])
      try await manager.addDocument(second, to: pane, at: 0 as UInt32?)
      try await manager.moveDocument(first, from: "pane-1", to: pane, at: 0 as UInt32?)
      try await manager.moveDocument(second, within: pane, to: 0 as UInt32)
      #expect(try await manager.pane(pane)?.documentIds == [second.id, first.id])

      let render = try await viewer.render(in: second)
      let colorPages = try await viewer.pages(of: second).map(\.page)
      let batch = try await render.renderPages(colorPages, options: RenderPageOptions(width: 30), concurrency: 2 as UInt32?)
      expectNoDifference(batch.applied.map(\.page), colorPages)
      try await viewer.destroy()
    }

    @available(*, deprecated)
    @Test func theFixedWidthOverloadsOfRedactionAndSignatureStillAnswer() async throws {
      let viewer = try PDFViewer(
        ViewerConfig(
          interaction: true, annotation: "", form: FormConfig(), redaction: RedactionConfig(),
          signature: SignatureConfig()))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("signed_two_approvals"))
      let signatures = try await viewer.signatures(for: document)
      _ = try await signatures.refresh()
      let analysis = try await signatures.analyzeChanges(since: 0 as UInt32, until: .persisted)
      expectNoDifference(analysis.laterRevisions, 1)
      let redaction = try await viewer.redaction(for: document)
      let pending: UInt32 = try await redaction.pendingCount()
      expectNoDifference(pending, 0)
      try await viewer.destroy()
    }
  }
}
