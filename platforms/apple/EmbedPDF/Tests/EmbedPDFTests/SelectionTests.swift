import CustomDump
import EmbedPDF
import Testing

extension BaseSuite {
  @Suite struct SelectionTests {
    func opened(search: Bool = false) async throws -> (PDFViewer, DocumentInfo, SelectionCapability, PageRef) {
      let viewer = try PDFViewer(
        ViewerConfig(search: search ? SearchConfig() : nil, interaction: true, selection: true))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      let page = try await viewer.pages(of: document)[0].page
      return (viewer, document, try await viewer.selection(for: document), page)
    }

    @Test func selectingAPageReadsItsText() async throws {
      let (_, _, selection, page) = try await opened()
      try await selection.selectPage(page)
      let text = try await selection.readText()
      // report.pdf opens with the title "Trace-based Just-in-Time Type
      // Specialization for Dynamic Languages".
      #expect(text.contains("Dynamic"))
      #expect(try await selection.rects(on: page).isEmpty == false)
    }

    @Test func aSearchHitSelectsItsCharacters() async throws {
      let (viewer, document, selection, _) = try await opened(search: true)
      let search = try await viewer.search(in: document)
      let hit = try #require(try await search.findAll(SearchQuery(text: "Report")).first)
      try await selection.select(hit)
      // Search matches case-insensitively by default, so compare that way.
      let text = try await selection.readText()
      expectNoDifference(text.lowercased(), "report")
    }

    @Test func selectAndClearAreObserved() async throws {
      let (_, _, selection, page) = try await opened()
      let events = selection.events
      try await selection.selectPage(page)
      try await selection.clear()
      var kinds: [String] = []
      for await event in events {
        switch event {
        case .changed: kinds.append("changed")
        case .committed: kinds.append("committed")
        case .cleared: kinds.append("cleared")
        }
        if kinds.contains("cleared") { break }
      }
      #expect(kinds.first == "changed")
    }

    @Test func readTextCancelsWhenTheTaskIsCancelled() async throws {
      let (_, _, selection, _) = try await opened()
      try await selection.selectAll()
      let task = Task { try await selection.readText() }
      task.cancel()
      do {
        _ = try await task.value
      } catch {
        expectNoDifference((error as? PDFError)?.code, "operation-cancelled")
      }
      #expect(try await selection.readText().isEmpty == false)
    }

    @Test func readTextInRangeCancelsWhenTheTaskIsCancelled() async throws {
      let (_, _, selection, page) = try await opened()
      let range = SelectionTextRange(
        start: SelectionTextPosition(page: page, index: 0),
        end: SelectionTextPosition(page: page, index: 20))
      let task = Task { try await selection.readText(in: range) }
      task.cancel()
      do {
        _ = try await task.value
      } catch {
        expectNoDifference((error as? PDFError)?.code, "operation-cancelled")
      }
      let text = try await selection.readText(in: range)
      expectNoDifference(text.utf16.count, 20)
    }

    @Test func aClosedDocumentsCapabilityThrows() async throws {
      let (viewer, document, selection, _) = try await opened()
      try await viewer.close(document)
      await #expect {
        _ = try await selection.hasSelection()
      } throws: { error in
        (error as? PDFError)?.code == "instance-closed"
      }
    }

    @Test func droppingTheStreamUnsubscribes() async throws {
      let (_, _, selection, page) = try await opened()
      let task = Task { for await _ in selection.events {} }
      try await Task.sleep(for: .milliseconds(20))
      task.cancel()
      _ = await task.value
      try await selection.selectPage(page)
    }
  }
}
