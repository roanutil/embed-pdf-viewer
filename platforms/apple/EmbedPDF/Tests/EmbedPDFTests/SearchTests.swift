import CustomDump
import EmbedPDF
import Testing

extension BaseSuite {
  @Suite struct SearchTests {
    func opened() async throws -> (PDFViewer, PDFDocument, SearchCapability) {
      let viewer = try PDFViewer(ViewerConfig(search: SearchConfig()))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      return (viewer, document, try await viewer.search(in: document))
    }

    @Test func aQueryFindsHitsAndNavigatesThem() async throws {
      let (_, _, search) = try await opened()
      let result = try await search.search(SearchQuery(text: "the"))
      expectNoDifference(result.status, .complete)
      #expect(result.hitCount > 1)
      let status = try await search.status()
      expectNoDifference(status, .complete)
      // The first hit becomes active as the hits arrive; navigating moves on.
      let first = try await search.activeHitIndex()
      expectNoDifference(first, 0)
      let next = try await search.nextHit()
      let index = try await search.activeHitIndex()
      expectNoDifference(index, 1)
      let active = try await search.activeHit()
      expectNoDifference(active, next)
      #expect(next?.snippet?.text.localizedCaseInsensitiveContains("the") == true)
      let count = try await search.hitCount()
      expectNoDifference(count, result.hitCount)
    }

    @Test func completionIsObserved() async throws {
      let (_, _, search) = try await opened()
      let events = search.events
      let result = try await search.search(SearchQuery(text: "the"))
      for await event in events {
        guard case let .completed(event) = event else { continue }
        expectNoDifference(event.hitCount, result.hitCount)
        break
      }
    }

    @Test func searchCancelsWhenTheTaskIsCancelled() async throws {
      let (_, _, search) = try await opened()
      let task = Task { try await search.findAll(SearchQuery(text: "e")) }
      task.cancel()
      await #expect {
        _ = try await task.value
      } throws: { error in
        (error as? PDFError)?.code == "operation-cancelled"
      }
      // The capability still answers after a cancelled call.
      let hits = try await search.findAll(SearchQuery(text: "Report"))
      #expect(!hits.isEmpty)
    }

    @Test func aClosedDocumentsCapabilityThrows() async throws {
      let (viewer, document, search) = try await opened()
      try await viewer.close(document)
      await #expect {
        _ = try await search.search(SearchQuery(text: "the"))
      } throws: { error in
        let error = error as? PDFError
        return error?.code == "instance-closed" && error?.name == "PluginError"
      }
    }
  }
}
