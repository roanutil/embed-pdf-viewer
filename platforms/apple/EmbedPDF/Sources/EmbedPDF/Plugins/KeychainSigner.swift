public import EmbedPDFBindings
public import Foundation
public import Security

/// Signs PDF signature data with a retained Security framework key and certificate chain.
///
/// Create one from a `SecIdentity` or a `SecKey` and leaf-first certificates,
/// then install it with `PDFViewer.setSigner(_:)`. Signing uses the supplied key
/// through Security framework; a Keychain or Secure Enclave key retains its own
/// storage and access-control policy. This type does not import the key into either store.
public final class KeychainSigner: SignatureSigner, @unchecked Sendable {
  let key: SecKey
  let chain: [Data]
  let keyAlgorithm: SignatureKeyAlgorithm
  let fieldSize: Int

  /// Creates a signer from an identity's private key and certificate chain.
  ///
  /// Throws `KeychainSignerError.unreadableIdentity` if the private key or leaf certificate cannot be read,
  /// or `KeychainSignerError.unreadableKey` if the key's attributes cannot be read.
  public convenience init(identity: SecIdentity) throws {
    var key: SecKey?
    var certificate: SecCertificate?
    guard SecIdentityCopyPrivateKey(identity, &key) == errSecSuccess, let key,
      SecIdentityCopyCertificate(identity, &certificate) == errSecSuccess, let certificate
    else { throw KeychainSignerError.unreadableIdentity }
    try self.init(privateKey: key, certificates: Self.chain(from: certificate))
  }

  /// Creates a signer that retains `privateKey` and copies the certificate chain as DER.
  ///
  /// - Parameters:
  ///   - privateKey: The key used by Security framework for each signing operation.
  ///   - certificates: The signer certificate first, followed by its issuing chain.
  /// - Throws: `KeychainSignerError.unreadableKey` if key attributes cannot be read.
  ///
  /// The initializer does not verify that the certificates match the key.
  public init(privateKey: SecKey, certificates: [SecCertificate]) throws {
    guard let attributes = SecKeyCopyAttributes(privateKey) as? [CFString: Any],
      let type = attributes[kSecAttrKeyType] as? String,
      let bits = attributes[kSecAttrKeySizeInBits] as? Int
    else { throw KeychainSignerError.unreadableKey }
    key = privateKey
    chain = certificates.map { SecCertificateCopyData($0) as Data }
    keyAlgorithm = type == (kSecAttrKeyTypeECSECPrimeRandom as String) ? .ecdsa : .rsaPkcs1
    fieldSize = (bits + 7) / 8
  }

  /// Returns the supplied certificate chain as DER bytes, leaf certificate first.
  public func certificateChain() throws -> [Data] { chain }
  /// Returns ECDSA for an EC key, or RSA PKCS#1 for other key types.
  public func algorithm() -> SignatureKeyAlgorithm { keyAlgorithm }
  /// Returns SHA-256, the digest used by this signer.
  public func digest() -> SignatureDigest { .sha256 }

  /// Called on the kernel thread, and returns at once: the signing, and any
  /// Face ID or passcode prompt a Keychain item's access control raises,
  /// runs on a global queue, so the viewer keeps serving other calls while
  /// the prompt is up. A cancelled prompt fails with `operation-cancelled`.
  public func sign(message: Data, completion: SignatureSignCompletion) {
    DispatchQueue.global(qos: .userInitiated).async { [self] in
      do {
        completion.finish(signature: try sign(message: message))
      } catch {
        completion.fail(error)
      }
    }
  }

  /// Sign `message` on the calling thread, which waits for any prompt the
  /// key raises. A cancelled prompt throws `operation-cancelled`.
  ///
  /// - Parameter message: The message bytes to hash with SHA-256 and sign.
  /// - Returns: An RSA PKCS#1 signature, or fixed-width ECDSA `r || s` bytes.
  /// - Throws: `ViewerError` with `operation-failed` when signing or ECDSA decoding fails.
  public func sign(message: Data) throws -> Data {
    let algorithm: SecKeyAlgorithm =
      keyAlgorithm == .ecdsa ? .ecdsaSignatureMessageX962SHA256 : .rsaSignatureMessagePKCS1v15SHA256
    var error: Unmanaged<CFError>?
    guard let signature = SecKeyCreateSignature(key, algorithm, message as CFData, &error) as Data? else {
      let cfError = error?.takeRetainedValue()
      let cancelled = (cfError.map { CFErrorGetCode($0) } ?? 0) == Int(errSecUserCanceled)
      throw ViewerError.Kernel(
        name: cancelled ? "UserCancelled" : "SigningError",
        code: cancelled ? "operation-cancelled" : "operation-failed",
        detail: cfError.map { "\($0)" } ?? "SecKeyCreateSignature failed")
    }
    guard keyAlgorithm == .ecdsa else { return signature }
    do { return try Self.rawECDSA(der: signature, fieldSize: fieldSize) } catch {
      throw ViewerError.Kernel(name: "SigningError", code: "operation-failed", detail: "\(error)")
    }
  }

  /// X9.62 `SEQUENCE { INTEGER r, INTEGER s }` to `r || s`, each left-padded
  /// to `fieldSize` bytes, which is what `epdf-signature-core` expects.
  static func rawECDSA(der: Data, fieldSize: Int) throws -> Data {
    var outer = DERReader(der, malformed: .badSignature)
    var sequence = try outer.take(0x30)
    let r = try sequence.take(0x02).bytes.drop { $0 == 0 }
    let s = try sequence.take(0x02).bytes.drop { $0 == 0 }
    guard r.count <= fieldSize, s.count <= fieldSize else { throw KeychainSignerError.badSignature }
    return Data(repeating: 0, count: fieldSize - r.count) + r + Data(repeating: 0, count: fieldSize - s.count) + s
  }

