public import EmbedPDFBindings
public import Foundation

public typealias SignatureConfig = EmbedPDFBindings.SignatureConfig
public typealias SignatureMode = EmbedPDFBindings.SignatureMode
public typealias SignatureSnapshot = EmbedPDFBindings.SignatureSnapshot
public typealias SignatureRevision = EmbedPDFBindings.SignatureRevision
public typealias SignatureInfo = EmbedPDFBindings.SignatureInfo
public typealias SignatureWidget = EmbedPDFBindings.SignatureWidget
public typealias SignatureKind = EmbedPDFBindings.SignatureKind
public typealias SignatureCoverage = EmbedPDFBindings.SignatureCoverage
public typealias SignatureSignerClaims = EmbedPDFBindings.SignatureSignerClaims
public typealias SignatureSeedValue = EmbedPDFBindings.SignatureSeedValue
public typealias SignatureFieldLock = EmbedPDFBindings.SignatureFieldLock
public typealias SignatureFieldLockAction = EmbedPDFBindings.SignatureFieldLockAction
public typealias SignatureModificationLevel = EmbedPDFBindings.SignatureModificationLevel
public typealias DocumentProtection = EmbedPDFBindings.DocumentProtection
public typealias DocumentCertification = EmbedPDFBindings.DocumentCertification
public typealias DocumentFieldLock = EmbedPDFBindings.DocumentFieldLock
public typealias DocumentFieldLockSource = EmbedPDFBindings.DocumentFieldLockSource
public typealias SignaturePending = EmbedPDFBindings.SignaturePending
public typealias SignatureVerdict = EmbedPDFBindings.SignatureVerdict
public typealias SignatureIntegrity = EmbedPDFBindings.SignatureIntegrity
public typealias SignatureCryptography = EmbedPDFBindings.SignatureCryptography
public typealias SignatureTrustStatus = EmbedPDFBindings.SignatureTrustStatus
public typealias SignatureValidationTime = EmbedPDFBindings.SignatureValidationTime
public typealias SignatureModifications = EmbedPDFBindings.SignatureModifications
public typealias SignatureModificationsVerdict = EmbedPDFBindings.SignatureModificationsVerdict
public typealias SignatureModificationsBasis = EmbedPDFBindings.SignatureModificationsBasis
public typealias SignatureSummary = EmbedPDFBindings.SignatureSummary
public typealias SignatureValidateRequest = EmbedPDFBindings.SignatureValidateRequest
public typealias SignatureChangeAnalysis = EmbedPDFBindings.SignatureChangeAnalysis
public typealias SignatureChangeFinding = EmbedPDFBindings.SignatureChangeFinding
public typealias SignatureFindingVerdict = EmbedPDFBindings.SignatureFindingVerdict
public typealias SignatureMark = EmbedPDFBindings.SignatureMark
public typealias SignatureSignerInput = EmbedPDFBindings.SignatureSignerInput
public typealias SignatureSignInput = EmbedPDFBindings.SignatureSignInput
public typealias SignaturePrepareInput = EmbedPDFBindings.SignaturePrepareInput
public typealias SignaturePrepared = EmbedPDFBindings.SignaturePrepared
public typealias SignatureVersionRef = EmbedPDFBindings.SignatureVersionRef
public typealias SignatureBaseVersion = EmbedPDFBindings.SignatureBaseVersion
public typealias SignatureCompletion = EmbedPDFBindings.SignatureCompletion
public typealias SignatureCompletionStatus = EmbedPDFBindings.SignatureCompletionStatus
public typealias SignaturePlaceTarget = EmbedPDFBindings.SignaturePlaceTarget
public typealias SignaturePlaceResult = EmbedPDFBindings.SignaturePlaceResult
public typealias SignatureEvent = EmbedPDFBindings.SignatureEvent
public typealias SignatureSigner = EmbedPDFBindings.SignatureSigner
public typealias SignatureSignCompletion = EmbedPDFBindings.SignatureSignCompletion
public typealias SignatureTrust = EmbedPDFBindings.SignatureTrust
public typealias SignatureKeyAlgorithm = EmbedPDFBindings.SignatureKeyAlgorithm
public typealias SignatureDigest = EmbedPDFBindings.SignatureDigest
/// What a `SignatureSigner`'s synchronous members or a `SignatureTrust`
/// throws: `.Kernel(name:code:detail:)` with a kernel code.
public typealias ViewerError = EmbedPDFBindings.ViewerError

