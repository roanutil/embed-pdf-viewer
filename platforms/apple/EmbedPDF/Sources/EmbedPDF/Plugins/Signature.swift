public import EmbedPDFBindings
public import Foundation

/// Configuration for signature placement mode and certification availability.
public typealias SignatureConfig = EmbedPDFBindings.SignatureConfig
/// Whether placing a mark signs, visually fills or requests a host signing flow.
public typealias SignatureMode = EmbedPDFBindings.SignatureMode
/// The document's signature fields, revisions and protection metadata.
public typealias SignatureSnapshot = EmbedPDFBindings.SignatureSnapshot
/// Metadata describing a saved revision covered by a signature.
public typealias SignatureRevision = EmbedPDFBindings.SignatureRevision
/// A signature field's identity, signed state, widgets and signature metadata.
public typealias SignatureInfo = EmbedPDFBindings.SignatureInfo
/// The annotation and page-space geometry of a signature field widget.
public typealias SignatureWidget = EmbedPDFBindings.SignatureWidget
/// The kind of PDF signature represented by a signed field.
public typealias SignatureKind = EmbedPDFBindings.SignatureKind
/// The extent of the document bytes covered by a signature.
public typealias SignatureCoverage = EmbedPDFBindings.SignatureCoverage
/// Signer identity and attribution claimed by signature metadata.
public typealias SignatureSignerClaims = EmbedPDFBindings.SignatureSignerClaims
/// Constraints a PDF signature field requests for signing.
public typealias SignatureSeedValue = EmbedPDFBindings.SignatureSeedValue
/// The form fields to lock when a signature is applied.
public typealias SignatureFieldLock = EmbedPDFBindings.SignatureFieldLock
/// Whether a signature lock targets all, included or excluded fields.
public typealias SignatureFieldLockAction = EmbedPDFBindings.SignatureFieldLockAction
/// The document changes permitted by a certification signature.
public typealias SignatureModificationLevel = EmbedPDFBindings.SignatureModificationLevel
/// The effective certification and field-lock protection on the document.
public typealias DocumentProtection = EmbedPDFBindings.DocumentProtection
/// The certification signature and its allowed modification level.
public typealias DocumentCertification = EmbedPDFBindings.DocumentCertification
/// An effective form field lock and the signature that imposed it.
public typealias DocumentFieldLock = EmbedPDFBindings.DocumentFieldLock
/// The signature or lock rule responsible for locking a field.
public typealias DocumentFieldLockSource = EmbedPDFBindings.DocumentFieldLockSource
/// The signing identifier and field of a prepared external signing operation.
public typealias SignaturePending = EmbedPDFBindings.SignaturePending
/// The integrity, cryptography, trust and modification assessment of a signature.
public typealias SignatureVerdict = EmbedPDFBindings.SignatureVerdict
/// The result of checking the signed byte ranges and digest.
public typealias SignatureIntegrity = EmbedPDFBindings.SignatureIntegrity
/// The result of verifying a signature's cryptographic value.
public typealias SignatureCryptography = EmbedPDFBindings.SignatureCryptography
/// The certificate chain's trust assessment against supplied anchors.
public typealias SignatureTrustStatus = EmbedPDFBindings.SignatureTrustStatus
/// The time basis used to validate a certificate chain.
public typealias SignatureValidationTime = EmbedPDFBindings.SignatureValidationTime
/// The assessment of changes made after a signed revision.
public typealias SignatureModifications = EmbedPDFBindings.SignatureModifications
/// Whether later document changes comply with a signature's restrictions.
public typealias SignatureModificationsVerdict = EmbedPDFBindings.SignatureModificationsVerdict
/// Whether change analysis ends at saved bytes or the current working copy.
public typealias SignatureModificationsBasis = EmbedPDFBindings.SignatureModificationsBasis
/// The combined validation outcome presented for a signature.
public typealias SignatureSummary = EmbedPDFBindings.SignatureSummary
/// Options controlling signature validation and its time basis.
public typealias SignatureValidateRequest = EmbedPDFBindings.SignatureValidateRequest
/// The findings from comparing a signed revision with a later document state.
public typealias SignatureChangeAnalysis = EmbedPDFBindings.SignatureChangeAnalysis
/// One detected document change and its compliance assessment.
public typealias SignatureChangeFinding = EmbedPDFBindings.SignatureChangeFinding
/// The compliance assessment assigned to a change finding.
public typealias SignatureFindingVerdict = EmbedPDFBindings.SignatureFindingVerdict
/// A stamp asset or supplied source used as a visible signing mark.
public typealias SignatureMark = EmbedPDFBindings.SignatureMark
/// The signer name, reason, location and other facts supplied when signing.
public typealias SignatureSignerInput = EmbedPDFBindings.SignatureSignerInput
/// The field, optional appearance and protection settings for signing.
public typealias SignatureSignInput = EmbedPDFBindings.SignatureSignInput
/// The field and digest, appearance and protection options for external signing.
public typealias SignaturePrepareInput = EmbedPDFBindings.SignaturePrepareInput
/// The signing identifier and data needed to complete an external signature.
public typealias SignaturePrepared = EmbedPDFBindings.SignaturePrepared
/// The reference identifying a version involved in signature completion.
public typealias SignatureVersionRef = EmbedPDFBindings.SignatureVersionRef
/// The document version from which a signing operation was prepared.
public typealias SignatureBaseVersion = EmbedPDFBindings.SignatureBaseVersion
/// The committed version and completion status of a signing operation.
public typealias SignatureCompletion = EmbedPDFBindings.SignatureCompletion
/// The status of completing a prepared signature.
public typealias SignatureCompletionStatus = EmbedPDFBindings.SignatureCompletionStatus
/// A signature field or a free page-space stamp placement.
public typealias SignaturePlaceTarget = EmbedPDFBindings.SignaturePlaceTarget
/// Whether placing a mark signed, filled, requested signing or created a stamp.
public typealias SignaturePlaceResult = EmbedPDFBindings.SignaturePlaceResult
/// A signing, validation, protection, target or inspection event.
public typealias SignatureEvent = EmbedPDFBindings.SignatureEvent
/// A host signing provider that supplies certificates, algorithm and asynchronous signing.
///
/// Members are invoked on the kernel thread. Return a leaf-first DER certificate chain.
/// `sign(message:completion:)` receives DER-encoded CMS signed attributes: hash those
/// message bytes using `digest()` before signing. Return immediately and complete from
/// any thread. RSA results are raw signature bytes; ECDSA results are fixed-width `r || s`.
public typealias SignatureSigner = EmbedPDFBindings.SignatureSigner
/// The completion callback a signer must finish or fail after producing a signature.
///
/// The first response wins. Dropping an unanswered completion fails the signing with
/// `operation-failed`; signing leaves no pending operation after success or failure.
public typealias SignatureSignCompletion = EmbedPDFBindings.SignatureSignCompletion
/// A host provider of DER-encoded trust-anchor certificates for validation.
///
/// Return an empty array when no anchors are trusted; validation then cannot exceed
/// `.validUntrusted`. Errors from `anchors()` propagate to validation.
public typealias SignatureTrust = EmbedPDFBindings.SignatureTrust
/// The RSA or ECDSA signature algorithm supplied by a signer.
public typealias SignatureKeyAlgorithm = EmbedPDFBindings.SignatureKeyAlgorithm
/// The digest algorithm used while preparing and signing signature data.
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

  /// Returns the signature snapshot's loading state.
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

  /// Returns signature information for `field`, or `nil` when it is absent from the snapshot.
  public func signature(_ field: FormFieldRef) async throws(PDFError) -> SignatureInfo? {
    try await bridged { try await handle.signature(field: field) }
  }

  /// The last verdicts, `nil` before the first validation.
  public func verdicts() async throws(PDFError) -> [SignatureVerdict]? {
    try await bridged { try await handle.verdicts() }
  }

  /// Returns the last validation verdict for `field`, or `nil` when none is available.
  public func verdict(_ field: FormFieldRef) async throws(PDFError) -> SignatureVerdict? {
    try await bridged { try await handle.verdict(field: field) }
  }

  /// Returns the current certification and field-lock protection, or `nil` before it is available.
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

  /// Prepares an external signing operation and returns its digest and signing identifier.
  ///
  /// The document becomes read-only while this operation is pending. Complete
  /// it with `completeSignature(_:cms:)` or release it with `cancelPending()`.
  public func prepareSignature(_ input: SignaturePrepareInput) async throws(PDFError) -> SignaturePrepared {
    try await bridgedCancellable { token in try await handle.prepareSignature(input: input, cancel: token) }
  }

  /// Completes a prepared external signing operation with a detached CMS signature.
  ///
  /// - Parameters:
  ///   - signingId: The identifier returned by `prepareSignature(_:)`.
  ///   - cms: The detached CMS signature for the prepared digest.
  /// - Returns: The completion status and committed document version.
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
  /// Returns the signature capability bound to an open document.
  ///
  /// The viewer must have the `signature` plugin enabled. The capability retains the viewer
  /// and becomes invalid when the document closes.
  ///
  /// - Throws: `PDFError` if the plugin or document scope is unavailable.
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
