import CustomDump
import EmbedPDF
import Foundation
import Testing

extension BaseSuite {
  @Suite struct SignatureTests {
    func opened(_ fixture: String, signer: Bool = true) async throws
      -> (PDFViewer, PDFDocument, SignatureCapability)
    {
      let viewer = try PDFViewer(
        ViewerConfig(interaction: true, annotation: AnnotationConfig(), form: FormConfig(), signature: SignatureConfig()))
      try await viewer.start()
      if signer { viewer.setSigner(try KeychainSignerTests.testSigner()) }
      let document = try await viewer.open(Fixture.data(fixture))
      let signatures = try await viewer.signatures(for: document)
      _ = try await signatures.refresh()
      return (viewer, document, signatures)
    }

    func unsignedField(_ signatures: SignatureCapability) async throws -> FormFieldRef {
      let fields = try await signatures.unsignedFields()
      return try #require(fields.first).field
    }

    @Test func twoApprovalsAreListed() async throws {
      let (_, _, signatures) = try await opened("signed_two_approvals")
      let got1 = try await signatures.signatures().count
      expectNoDifference(got1, 2)
      let protection = try await signatures.protection()
      expectNoDifference(protection?.judged, .annotate)
      let snapshot = try #require(try await signatures.snapshot())
      expectNoDifference(snapshot.revisions.count, 3)
    }

    @Test func aCertificationIsNamed() async throws {
      let (_, _, signatures) = try await opened("signed_certified")
      let protection = try await signatures.protection()
      expectNoDifference(protection?.certification?.permission, 2)
    }

    @Test func aFieldSignsAndValidatesUntrusted() async throws {
      let (_, _, signatures) = try await opened("unsigned_sigfield")
      let field = try await unsignedField(signatures)
      let done = try await signatures.sign(SignatureSignInput(field: field))
      expectNoDifference(done.signature.signed, true)
      expectNoDifference(done.status, .completed)
      let verdict = try await signatures.validate(field)
      expectNoDifference(verdict.integrity, .valid)
      expectNoDifference(verdict.summary, .validUntrusted)
    }

    @Test func withoutAnchorsASignatureIsValidUntrusted() async throws {
      let (_, _, signatures) = try await opened("signed_two_approvals")
      let verdicts = try await signatures.validate()
      #expect(!verdicts.isEmpty)
      #expect(verdicts.allSatisfy { $0.summary != .valid })
    }

    @Test func theTestCertificateAsAnAnchorIsTrusted() async throws {
      let (viewer, _, signatures) = try await opened("unsigned_sigfield")
      viewer.setTrust(try TrustAnchors(pem: [Fixture.text("TEST-ONLY-signer", ext: "pem")]))
      let field = try await unsignedField(signatures)
      _ = try await signatures.sign(SignatureSignInput(field: field))
      let verdict = try await signatures.validate(field)
      expectNoDifference(verdict.trust, .trusted)
      expectNoDifference(verdict.summary, .valid)
    }

    @Test func aCancelledSignerLeavesNothingPending() async throws {
      let (viewer, _, signatures) = try await opened("unsigned_sigfield", signer: false)
      viewer.setSigner(CancellingSigner())
      let field = try await unsignedField(signatures)
      await #expect {
        _ = try await signatures.sign(SignatureSignInput(field: field))
      } throws: { error in
        (error as? PDFError)?.code == "operation-cancelled"
      }
      let got2 = try await signatures.pending()
      expectNoDifference(got2, nil)
      let got3 = try await signatures.isBusy()
      expectNoDifference(got3, false)
    }

    @Test func aVisualFillNeedsNoSigner() async throws {
      let (_, _, signatures) = try await opened("unsigned_sigfield", signer: false)
      let got4 = try await signatures.mode()
      expectNoDifference(got4, .visual)
      let got5 = try await signatures.canSign()
      expectNoDifference(got5, false)
      let field = try await unsignedField(signatures)
      try await signatures.fill(field, with: .source(bytes: Fixture.data("signature_artwork")))
      try await signatures.clearField(field)
    }