extension SignatureSignCompletion {
  /// Fail the signing with `error`, keeping its code (see
  /// `HostFailure.init(_:)`): a cancelled prompt's `operation-cancelled`
  /// is what `sign` throws.
  public func fail(_ error: any Error) {
    failWith(failure: HostFailure(error))
  }
}

/// A document's signature fields: reading, validating, signing and filling
/// them. Get one with `PDFViewer.signatures(for:)`; the viewer needs
/// `ViewerConfig(interaction: true, form: FormConfig(), signature: SignatureConfig())`.
/// Signing uses the signer from `PDFViewer.setSigner(_:)` unless a call names
/// one. It keeps the viewer alive. After its document closes, every call
/// throws `instance-closed`.
public struct SignatureCapability: Sendable {
  let handle: SignatureHandle
  let viewer: PDFViewer

  /// The last snapshot read, `nil` before the first.
  public func snapshot() async throws(PDFError) -> SignatureSnapshot? {
    try await bridged { try await handle.snapshot() }
  }

  public func status() async throws(PDFError) -> ResourceStatus {
    try await bridged { try await handle.status() }
  }

  /// The signed fields.
  public func signatures() async throws(PDFError) -> [SignatureInfo] {
    try await bridged { try await handle.signatures() }
  }

  /// The signature fields nobody has signed.
  public func unsignedFields() async throws(PDFError) -> [SignatureInfo] {
    try await bridged { try await handle.unsignedFields() }
  }

  public func signature(_ field: FormFieldRef) async throws(PDFError) -> SignatureInfo? {
    try await bridged { try await handle.signature(field: field) }
  }

  /// The last verdicts, `nil` before the first validation.
  public func verdicts() async throws(PDFError) -> [SignatureVerdict]? {
    try await bridged { try await handle.verdicts() }
  }

  public func verdict(_ field: FormFieldRef) async throws(PDFError) -> SignatureVerdict? {
    try await bridged { try await handle.verdict(field: field) }
  }

  public func protection() async throws(PDFError) -> DocumentProtection? {
    try await bridged { try await handle.protection() }
  }

  /// A parked two-phase signing: the document is read-only until it
  /// completes or is aborted.
  public func pending() async throws(PDFError) -> SignaturePending? {
    try await bridged { try await handle.pending() }
  }

  /// A sign or fill is in flight.
  public func isBusy() async throws(PDFError) -> Bool {
    try await bridged { try await handle.isBusy() }
  }

  /// What placing a mark on a field does: `.sign` while a signer is set,
  /// else `.visual`, unless the config names a mode.
  public func mode() async throws(PDFError) -> SignatureMode {
    try await bridged { try await handle.mode() }
  }

  /// The field the next picked mark goes to.
  public func target() async throws(PDFError) -> FormFieldRef? {
    try await bridged { try await handle.target() }
  }

  /// Whether signing `field` is allowed: `doc.sign`, or `fields:sign` for
  /// its group. `false` for a field the form doesn't have.
  public func canSign(_ field: FormFieldRef) async throws(PDFError) -> Bool {
    try await bridged { try await handle.canSign(field: field) }
  }

  /// Whether drawing a mark into `field` is allowed: `doc.forms.fill`, or
  /// `fields:fill` for its group.
  public func canFill(_ field: FormFieldRef) async throws(PDFError) -> Bool {
    try await bridged { try await handle.canFill(field: field) }
  }

