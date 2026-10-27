import CustomDump
import EmbedPDF
import Testing

extension BaseSuite {
  @Suite struct SecurityTests {
    @Test func aLockedDocumentUnlocks() async throws {
      let viewer = try PDFViewer(ViewerConfig())
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("encrypted-hello"))
      expectNoDifference(document.status, .locked)
      await #expect(throws: PDFError.self) { try await viewer.unlock(document, password: "nope") }
      try await viewer.unlock(document, password: "hôtel")
      let info = try #require(try await viewer.documents().first { $0.id == document.id })
      expectNoDifference(info.status, .ready)
      #expect(info.pageCount > 0)
    }

    @Test func aNarrowScopeCannotSign() async throws {
      // The field's ref, read where the session may read the form: refs
      // are object numbers, the same in every viewer of the file.
      let full = try PDFViewer(ViewerConfig(interaction: true, form: FormConfig(), signature: SignatureConfig()))
      try await full.start()
      let readable = try await full.signatures(for: try await full.open(Fixture.data("unsigned_sigfield")))
      _ = try await readable.refresh()
      let field = try #require(try await readable.unsignedFields().first).field
      #expect(try await readable.canSign(field))
      let viewer = try PDFViewer(
        ViewerConfig(
          scope: ["doc.open", "doc.render"], interaction: true, form: FormConfig(), signature: SignatureConfig()))
      try await viewer.start()
      viewer.setSigner(try KeychainSignerTests.testSigner())
      let document = try await viewer.open(Fixture.data("unsigned_sigfield"))
      let signatures = try await viewer.signatures(for: document)
      let got1 = try await signatures.canSign(field)
      expectNoDifference(got1, false)
      let got2 = try await signatures.canFill(field)
      expectNoDifference(got2, false)
    }

    @Test func aNarrowScopeCannotSave() async throws {
      let viewer = try PDFViewer(ViewerConfig(scope: ["doc.open", "doc.render"]))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      await #expect {
        _ = try await viewer.save(document)
      } throws: { error in
        (error as? PDFError)?.code == "permission-denied"
      }
    }

    @Test func aNarrowScopeCannotEditMetadata() async throws {
      let viewer = try PDFViewer(ViewerConfig(scope: ["doc.open", "doc.render"], metadata: true))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      let metadata = try await viewer.metadata(for: document)
      let got3 = try await metadata.canUpdate()
      expectNoDifference(got3, false)
    }
  }
}
