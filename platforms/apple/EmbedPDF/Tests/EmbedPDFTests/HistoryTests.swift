import CustomDump
import EmbedPDF
import Testing

extension BaseSuite {
  @Suite struct HistoryTests {
    @Test func undoRedoTracksPageEditsAndDocumentsIndependently() async throws {
      let viewer = try PDFViewer(ViewerConfig(history: true, pageEdit: true))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      let other = try await viewer.open(Fixture.data("report"))
      let history = try await viewer.history(for: document)
      let otherHistory = try await viewer.history(for: other)
      let edit = try await viewer.pageEdit(for: document)
      let first = try await viewer.pages(of: document)[0].page
      let stream = history.events
      let completion = Task { for await event in stream { if case .undone = event { return } } }
      _ = try await edit.rotate([first], by: .clockwise)
      #expect(try await history.canUndo())
      #expect(try await !otherHistory.canUndo())
      let label = try #require(try await history.undoLabel())
      #expect(!label.key.isEmpty)
      try await history.undo()
      await completion.value
      let pages = try await viewer.pages(of: document)
      expectNoDifference(pages[0].rotation, 0)
      #expect(try await history.canRedo())
      let redoStream = history.events
      let redone = Task { for await event in redoStream { if case .undone = event { return } } }
      try await history.redo()
      await redone.value
      let rotated = try await viewer.pages(of: document)
      expectNoDifference(rotated[0].rotation, 90)
      try await viewer.destroy()
    }

    @Test func settingsAndEmptyHistoryDoNotRequireCompletionEvents() async throws {
      let viewer = try PDFViewer(ViewerConfig(history: true))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      let history = try await viewer.history(for: document)
      try await history.undo()
      try await history.redo()
      try await history.updateSettings(HistorySettingsPatch(limit: 5))
      let settings = try await history.settings()
      expectNoDifference(settings.limit, 5)
      try await history.resetSettings()
      let defaults = try await history.settings()
      expectNoDifference(defaults.limit, 100)
      try await history.clear()
      try await viewer.close(document)
      await #expect { _ = try await history.canUndo() } throws: { ($0 as? PDFError)?.code == .instanceClosed }
      var ended = history.events.makeAsyncIterator()
      let event = await ended.next()
      expectNoDifference(event, nil)
      try await viewer.destroy()
    }

    @Test func absentHistoryIsUnsupported() async throws {
      let viewer = try PDFViewer(ViewerConfig())
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      await #expect { _ = try await viewer.history(for: document) } throws: { ($0 as? PDFError)?.code == .unsupported }
      try await viewer.destroy()
    }
  }
}
