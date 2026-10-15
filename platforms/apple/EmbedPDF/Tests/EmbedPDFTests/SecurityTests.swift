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
      let viewer = try PDFViewer(
        ViewerConfig(
          scope: ["doc.open", "doc.render"], interaction: true, form: FormConfig(), signature: SignatureConfig()))
      try await viewer.start()
      viewer.setSigner(try KeychainSignerTests.testSigner())
      let document = try await viewer.open(Fixture.data("unsigned_sigfield"))
      let signatures = try await viewer.signatures(for: document)
      let got1 = try await signatures.canSign()
      expectNoDifference(got1, false)
      let got2 = try await signatures.canFill()
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
      let got3 = try await metadata.canEdit()
      expectNoDifference(got3, false)
    }
  }
}
