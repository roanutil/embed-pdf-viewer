import CustomDump
import EmbedPDF
import Testing

extension BaseSuite {
  @Suite struct RevisionTests {
    @Test func independentObserversSurviveAnotherConsumersCancellation() async throws {
      let (viewer, _, metadata) = try await MetadataTests().opened()
      try await metadata.ensureLoaded()
      let first = viewer.observeStateRevisions()
      let second = viewer.observeStateRevisions()
      let cancelled = Task { for await _ in viewer.observeStateRevisions() {} }
      cancelled.cancel()
      await cancelled.value
      _ = try await metadata.update(MetadataPatch(title: "Broadcast"))
      var firstIterator = first.makeAsyncIterator()
      var secondIterator = second.makeAsyncIterator()
      let firstValue = await firstIterator.next()
      let secondValue = await secondIterator.next()
      #expect(firstValue != nil)
      #expect(secondValue != nil)
      try await viewer.destroy()
      for await _ in first {}
      for await _ in second {}
      var late = viewer.observeStateRevisions().makeAsyncIterator()
      let end = await late.next()
      expectNoDifference(end, nil)
    }

    @Test func subscribingBeforeTheInitialReadDoesNotLoseAMutation() async throws {
      let (viewer, _, metadata) = try await MetadataTests().opened()
      let stream = viewer.observeStateRevisions()
      try await metadata.ensureLoaded()
      let initial = try await metadata.snapshot()
      _ = try await metadata.update(MetadataPatch(title: "After subscription"))
      var iterator = stream.makeAsyncIterator()
      let changed = await iterator.next()
      #expect(changed != nil)
      let current = try await metadata.snapshot()
      expectNoDifference(current?.title, "After subscription")
      #expect(initial?.title != current?.title)
      try await viewer.destroy()
    }
  }
}