  static func chain(from leaf: SecCertificate) -> [SecCertificate] {
    var trust: SecTrust?
    guard SecTrustCreateWithCertificates(leaf, SecPolicyCreateBasicX509(), &trust) == errSecSuccess, let trust,
      let chain = SecTrustCopyCertificateChain(trust) as? [SecCertificate]
    else { return [leaf] }
    return chain
  }

  /// Reads an RSA private key from an unencrypted PKCS#8 PEM (`-----BEGIN PRIVATE KEY-----`).
  ///
  /// - Parameter pkcs8PEM: PEM text containing the PKCS#8 private key.
  /// - Returns: A Security framework key; this method does not save it to the Keychain.
  /// - Throws: `KeychainSignerError.unreadableKey` for malformed or unsupported key data.
  public static func privateKey(pkcs8PEM: String) throws -> SecKey {
    // SecKeyCreateWithData takes PKCS#1 for RSA: strip the PKCS#8 wrapper.
    let pkcs1 = try unwrapPKCS8(pemBody(pkcs8PEM))
    let attributes: [CFString: Any] = [kSecAttrKeyType: kSecAttrKeyTypeRSA, kSecAttrKeyClass: kSecAttrKeyClassPrivate]
    var error: Unmanaged<CFError>?
    guard let key = SecKeyCreateWithData(pkcs1 as CFData, attributes as CFDictionary, &error) else {
      throw KeychainSignerError.unreadableKey
    }
    return key
  }

  /// Reads a certificate from PEM text (`-----BEGIN CERTIFICATE-----`).
  ///
  /// - Parameter pem: PEM text containing one DER-encoded certificate.
  /// - Returns: The parsed Security framework certificate.
  /// - Throws: `KeychainSignerError.unreadableCertificate` for invalid base64 or certificate data.
  public static func certificate(pem: String) throws -> SecCertificate {
    guard let certificate = SecCertificateCreateWithData(nil, try pemBody(pem, else: .unreadableCertificate) as CFData)
    else {
      throw KeychainSignerError.unreadableCertificate
    }
    return certificate
  }

  /// The base64 body of a PEM; `failure` when it isn't base64.
  static func pemBody(_ pem: String, else failure: KeychainSignerError = .unreadableKey) throws -> Data {
    let body = pem.split(whereSeparator: \.isNewline).filter { !$0.hasPrefix("-----") }.joined()
    guard let data = Data(base64Encoded: body) else { throw failure }
    return data
  }

  /// `PrivateKeyInfo { version, algorithm, OCTET STRING privateKey }`.
  static func unwrapPKCS8(_ der: Data) throws -> Data {
    var outer = DERReader(der, malformed: .unreadableKey)
    var info = try outer.take(0x30)
    _ = try info.take(0x02)
    _ = try info.take(0x30)
    return Data(try info.take(0x04).bytes)
  }
}

/// Failures reading Security framework identities, keys, certificates or signature encodings.
public enum KeychainSignerError: Error, Sendable {
  /// The identity lacks a readable key or certificate (`unreadableIdentity`), key
  /// attributes or encoding cannot be read (`unreadableKey`), certificate data
  /// cannot be parsed (`unreadableCertificate`), or ECDSA DER is malformed (`badSignature`).
  case unreadableIdentity, unreadableKey, unreadableCertificate, badSignature
}

/// A DER TLV reader: definite lengths only, which is all a signature or a
/// key container uses.
struct DERReader {
  var bytes: ArraySlice<UInt8>
  /// What a malformed element throws: `badSignature` for a signature,
  /// `unreadableKey` for a key container.
  let malformed: KeychainSignerError

  init(_ data: Data, malformed: KeychainSignerError) {
    bytes = [UInt8](data)[...]
    self.malformed = malformed
  }

  init(_ slice: ArraySlice<UInt8>, malformed: KeychainSignerError) {
    bytes = slice
    self.malformed = malformed
  }

  /// The value of the next element, which must carry `tag`.
  mutating func take(_ tag: UInt8) throws(KeychainSignerError) -> DERReader {
    guard bytes.popFirst() == tag, let first = bytes.popFirst() else { throw malformed }
    var length = Int(first)
    if first & 0x80 != 0 {
      // 0x80 alone is BER's indefinite length, which DER forbids. Past four
      // length bytes (4 GiB) the shift below could overflow into a negative
      // length, and no key or signature is that long.
      let count = Int(first & 0x7f)
      guard (1...4).contains(count) else { throw malformed }
      length = 0
      for _ in 0..<count {
        guard let b = bytes.popFirst() else { throw malformed }
        length = length << 8 | Int(b)
      }
    }
    guard bytes.count >= length else { throw malformed }
    defer { bytes = bytes.dropFirst(length) }
    return DERReader(bytes.prefix(length), malformed: malformed)
  }
}

/// Trust anchors the app supplies, DER. iOS can't list the system's roots,
/// so anchors are the app's own (its organisation's CA, say).
public final class TrustAnchors: SignatureTrust, Sendable {
  /// The app-supplied trust-anchor certificates in DER format.
  public let certificates: [Data]

  /// Creates a trust provider from DER bytes without parsing or validating them.
  public init(der: [Data]) { certificates = der }

  /// Throws `unreadableCertificate` for a PEM that isn't a certificate.
  public init(pem: [String]) throws {
    certificates = try pem.map { SecCertificateCopyData(try KeychainSigner.certificate(pem: $0)) as Data }
  }

  /// Returns the app-supplied DER trust anchors for signature validation.
  public func anchors() throws -> [Data] { certificates }
}
