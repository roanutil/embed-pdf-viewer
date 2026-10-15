import CustomDump
import EmbedPDF
import Testing

extension BaseSuite {
  @Suite struct MetadataTests {
    func opened() async throws -> (PDFViewer, PDFDocument, MetadataCapability) {
      let viewer = try PDFViewer(ViewerConfig(metadata: true))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      return (viewer, document, try await viewer.metadata(for: document))
    }

    @Test func refreshThenUpdateIsObserved() async throws {
      let (_, _, metadata) = try await opened()
      try await metadata.refresh()
      let status = try await metadata.status()
      expectNoDifference(status, .ready)
      let events = metadata.events
      let updated = try await metadata.update(MetadataPatch(title: "Renamed"))
      expectNoDifference(updated.title, "Renamed")
      for await event in events {
        guard case let .updated(event) = event else { continue }
        expectNoDifference(event.changedKeys, ["title"])
        break
      }
      let snapshot = try await metadata.snapshot()
      expectNoDifference(snapshot?.title, "Renamed")
    }

    @Test func aClosedDocumentsCapabilityThrows() async throws {
      let (viewer, document, metadata) = try await opened()
      try await viewer.close(document)
      await #expect {
        _ = try await metadata.snapshot()
      } throws: { error in
        // `PluginError`, not `Closed`: the viewer is still running.
        let error = error as? PDFError
        return error?.code == "instance-closed" && error?.name == "PluginError"
      }
    }

    @Test func droppingTheStreamUnsubscribes() async throws {
      let (_, _, metadata) = try await opened()
      let task = Task { for await _ in metadata.events {} }
      try await Task.sleep(for: .milliseconds(20))
      task.cancel()
      _ = await task.value
      try await metadata.refresh()
    }

    @Test func cancellingARefreshDoesNotHang() async throws {
      let (_, _, metadata) = try await opened()
      let task = Task { try await metadata.refresh() }
      task.cancel()
      do {
        try await task.value
      } catch {
        expectNoDifference((error as? PDFError)?.code, "operation-cancelled")
      }
      try await metadata.refresh()
    }

    @Test func updateCancelsWhenTheTaskIsCancelled() async throws {
      let (_, _, metadata) = try await opened()
      try await metadata.refresh()
      await expectCancelledBeforeItStarts { try await metadata.update(MetadataPatch(title: "Renamed")) }
      let snapshot = try await metadata.snapshot()
      #expect(snapshot?.title != "Renamed")
    }

    @Test func refreshCancelsWhenTheTaskIsCancelled() async throws {
      let (_, _, metadata) = try await opened()
      await expectCancelledBeforeItStarts { try await metadata.refresh() }
    }

    @Test func destroyingTheViewerFinishesTheEvents() async throws {
      let (viewer, _, metadata) = try await opened()
      let events = Task { for await _ in metadata.events {} }
      try await Task.sleep(for: .milliseconds(50))
      try await viewer.destroy()
      await events.value
    }

    @Test func closingTheDocumentFinishesTheEvents() async throws {
      let (viewer, document, metadata) = try await opened()
      let events = Task { for await _ in metadata.events {} }
      try await Task.sleep(for: .milliseconds(50))
      try await viewer.close(document)
      let closed = await finishes(events)
      expectNoDifference(closed, true)
      // A stream made after the close ends at once.
      let late = await finishes(Task { for await _ in metadata.events {} })
      expectNoDifference(late, true)
    }

    /// Whether `task` finishes within two seconds.
    func finishes(_ task: Task<Void, Never>) async -> Bool {
      await withTaskGroup(of: Bool.self) { group in
        group.addTask {
          await task.value
          return true
        }
        group.addTask {
          try? await Task.sleep(for: .seconds(2))
          return false
        }
        let first = await group.next() ?? false
        group.cancelAll()
        task.cancel()
        return first
      }
    }
  }
}