  /// The config allows certifying and `doc.sign.certify` is granted.
  public func canCertify() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canCertify() }
  }

  /// Re-read the snapshot. `nil` when the engine has no signatures.
  public func refresh() async throws(PDFError) -> SignatureSnapshot? {
    try await bridgedCancellable { token in try await handle.refresh(cancel: token) }
  }

  /// Judge every signature. Without trust anchors a good one tops out at
  /// `.validUntrusted`.
  public func validate(_ request: SignatureValidateRequest = SignatureValidateRequest()) async throws(PDFError)
    -> [SignatureVerdict]
  {
    try await bridgedCancellable { token in try await handle.validate(request: request, cancel: token) }
  }

  /// Judge one signature.
  public func validate(_ field: FormFieldRef, _ request: SignatureValidateRequest = SignatureValidateRequest())
    async throws(PDFError) -> SignatureVerdict
  {
    try await bridgedCancellable { token in
      try await handle.validateField(field: field, request: request, cancel: token)
    }
  }

  /// What changed after signature `since` (its index), up to the saved
  /// bytes or the working copy.
  public func analyzeChanges(since: UInt32, until: SignatureModificationsBasis = .workingCopy)
    async throws(PDFError) -> SignatureChangeAnalysis
  {
    try await bridgedCancellable { token in
      try await handle.analyzeChanges(since: since, until: until, cancel: token)
    }
  }

  /// The revision the signature on `field` seals: a complete PDF of its own.
  public func readRevision(_ field: FormFieldRef) async throws(PDFError) -> Data {
    try await bridgedCancellable { token in try await handle.readRevision(field: field, cancel: token) }
  }

  /// Name the field the next picked mark goes to; `nil` clears.
  public func setTarget(_ field: FormFieldRef?) async throws(PDFError) {
    try await bridged { try await handle.setTarget(field: field) }
  }

  /// Ask the app to show a signed field's facts (`.inspectionRequested`).
  public func requestInspection(_ field: FormFieldRef) async throws(PDFError) {
    try await bridged { try await handle.requestInspection(field: field) }
  }

  /// Seal `input.field` with `signer`, or the viewer's. Fails with the
  /// signer's own error when it throws, and leaves nothing pending.
  public func sign(_ input: SignatureSignInput, with signer: (any SignatureSigner)? = nil) async throws(PDFError)
    -> SignatureCompletion
  {
    try await bridgedCancellable { token in try await handle.sign(input: input, signer: signer, cancel: token) }
  }

  /// Phase one of an external signing: the digest to sign elsewhere.
  public func prepareSignature(_ input: SignaturePrepareInput) async throws(PDFError) -> SignaturePrepared {
    try await bridgedCancellable { token in try await handle.prepareSignature(input: input, cancel: token) }
  }

  /// Phase two: seal the parked signing with a detached CMS.
  public func completeSignature(_ signingId: String, cms: Data) async throws(PDFError) -> SignatureCompletion {
    try await bridgedCancellable { token in
      try await handle.completeSignature(signingId: signingId, cms: cms, cancel: token)
    }
  }

  /// Give up the parked signing, if any.
  public func cancelPending() async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.cancelPending(cancel: token) }
  }

  /// Draw `mark` into an unsigned field without sealing it.
  public func fill(_ field: FormFieldRef, with mark: SignatureMark) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.fillField(field: field, mark: mark, cancel: token) }
  }

  /// Undo a visual fill.
  public func clearField(_ field: FormFieldRef) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.clearField(field: field, cancel: token) }
  }

  /// Into a field by `mode`, or freely on a page as a stamp.
  public func place(_ mark: SignatureMark, at target: SignaturePlaceTarget) async throws(PDFError)
    -> SignaturePlaceResult
  {
    try await bridgedCancellable { token in try await handle.placeMark(mark: mark, target: target, cancel: token) }
  }

  /// Signed, filled, cleared, validated, protection changed, invalidating,
  /// target changed, sign requested and inspection requested. Finishes when
  /// the document closes or the viewer is destroyed.
  public var events: AsyncStream<SignatureEvent> {
    viewer.registry.stream { handle.subscribe(listener: $0) }
  }
}

extension EventRelay: SignatureEventListener where Event == SignatureEvent {
  func onEvent(event: SignatureEvent) {
    continuation.yield(event)
  }

  func onEnd() {
    continuation.finish()
  }
}

extension PDFViewer {
  public func signatures(for document: DocumentInfo) async throws(PDFError) -> SignatureCapability {
    let handle = try await bridged { try await host.signature(documentId: document.id) }
    return SignatureCapability(handle: handle, viewer: self)
  }

  /// The signer `sign` uses when a call names none, for every document.
  /// Takes effect at the next signing. `nil` clears it.
  public func setSigner(_ signer: (any SignatureSigner)?) {
    host.setSignatureSigner(signer: signer)
  }

  /// The anchors validation trusts. `nil` makes every verdict top out at
  /// `.validUntrusted`.
  public func setTrust(_ trust: (any SignatureTrust)?) {
    host.setSignatureTrust(trust: trust)
  }
}
