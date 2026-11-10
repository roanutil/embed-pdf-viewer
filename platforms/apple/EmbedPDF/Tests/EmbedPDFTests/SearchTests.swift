import CustomDump
import EmbedPDF
import Testing

extension BaseSuite {
  @Suite struct SearchTests {
    func opened() async throws -> (PDFViewer, DocumentInfo, SearchCapability) {
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
      #expect(next?.snippet?.matched.localizedCaseInsensitiveContains("the") == true)
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

    @Test func findAllCancelsWhenTheTaskIsCancelled() async throws {
      let (_, _, search) = try await opened()
      let task = Task { try await search.findAll(SearchQuery(text: "e")) }
      task.cancel()
      await #expect {
        _ = try await task.value
      } throws: { error in
        (error as? PDFError)?.code == .operationCancelled
      }
      // The capability still answers after a cancelled call.
      let hits = try await search.findAll(SearchQuery(text: "Report"))
      #expect(!hits.isEmpty)
    }

    @Test func searchCancelsWhenTheTaskIsCancelled() async throws {
      let (_, _, search) = try await opened()
      await expectCancelledBeforeItStarts { try await search.search(SearchQuery(text: "the")) }
    }

    @Test func refreshCancelsWhenTheTaskIsCancelled() async throws {
      let (_, _, search) = try await opened()
      _ = try await search.search(SearchQuery(text: "the"))
      await expectCancelledBeforeItStarts { try await search.refresh() }
    }

    @Test func droppingTheStreamUnsubscribes() async throws {
      let (_, _, search) = try await opened()
      let task = Task { for await _ in search.events {} }
      try await Task.sleep(for: .milliseconds(20))
      task.cancel()
      _ = await task.value
      _ = try await search.search(SearchQuery(text: "the"))
    }

    /// May search, may not copy: the scope where `snippets` changes answers.
    func openedWithoutCopy() async throws -> SearchCapability {
      let viewer = try PDFViewer(ViewerConfig(scope: ["doc.open", "doc.text.search"], search: SearchConfig()))
      try await viewer.start()
      return try await viewer.search(in: try await viewer.open(Fixture.data("report")))
    }

    @Test func canSearchLeftOutIsWithoutSnippets() async throws {
      let search = try await openedWithoutCopy()
      let omitted = try await search.canSearch()
      let without = try await search.canSearch(snippets: false)
      let with = try await search.canSearch(snippets: true)
      expectNoDifference([omitted, without, with], [true, true, false])
    }

    @Test func findAllLeftOutCarriesSnippetsWhenTheSessionMayCopy() async throws {
      let (_, _, search) = try await opened()
      let omitted = try await search.findAll(SearchQuery(text: "Report"))
      let always = try await search.findAll(SearchQuery(text: "Report"), snippets: .always)
      let never = try await search.findAll(SearchQuery(text: "Report"), snippets: .never)
      #expect(!omitted.isEmpty)
      expectNoDifference(omitted, always)
      #expect(never.allSatisfy { $0.snippet == nil })
      #expect(omitted.allSatisfy { $0.snippet != nil })
    }

    @Test func findAllLeftOutDropsSnippetsWhenTheSessionMayNotCopy() async throws {
      let search = try await openedWithoutCopy()
      let omitted = try await search.findAll(SearchQuery(text: "Report"))
      let never = try await search.findAll(SearchQuery(text: "Report"), snippets: .never)
      #expect(!omitted.isEmpty)
      expectNoDifference(omitted, never)
      await #expect {
        _ = try await search.findAll(SearchQuery(text: "Report"), snippets: .always)
      } throws: { error in
        (error as? PDFError)?.code == .permissionDenied
      }
    }

    @available(*, deprecated)
    @Test func theBoolOverloadsPassNilThrough() async throws {
      let search = try await openedWithoutCopy()
      let nilSnippets: Bool? = nil
      let can = try await search.canSearch(snippets: nilSnippets)
      let omitted = try await search.canSearch()
      expectNoDifference(can, omitted)
      let hits = try await search.findAll(SearchQuery(text: "Report"), snippets: nilSnippets)
      let defaulted = try await search.findAll(SearchQuery(text: "Report"))
      expectNoDifference(hits, defaulted)
      let pinned = try await search.findAll(SearchQuery(text: "Report"), snippets: false as Bool?)
      let never = try await search.findAll(SearchQuery(text: "Report"), snippets: .never)
      expectNoDifference(pinned, never)
    }

    @Test func aClosedDocumentsCapabilityThrows() async throws {
      let (viewer, document, search) = try await opened()
      try await viewer.close(document)
      await #expect {
        _ = try await search.search(SearchQuery(text: "the"))
      } throws: { error in
        let error = error as? PDFError
        return error?.code == .instanceClosed && error?.name == "PluginError"
      }
    }
  }
}
