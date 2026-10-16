import CustomDump
import EmbedPDF
import Testing

extension BaseSuite {
  @Suite struct ViewManagerTests {
    func opened() async throws -> (PDFViewer, DocumentInfo, ViewManagerCapability) {
      let viewer = try PDFViewer(ViewerConfig(viewManager: true))
      try await viewer.start()
      let manager = try await viewer.viewManager()
      let document = try await viewer.open(Fixture.data("report"))
      return (viewer, document, manager)
    }

    @Test func theFirstDocumentLandsInTheDefaultPane() async throws {
      let (_, document, manager) = try await opened()
      let panes = try await manager.panes()
      expectNoDifference(panes, [PaneInfo(id: "pane-1", documentIds: [document.id], activeDocumentId: document.id)])
      let focused = try await manager.focusedPaneId()
      expectNoDifference(focused, "pane-1")
    }

    @Test func splittingIsObserved() async throws {
      let (viewer, _, manager) = try await opened()
      let colors = try await viewer.open(Fixture.data("colors"))
      let events = manager.events
      let pane = try await manager.splitPane(showing: colors)
      for await event in events {
        guard case let .documentMoved(event) = event else { continue }
        expectNoDifference(event, DocumentMovedEvent(documentId: colors.id, fromPaneId: "pane-1", toPaneId: pane))
        break
      }
      let holder = try await manager.pane(holding: colors)
      expectNoDifference(holder, pane)
    }

    @Test func closingADocumentTakesItOutOfItsPane() async throws {
      let (viewer, document, manager) = try await opened()
      let colors = try await viewer.open(Fixture.data("colors"))
      try await manager.setActiveDocument(document, in: "pane-1")
      try await viewer.close(document)
      let pane = try await manager.pane("pane-1")
      expectNoDifference(pane?.documentIds, [colors.id])
      expectNoDifference(pane?.activeDocumentId, colors.id)
    }

    @Test func documentsMoveBetweenPanesAsDocumentInfos() async throws {
      let (viewer, document, manager) = try await opened()
      let colors = try await viewer.open(Fixture.data("colors"))
      let pane = try await manager.createPane(documents: [colors])
      try await manager.addDocument(document, to: pane, at: 0)
      var info = try await manager.pane(pane)
      expectNoDifference(info?.documentIds, [document.id, colors.id])
      try await manager.moveDocument(colors, within: pane, to: 0)
      try await manager.setActiveDocument(colors, in: pane)
      info = try await manager.pane(pane)
      expectNoDifference(info?.documentIds, [colors.id, document.id])
      expectNoDifference(info?.activeDocumentId, colors.id)
      try await manager.moveDocument(document, from: pane, to: "pane-1")
      let holder = try await manager.pane(holding: document)
      expectNoDifference(holder, "pane-1")
      try await manager.removeDocument(colors, from: pane)
      let removed = try await manager.pane(holding: colors)
      expectNoDifference(removed, nil)
    }

    @Test func anIndexPastTheEndAppends() async throws {
      let (_, _, manager) = try await opened()
      let second = try await manager.createPane()
      try await manager.movePane("pane-1", to: .max)
      let order = try await manager.paneOrder()
      expectNoDifference(order, [second, "pane-1"])
    }

    @Test func droppingTheStreamUnsubscribes() async throws {
      let (_, _, manager) = try await opened()
      let task = Task { for await _ in manager.events {} }
      try await Task.sleep(for: .milliseconds(20))
      task.cancel()
      _ = await task.value
      _ = try await manager.createPane()
    }

    @Test func destroyingTheViewerFinishesTheEvents() async throws {
      let (viewer, _, manager) = try await opened()
      let events = Task { for await _ in manager.events {} }
      try await Task.sleep(for: .milliseconds(50))
      try await viewer.destroy()
      await events.value
    }

    @Test func aDestroyedViewersCapabilityThrows() async throws {
      let viewer = try PDFViewer(ViewerConfig(viewManager: true))
      try await viewer.start()
      let manager = try await viewer.viewManager()
      try await viewer.destroy()
      await #expect {
        _ = try await manager.panes()
      } throws: { error in
        // `destroy` stops the kernel and then the thread, so the call
        // answers either the plugin's `instance-closed` or the viewer's.
        (error as? PDFError)?.code == "instance-closed"
      }
    }
  }
}
