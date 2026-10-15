import CustomDump
import EmbedPDF
import Foundation
import Testing

extension BaseSuite {
  @Suite struct PageEditTests {
    func opened() async throws -> (PDFViewer, PDFDocument, PageEditCapability) {
      let viewer = try PDFViewer(ViewerConfig(pageEdit: true))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      return (viewer, document, try await viewer.pageEdit(for: document))
    }

    @Test func rotatingTwiceMakesAHalfTurn() async throws {
      let (viewer, document, edit) = try await opened()
      let first = try await viewer.pages(of: document)[0].page
      _ = try await edit.rotate([first], by: .clockwise)
      let layout = try await edit.rotate([first], by: .clockwise)
      expectNoDifference(layout.pages[0].rotation, 180)
      let pages = try await viewer.pages(of: document)
      expectNoDifference(pages[0].rotation, 180)
    }

    @Test func movingAPageToTheEndReordersTheList() async throws {
      let (viewer, document, edit) = try await opened()
      let first = try await viewer.pages(of: document)[0].page
      // An index in the post-removal space: 13 pages remain once the first
      // is detached.
      let layout = try await edit.movePages([first], to: .index(index: 13))
      expectNoDifference(layout.pages.last?.page, first)
      expectNoDifference(layout.pages.map(\.index), Array(0..<14))
    }

    @Test func endMovesTheBlockLast() async throws {
      let (viewer, document, edit) = try await opened()
      let pages = try await viewer.pages(of: document).map(\.page)
      let layout = try await edit.movePages([pages[0], pages[1]], to: .end)
      expectNoDifference(Array(layout.pages.map(\.page).suffix(2)), [pages[0], pages[1]])
      expectNoDifference(layout.pages.count, 14)
    }

    @Test func deletingEveryPageThrowsAndKeepsThePages() async throws {
      let (viewer, document, edit) = try await opened()
      let all = try await viewer.pages(of: document).map(\.page)
      await #expect(throws: PDFError.self) {
        _ = try await edit.deletePages(all)
      }
      let pages = try await viewer.pages(of: document)
      expectNoDifference(pages.count, 14)
    }

    @Test func anInsertedBlankPageTakesItsNeighboursSize() async throws {
      let (viewer, document, edit) = try await opened()
      let first = try await viewer.pages(of: document)[0]
      let insertion = try await edit.insertBlank(PageEditBlankOptions(count: 2, placement: .after(page: first.page)))
      expectNoDifference(insertion.inserted.count, 2)
      expectNoDifference(insertion.pages.count, 16)
      expectNoDifference(insertion.pages[1].width, first.width)
      expectNoDifference(insertion.pages[1].page, insertion.inserted[0])
    }

    @Test func anExtractOpensAsItsOwnDocument() async throws {
      let (viewer, document, edit) = try await opened()
      let pages = try await viewer.pages(of: document).prefix(3).map(\.page)
      let data = try await edit.extract(Array(pages))
      expectNoDifference(data.prefix(4), Data("%PDF".utf8))
      let extracted = try await viewer.open(data)
      expectNoDifference(extracted.pageCount, 3)
    }

    @Test func pagesFromAnotherOpenDocumentInsert() async throws {
      let (viewer, _, edit) = try await opened()
      let other = try await viewer.open(Fixture.data("colors"))
      let source = try await viewer.pages(of: other).map(\.page)
      let insertion = try await edit.insert(source, from: other, at: .index(index: 0))
      expectNoDifference(insertion.inserted.count, source.count)
      expectNoDifference(insertion.pages.count, 14 + source.count)
    }

    @Test func anEditIsObservedAsPagesChanged() async throws {
      let (viewer, document, edit) = try await opened()
      let events = viewer.documentEvents
      let first = try await viewer.pages(of: document)[0].page
      _ = try await edit.setRotation(.clockwise90, of: [first])
      for await event in events {
        guard case let .pagesChanged(documentId) = event else { continue }
        expectNoDifference(documentId, document.id)
        break
      }
    }

    @Test func rotateCancelsWhenTheTaskIsCancelled() async throws {
      let (viewer, document, edit) = try await opened()
      let first = try await viewer.pages(of: document)[0].page
      let task = Task { try await edit.rotate([first], by: .half) }
      task.cancel()
      do {
        _ = try await task.value
      } catch {
        // Cancelled before it started. Once started, the edit lands.
        expectNoDifference((error as? PDFError)?.code, "operation-cancelled")
      }
      // The capability still answers after a cancelled call.
      #expect(try await edit.canEdit())
    }

    @Test func aClosedDocumentsCapabilityThrows() async throws {
      let (viewer, document, edit) = try await opened()
      try await viewer.close(document)
      await #expect {
        _ = try await edit.canEdit()
      } throws: { error in
        let error = error as? PDFError
        return error?.code == "instance-closed" && error?.name == "PluginError"
      }
    }
  }
}
