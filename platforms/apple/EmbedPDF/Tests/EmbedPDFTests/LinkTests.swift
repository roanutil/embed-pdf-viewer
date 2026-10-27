import CustomDump
import EmbedPDF
import Foundation
import Testing

extension BaseSuite {
  @Suite struct LinkTests {
    func opened() async throws -> (PDFViewer, DocumentInfo, LinkCapability, PageRef) {
      let viewer = try PDFViewer(ViewerConfig(interaction: true, link: true))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("features"))
      let page = try await viewer.pages(of: document)[0].page
      let link = try await viewer.link(for: document)
      try await link.ensureLoaded(page)
      return (viewer, document, link, page)
    }

    @Test func aPagesLinksAreListed() async throws {
      let (_, _, link, page) = try await opened()
      let links = try await link.links(on: page)
      expectNoDifference(links.count, 2)
    }

    @Test func aBareTargetIsLabelled() async throws {
      let (_, _, link, page) = try await opened()
      let uri = try await link.label(forTargetJSON: #"{"kind": "uri", "uri": "https://example.invalid/x"}"#)
      expectNoDifference(uri, "https://example.invalid/x")
      let named = try await link.label(forTargetJSON: #"{"kind": "named", "name": "NextPage"}"#)
      expectNoDifference(named, "NextPage")
      // The same label a listed link gets for its own target.
      let listed = try #require(try await link.links(on: page).first)
      let byLink = try await link.label(for: listed)
      let byTarget = try await link.label(forTargetJSON: listed.targetJson)
      expectNoDifference(byTarget, byLink)
    }

    @Test func aURILinkIsReportedToTheApp() async throws {
      let (_, _, link, page) = try await opened()
      let events = link.events
      let activation = try await link.activate(at: Point(x: 100, y: 145), on: page)
      expectNoDifference(activation, .uri(uri: "https://example.invalid/target"))
      for await event in events {
        guard case let .activated(event) = event else { continue }
        expectNoDifference(event.activation, .uri(uri: "https://example.invalid/target"))
        break
      }
    }

    @Test func aGotoLinkWithoutAStageReportsItsDestination() async throws {
      let (_, _, link, page) = try await opened()
      let activation = try await link.activate(at: Point(x: 100, y: 85), on: page)
      guard case let .destination(destinationJSON) = activation else {
        Issue.record("expected a destination, got \(String(describing: activation))")
        return
      }
      // The page-space `PageDestination` record.
      let destination = try JSONSerialization.jsonObject(with: Data(destinationJSON.utf8)) as? [String: Any]
      expectNoDifference(destination?["kind"] as? String, "xyz")
    }

    @Test func aTapOnALinkActivatesItOnceTheTapHandlerIsRegistered() async throws {
      let viewer = try PDFViewer(ViewerConfig(interaction: true, link: true))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("features"))
      let page = try await viewer.pages(of: document)[0].page
      let link = try await viewer.link(for: document)
      let interaction = try await viewer.interaction(for: document)
      try await link.ensureLoaded(page)
      let registration = try await link.registerTapHandler()
      defer { registration.unregister() }
      let events = link.events
      // Inside the external link: x 40...360, y 130...160.
      let at = Point(x: 100, y: 145)
      for phase in [PointerPhase.down, .up] {
        interaction.dispatchPointer(
          PointerSample(
            phase: phase, viewport: at, page: PointerPageHit(page: page, point: at),
            modifiers: Modifiers(), pointerType: .touch))
      }
      for await event in events {
        guard case let .activated(event) = event else { continue }
        expectNoDifference(event.activation, .uri(uri: "https://example.invalid/target"))
        break
      }
    }

    @Test func aClosedDocumentsCapabilityThrows() async throws {
      let (viewer, document, link, page) = try await opened()
      try await viewer.close(document)
      await #expect {
        _ = try await link.links(on: page)
      } throws: { error in
        (error as? PDFError)?.code == "instance-closed"
      }
    }

    @Test func droppingTheStreamUnsubscribes() async throws {
      let (_, _, link, page) = try await opened()
      let task = Task { for await _ in link.events {} }
      try await Task.sleep(for: .milliseconds(20))
      task.cancel()
      _ = await task.value
      _ = try await link.activate(at: Point(x: 100, y: 145), on: page)
    }
  }
}
