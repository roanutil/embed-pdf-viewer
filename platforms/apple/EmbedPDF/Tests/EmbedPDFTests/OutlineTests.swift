import CustomDump
import EmbedPDF
import Testing

extension BaseSuite {
  @Suite struct OutlineTests {
    @Test func nestedOutlineEditsPersistAndPatchClearingIsExplicit() async throws {
      let viewer = try PDFViewer(ViewerConfig(outline: true))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      let outline = try await viewer.outline(for: document)
      try await outline.ensureLoaded()
      let parent = try await outline.create(OutlineItemInput(title: "Parent", clearTarget: true), options: OutlineCreateOptions(select: true))
      let child = try await outline.create(OutlineItemInput(title: "Child", parent: parent.itemRef, clearTarget: true, color: "#ff0000"))
      try await outline.expand(parent.itemRef)
      let rows = try await outline.rows()
      expectNoDifference(rows.first { $0.item.itemRef == child.itemRef }?.depth, 1)
      let cleared = try await outline.update(child.itemRef, OutlineItemPatch(title: "Renamed", clear: [.color, .target]))
      expectNoDifference(cleared.title, "Renamed")
      expectNoDifference(cleared.color, nil)
      try await outline.move([child.itemRef], to: nil, position: .start)
      let moved = try #require(try await outline.item(child.itemRef))
      expectNoDifference(moved.parent, nil)
      await expectCancelledBeforeItStarts { try await outline.refresh() }
      let bytes = try await viewer.save(document)
      let reopened = try await viewer.open(bytes)
      let saved = try await viewer.outline(for: reopened)
      try await saved.ensureLoaded()
      let persisted = try #require(try await saved.item(child.itemRef))
      expectNoDifference(persisted.title, "Renamed")
      try await outline.delete([child.itemRef, parent.itemRef])
      let removed = try await outline.item(parent.itemRef)
      #expect(removed == nil)
      try await viewer.close(document)
      var ended = outline.events.makeAsyncIterator()
      let end = await ended.next()
      #expect(end == nil)
      try await viewer.destroy()
    }

    @Test func editingIsUnavailableWithRestrictedScope() async throws {
      let viewer = try PDFViewer(ViewerConfig(scope: ["doc.open"], outline: true))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      let outline = try await viewer.outline(for: document)
      #expect(try await !outline.canEdit())
      await #expect { _ = try await outline.create(OutlineItemInput(title: "Denied")) } throws: { ($0 as? PDFError)?.code == .permissionDenied }
      try await viewer.destroy()
    }
  }
}
