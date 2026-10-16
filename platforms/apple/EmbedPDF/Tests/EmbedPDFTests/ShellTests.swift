import CustomDump
import EmbedPDF
import Testing

extension BaseSuite {
  @Suite struct ShellTests {
    struct StyleProps: Codable, Equatable, Sendable {
      var annotationId: String
      var page: Int
    }

    func opened() async throws -> (PDFViewer, DocumentInfo, ShellCapability) {
      let viewer = try PDFViewer(ViewerConfig(shell: true))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      return (viewer, document, try await viewer.shell(for: document))
    }

    @Test func typedPropsRoundTrip() async throws {
      let (_, _, shell) = try await opened()
      try await shell.open("style", props: StyleProps(annotationId: "a1", page: 3))
      let surface = try #require(try await shell.surface("style"))
      expectNoDifference(surface.open, true)
      expectNoDifference(try surface.props(as: StyleProps.self), StyleProps(annotationId: "a1", page: 3))
    }

    @Test func aNeverOpenedSurfaceIsNil() async throws {
      let (_, _, shell) = try await opened()
      let surface = try await shell.surface("thumbnails")
      expectNoDifference(surface, nil)
    }

    /// `surfaces[id]` on the TypeScript plugin's plain object finds an
    /// `Object.prototype` member, so these ids are found, closed and bare.
    @Test(arguments: ["constructor", "toString"])
    func anObjectPrototypeNameIsFoundClosed(id: String) async throws {
      let (_, _, shell) = try await opened()
      let surface = try await shell.surface(id)
      expectNoDifference(surface, ShellSurface(id: id, open: false, exclusive: nil, propsJson: nil))
      let isOpen = try await shell.isOpen(id)
      expectNoDifference(isOpen, false)
    }

    @Test func propsThatAreNotAnObjectThrow() async throws {
      let (_, _, shell) = try await opened()
      await #expect {
        try await shell.open("style", props: [1, 2, 3])
      } throws: { error in
        (error as? PDFError)?.code == "invalid-input"
      }
      let isOpen = try await shell.isOpen("style")
      expectNoDifference(isOpen, false)
    }

    @Test func anExclusiveOpenIsObserved() async throws {
      let (_, _, shell) = try await opened()
      try await shell.open("thumbnails", exclusive: "left")
      let events = shell.events
      try await shell.open("search", exclusive: "left")
      var seen: [ShellEvent] = []
      for await event in events {
        seen.append(event)
        if seen.count == 2 { break }
      }
      expectNoDifference(
        seen,
        [
          .surfaceClosed(event: ShellSurfaceEvent(id: "thumbnails", propsJson: nil)),
          .surfaceOpened(event: ShellSurfaceEvent(id: "search", propsJson: nil)),
        ])
    }

    @Test func aSnapshotRestores() async throws {
      let (_, _, shell) = try await opened()
      try await shell.open("sidebar", exclusive: "left")
      try await shell.openMenu("file")
      let snapshot = try await shell.snapshot()
      try await shell.closeAll()
      try await shell.closeAllMenus()
      try await shell.applySnapshot(snapshot)
      let restored = try await shell.snapshot()
      expectNoDifference(restored, snapshot)
    }

    @Test func eachDocumentKeepsItsOwnSurfaces() async throws {
      let (viewer, _, first) = try await opened()
      let colors = try await viewer.open(Fixture.data("colors"))
      let second = try await viewer.shell(for: colors)
      try await first.open("sidebar")
      let firstOpen = try await first.isOpen("sidebar")
      let secondOpen = try await second.isOpen("sidebar")
      expectNoDifference(firstOpen, true)
      expectNoDifference(secondOpen, false)
    }

    @Test func droppingTheStreamUnsubscribes() async throws {
      let (_, _, shell) = try await opened()
      let task = Task { for await _ in shell.events {} }
      try await Task.sleep(for: .milliseconds(20))
      task.cancel()
      _ = await task.value
      try await shell.open("sidebar")
    }

    @Test func closingTheDocumentFinishesTheEvents() async throws {
      let (viewer, document, shell) = try await opened()
      let events = Task { for await _ in shell.events {} }
      try await Task.sleep(for: .milliseconds(50))
      try await viewer.close(document)
      await events.value
    }

    @Test func aClosedDocumentsCapabilityThrows() async throws {
      let (viewer, document, shell) = try await opened()
      try await viewer.close(document)
      await #expect {
        _ = try await shell.isOpen("sidebar")
      } throws: { error in
        let error = error as? PDFError
        return error?.code == "instance-closed" && error?.name == "PluginError"
      }
    }
  }
}
