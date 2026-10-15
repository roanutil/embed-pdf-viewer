import CustomDump
import EmbedPDF
import Testing

extension BaseSuite {
  @Suite struct ViewerTests {
    func started() async throws -> PDFViewer {
      let viewer = try PDFViewer(ViewerConfig())
      try await viewer.start()
      return viewer
    }

    @Test func aDocumentOpensWithItsPages() async throws {
      let viewer = try await started()
      let document = try await viewer.open(Fixture.data("report"), name: "report.pdf")
      expectNoDifference(document.pageCount, 14)
      expectNoDifference(document.name, "report.pdf")
      let pages = try await viewer.pages(of: document)
      expectNoDifference(pages.count, 14)
    }

    @Test func aLockedDocumentOpensAsALockedTab() async throws {
      let viewer = try await started()
      let document = try await viewer.open(Fixture.data("encrypted-hello"))
      expectNoDifference(document.status, .locked)
    }

    @Test func aFailedOpenListsItsTabAsFailed() async throws {
      let viewer = try await started()
      _ = try? await viewer.open(Fixture.data("encrypted-hello"), password: "nope")
      let documents = try await viewer.documents()
      expectNoDifference(documents.map(\.status), [DocumentStatus.failed])
      expectNoDifference(documents.first?.error?.message.hasPrefix("DocPasswordIncorrect"), true)
    }

    @Test func aFailedOpenAnnouncesTheTabsErrorWithItsCode() async throws {
      let viewer = try await started()
      let events = viewer.documentEvents
      _ = try? await viewer.open(Fixture.data("encrypted-hello"), password: "nope")
      let tab = try #require(try await viewer.documents().first)
      for await event in events {
        guard case let .openFailed(documentId, error) = event else { continue }
        expectNoDifference(documentId, tab.id)
        expectNoDifference(error, tab.error)
        expectNoDifference(error.code, "permission-denied")
        break
      }
    }

    @Test func aLockedOpenSaysNoPasswordWasGiven() async throws {
      let viewer = try await started()
      let events = viewer.documentEvents
      let document = try await viewer.open(Fixture.data("encrypted-hello"))
      for await event in events {
        guard case let .locked(documentId, passwordProvided) = event else { continue }
        expectNoDifference(documentId, document.id)
        expectNoDifference(passwordProvided, false)
        break
      }
    }

    @Test func aSavedDocumentReopensInEitherMode() async throws {
      let viewer = try await started()
      let original = try Fixture.data("report")
      let document = try await viewer.open(original)
      let incremental = try await viewer.save(document)
      expectNoDifference(incremental.prefix(original.count), original)
      let rewritten = try await viewer.save(document, mode: .rewrite)
      for bytes in [incremental, rewritten] {
        let again = try await viewer.open(bytes)
        expectNoDifference(again.pageCount, 14)
      }
    }

    @Test func aSignedDocumentRefusesARewriteSave() async throws {
      let viewer = try await started()
      let document = try await viewer.open(Fixture.data("signed_two_approvals"))
      await #expect {
        _ = try await viewer.save(document, mode: .rewrite)
      } throws: { error in
        (error as? PDFError)?.code == "permission-denied"
      }
      _ = try await viewer.save(document, mode: .incremental)
    }

    @Test func saveCancelsWhenTheTaskIsCancelled() async throws {
      let viewer = try await started()
      let document = try await viewer.open(Fixture.data("report"))
      let task = Task { try await viewer.save(document, mode: .rewrite) }
      task.cancel()
      do { _ = try await task.value } catch { expectNoDifference((error as? PDFError)?.code, "operation-cancelled") }
      _ = try await viewer.save(document)
    }

    @Test func openingAndClosingAreObserved() async throws {
      let viewer = try await started()
      let events = viewer.documentEvents
      let document = try await viewer.open(Fixture.data("report"))
      try await viewer.close(document)
      var seen: [String] = []
      for await event in events {
        switch event {
        case .opened: seen.append("opened")
        case .closed: seen.append("closed")
        default: continue
        }
        if seen.count == 2 { break }
      }
      expectNoDifference(seen, ["opened", "closed"])
    }

    @Test func destroyingFinishesTheStreams() async throws {
      let viewer = try await started()
      let events = Task { for await _ in viewer.documentEvents {} }
      let revisions = Task { for await _ in viewer.stateRevisions {} }
      try await Task.sleep(for: .milliseconds(100))
      try await viewer.destroy()
      let finished = await withTaskGroup(of: Bool.self) { group in
        group.addTask {
          await events.value
          await revisions.value
          return true
        }
        group.addTask {
          try? await Task.sleep(for: .seconds(5))
          return false
        }
        let first = await group.next() ?? false
        events.cancel()
        revisions.cancel()
        group.cancelAll()
        return first
      }
      expectNoDifference(finished, true)
    }

    @Test func aCallAfterDestroyIsClosed() async throws {
      let viewer = try await started()
      try await viewer.destroy()
      await #expect {
        _ = try await viewer.documents()
      } throws: { error in
        (error as? PDFError)?.code == "instance-closed"
      }
    }

    @Test func reportedErrorsFinishWhenTheViewerIsDestroyed() async throws {
      let viewer = try await started()
      let drained = Task {
        var count = 0
        for await _ in viewer.reportedErrors { count += 1 }
        return count
      }
      try await viewer.destroy()
      let count = await drained.value
      expectNoDifference(count, 0)
    }

    @Test func theFeaturesFixtureIsBundled() async throws {
      let viewer = try await started()
      let document = try await viewer.open(Fixture.data("features"))
      expectNoDifference(document.pageCount, 3)
    }
  }
}
