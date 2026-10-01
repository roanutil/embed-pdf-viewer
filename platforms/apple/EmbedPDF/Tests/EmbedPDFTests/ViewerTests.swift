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

    @Test func aLockedDocumentReportsThatItNeedsAPassword() async throws {
      let viewer = try await started()
      await #expect {
        _ = try await viewer.open(Fixture.data("encrypted-hello"))
      } throws: { error in
        (error as? PDFError)?.detail.contains("DocPasswordRequired") == true
      }
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

    @Test func aCallAfterDestroyIsClosed() async throws {
      let viewer = try await started()
      try await viewer.destroy()
      await #expect {
        _ = try await viewer.documents()
      } throws: { error in
        (error as? PDFError)?.code == "instance-closed"
      }
    }
  }
}
