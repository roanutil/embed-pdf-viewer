import CustomDump
import EmbedPDF
import Foundation
import Testing

extension BaseSuite {
  @Suite struct AttachmentTests {
    @Test func batchImportDownloadsAndReopensWithDistinctUnicodeNames() async throws {
      let viewer = try PDFViewer(ViewerConfig(attachment: true))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      let attachments = try await viewer.attachments(for: document)
      try await attachments.ensureLoaded()
      let data = Data([0, 255, 128, 13, 10])
      let added = try await attachments.add([
        AttachmentFileSource(data: data, name: "Résumé.bin"),
        AttachmentFileSource(data: Data(), name: "Résumé.bin")
      ])
      expectNoDifference(added.map(\.name), ["Résumé.bin", "Résumé (2).bin"])
      #expect(added[0].id != added[1].id)
      let content = try await attachments.download(added[0])
      expectNoDifference(content.data, data)
      expectNoDifference(content.mimeType, nil)
      try await attachments.select(added)
      let selected = try await attachments.selectedFiles()
      expectNoDifference(selected.map(\.id), added.map(\.id))
      let empty = try await attachments.add([])
      expectNoDifference(empty, [])
      await expectCancelledBeforeItStarts { try await attachments.download(added[0]) }
      await expectCancelledBeforeItStarts { try await attachments.add([AttachmentFileSource(data: data, name: "cancelled")]) }
      let bytes = try await viewer.save(document)
      let reopened = try await viewer.open(bytes)
      let saved = try await viewer.attachments(for: reopened)
      try await saved.ensureLoaded()
      let files = try await saved.files()
      let savedFile = try #require(files.first { $0.name == "Résumé.bin" })
      let persisted = try await saved.download(savedFile)
      expectNoDifference(persisted.data, data)
      try await attachments.remove(added[0])
      let removed = try await attachments.file(added[0].id)
      expectNoDifference(removed, nil)
      try await viewer.close(document)
      var events = attachments.events.makeAsyncIterator()
      let end = await events.next()
      expectNoDifference(end, nil)
      try await viewer.destroy()
    }

    @Test func pageAttachmentDownloadsAndRevealsThroughAnnotationCapability() async throws {
      let viewer = try PDFViewer(ViewerConfig(interaction: true, annotation: "", attachment: true))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      let annotations = try await viewer.annotations(for: document)
      let page = try await viewer.pages(of: document)[0].page
      let data = Data("attached".utf8)
      let ref = try await annotations.drafts.createAttachment(AnnotationAttachmentFile(bytes: data, name: "page.txt"), at: Point(x: 40, y: 40), on: page)
      let attachments = try await viewer.attachments(for: document)
      try await attachments.ensureLoaded()
      let file = try #require(try await attachments.files().first { if case .page = $0.home { return true }; return false })
      let content = try await attachments.download(file)
      expectNoDifference(content.data, data)
      try await attachments.reveal(file)
      let selected = try await annotations.selected()
      #expect(selected.contains { $0.annotationRef == ref })
      try await viewer.destroy()
    }
  }
}
