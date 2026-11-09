import CustomDump
import EmbedPDF
import Testing

extension BaseSuite {
  @Suite struct APICoverageTests {
    @Test func customMetadataPreservesOmittedKeysAndClearsExplicitNull() async throws {
      let (viewer, document, metadata) = try await MetadataTests().opened()
      defer { _ = viewer }
      try await metadata.refreshCustom()
      _ = try await metadata.updateCustom(["Project": "Apple", "Keep": "yes"])
      let patch: [String: String?] = ["Project": nil]
      let changed = try await metadata.updateCustom(patch)
      expectNoDifference(changed["Project"], nil)
      expectNoDifference(changed["Keep"], "yes")
      try await metadata.refreshCustom()
      let snapshot = try await metadata.customSnapshot()
      expectNoDifference(snapshot?["Keep"], "yes")
      let status = try await metadata.customStatus()
      expectNoDifference(status, .ready)
      await expectCancelledBeforeItStarts { try await metadata.refreshCustom() }
      await expectCancelledBeforeItStarts { try await metadata.updateCustom(["Cancelled": "yes"]) }
      try await viewer.close(document)
      await #expect(throws: PDFError.self) { _ = try await metadata.customSnapshot() }
    }

    @Test func pageSelectionUsesStableRefsAcrossReorder() async throws {
      let (viewer, document, edit) = try await PageEditTests().opened()
      let page = try await viewer.pages(of: document)[0].page
      try await edit.select([page])
      _ = try await edit.movePages([page], to: .end)
      let selected = try await edit.selectedPages()
      expectNoDifference(selected, [page])
      #expect(try await edit.canExtract())
    }

    @Test func formValidationAndPermissionReadsAreExposed() async throws {
      let (_, _, form) = try await FormTests().opened()
      let validation = try await form.validate()
      expectNoDifference(validation.valid, validation.missing.isEmpty)
      #expect(try await form.canRestoreAttribution())
      #expect(try await form.canWriteScripts())
    }

    @Test func stageDestinationAndZoomReadsAreExposed() async throws {
      let viewer = try PDFViewer(ViewerConfig(stage: StageConfig()))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      let stage = try await viewer.stage(for: document)
      try await stage.setViewportSize(Size(width: 800, height: 600))
      let count = try await stage.pageCount()
      expectNoDifference(count, document.pageCount)
      _ = try await stage.canZoomIn()
      _ = try await stage.canZoomOut()
      if let destination = try await stage.currentDestination() {
        try await stage.goToDestination(destination)
      }
      try await viewer.destroy()
    }

    @Test func deniedScopeDoesNotAdvertiseNewPermissions() async throws {
      let viewer = try PDFViewer(ViewerConfig(
        scope: ["doc.open"], interaction: true, link: true, annotation: "",
        form: FormConfig(), redaction: RedactionConfig(), stamp: StampConfig(),
        pageEdit: true, signature: SignatureConfig()))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      #expect(try await !viewer.pageEdit(for: document).canExtract())
      #expect(try await !viewer.form(for: document).canRestoreAttribution())
      #expect(try await !viewer.form(for: document).canWriteScripts())
      #expect(try await !viewer.signatures(for: document).canReadRevision())
      #expect(try await !viewer.stamps().canCreateFromAnnotations(in: document))
      let interaction = try await viewer.interaction(for: document)
      #expect(try await !interaction.hasCursorClaim())
      let page = try await viewer.pages(of: document)[0].page
      _ = try await viewer.link(for: document).status(for: page)
      let redaction = try await viewer.redaction(for: document)
      let ref = AnnotationRef.objectNumber(page: page, annotObjectNumber: 999999)
      #expect(try await !redaction.canUnmark(ref))
      #expect(try await !redaction.canUpdateLabel(ref))
      try await viewer.destroy()
    }
  }
}