    @Test func settingASignerTurnsSigningOn() async throws {
      let (viewer, _, signatures) = try await opened("unsigned_sigfield", signer: false)
      let got6 = try await signatures.canSign()
      expectNoDifference(got6, false)
      viewer.setSigner(try KeychainSignerTests.testSigner())
      let got7 = try await signatures.canSign()
      expectNoDifference(got7, true)
      let got8 = try await signatures.mode()
      expectNoDifference(got8, .sign)
    }

    @Test func aFreeMarkIsAStamp() async throws {
      let (viewer, document, signatures) = try await opened("unsigned_sigfield", signer: false)
      let page = try await viewer.pages(of: document)[0].page
      let result = try await signatures.place(
        .source(bytes: Fixture.data("signature_artwork")),
        at: .free(page: page, at: Point(x: 50, y: 50), targetWidth: nil, rotation: nil))
      guard case .placed = result else {
        Issue.record("expected a placed annotation, got \(result)")
        return
      }
    }

    @Test func aTwoPhaseSigningParksThenAborts() async throws {
      let (_, _, signatures) = try await opened("unsigned_sigfield", signer: false)
      let field = try await unsignedField(signatures)
      let prepared = try await signatures.prepareSignature(SignaturePrepareInput(field: field))
      expectNoDifference(prepared.digest.count, 32)
      let got9 = try await signatures.pending()?.signingId
      expectNoDifference(got9, prepared.signingId)
      try await signatures.abortPending()
      let got10 = try await signatures.pending()
      expectNoDifference(got10, nil)
    }

    @Test func aChangeAfterTheSignatureIsAnalyzed() async throws {
      let (_, _, signatures) = try await opened("signed_two_approvals")
      let analysis = try await signatures.analyzeChanges(since: 0, until: .persisted)
      expectNoDifference(analysis.laterRevisions, 1)
      expectNoDifference(analysis.verdict, .permitted)
      let revision = try await signatures.readRevision(try #require(try await signatures.signatures().first).field)
      expectNoDifference(revision.prefix(5), Data("%PDF-".utf8))
    }

    @Test func settingTheTargetIsObserved() async throws {
      let (_, _, signatures) = try await opened("unsigned_sigfield")
      let field = try await unsignedField(signatures)
      let stream = signatures.events
      try await signatures.setTarget(field)
      let got11 = try await signatures.target()
      expectNoDifference(got11, field)
      for await event in stream {
        if case .targetChanged(let changed) = event {
          expectNoDifference(changed, field)
          break
        }
      }
    }

    @Test func aClosedDocumentsCapabilityThrows() async throws {
      let (viewer, document, signatures) = try await opened("signed_two_approvals")
      try await viewer.close(document)
      await #expect {
        _ = try await signatures.snapshot()
      } throws: { error in
        let error = error as? PDFError
        return error?.code == "instance-closed" && error?.name == "PluginError"
      }
    }

    @Test func droppingTheStreamUnsubscribes() async throws {
      let (_, _, signatures) = try await opened("signed_two_approvals")
      let task = Task { for await _ in signatures.events {} }
      try await Task.sleep(for: .milliseconds(20))
      task.cancel()
      _ = await task.value
      _ = try await signatures.refresh()
    }

    /// Start `body` in a task, cancel it at once, and expect either a result
    /// or `operation-cancelled`; the capability stays usable.
    func cancels<T: Sendable>(
      _ signatures: SignatureCapability, _ body: @escaping @Sendable () async throws -> T
    ) async throws {
      let task = Task { try await body() }
      task.cancel()
      do { _ = try await task.value } catch { expectNoDifference((error as? PDFError)?.code, "operation-cancelled") }
      _ = try await signatures.status()
    }

    @Test func refreshCancelsWhenTheTaskIsCancelled() async throws {
      let (_, _, signatures) = try await opened("signed_two_approvals")
      try await cancels(signatures) { try await signatures.refresh() }
    }

    @Test func validateCancelsWhenTheTaskIsCancelled() async throws {
      let (_, _, signatures) = try await opened("signed_two_approvals")
      try await cancels(signatures) { try await signatures.validate() }
    }

    @Test func validateFieldCancelsWhenTheTaskIsCancelled() async throws {
      let (_, _, signatures) = try await opened("signed_two_approvals")
      let field = try #require(try await signatures.signatures().first).field
      try await cancels(signatures) { try await signatures.validate(field) }
    }

    @Test func analyzeChangesCancelsWhenTheTaskIsCancelled() async throws {
      let (_, _, signatures) = try await opened("signed_two_approvals")
      try await cancels(signatures) { try await signatures.analyzeChanges(since: 0) }
    }

    @Test func readRevisionCancelsWhenTheTaskIsCancelled() async throws {
      let (_, _, signatures) = try await opened("signed_two_approvals")
      let field = try #require(try await signatures.signatures().first).field
      try await cancels(signatures) { try await signatures.readRevision(field) }
    }

    @Test func signCancelsWhenTheTaskIsCancelled() async throws {
      let (_, _, signatures) = try await opened("unsigned_sigfield")
      let field = try await unsignedField(signatures)
      try await cancels(signatures) { try await signatures.sign(SignatureSignInput(field: field)) }
    }

    @Test func prepareSignatureCancelsWhenTheTaskIsCancelled() async throws {
      let (_, _, signatures) = try await opened("unsigned_sigfield")
      let field = try await unsignedField(signatures)
      try await cancels(signatures) { try await signatures.prepareSignature(SignaturePrepareInput(field: field)) }
    }

    @Test func completeSignatureCancelsWhenTheTaskIsCancelled() async throws {
      let (_, _, signatures) = try await opened("unsigned_sigfield")
      try await cancels(signatures) { try? await signatures.completeSignature("none", cms: Data([0x30, 0])) }
    }

    @Test func abortPendingCancelsWhenTheTaskIsCancelled() async throws {
      let (_, _, signatures) = try await opened("unsigned_sigfield")
      try await cancels(signatures) { try await signatures.abortPending() }
    }

    @Test func fillCancelsWhenTheTaskIsCancelled() async throws {
      let (_, _, signatures) = try await opened("unsigned_sigfield", signer: false)
      let field = try await unsignedField(signatures)
      let artwork = try Fixture.data("signature_artwork")
      try await cancels(signatures) { try await signatures.fill(field, with: .source(bytes: artwork)) }
    }

    @Test func clearFieldCancelsWhenTheTaskIsCancelled() async throws {
      let (_, _, signatures) = try await opened("unsigned_sigfield", signer: false)
      let field = try await unsignedField(signatures)
      try await cancels(signatures) { try await signatures.clearField(field) }
    }

    @Test func placeCancelsWhenTheTaskIsCancelled() async throws {
      let (viewer, document, signatures) = try await opened("unsigned_sigfield", signer: false)
      let page = try await viewer.pages(of: document)[0].page
      let artwork = try Fixture.data("signature_artwork")
      try await cancels(signatures) {
        try await signatures.place(
          .source(bytes: artwork), at: .free(page: page, at: Point(x: 50, y: 50), targetWidth: nil, rotation: nil))
      }
    }
  }
}

/// A signer whose prompt the user cancelled.
final class CancellingSigner: SignatureSigner, Sendable {
  func certificateChain() throws -> [Data] {
    [try BaseSuite.KeychainSignerTests.testSigner().certificateChain()].flatMap { $0 }
  }
  func algorithm() -> SignatureKeyAlgorithm { .rsaPkcs1 }
  func digest() -> SignatureDigest { .sha256 }
  func sign(message: Data) throws -> Data {
    throw ViewerError.Kernel(name: "UserCancelled", code: "operation-cancelled", detail: "the user cancelled")
  }
}
