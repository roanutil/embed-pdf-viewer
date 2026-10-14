public import EmbedPDFBindings
public import Foundation
public import Security

/// A `SignatureSigner` over a `SecKey` that never leaves the Keychain (or
/// the Secure Enclave), and its certificate chain. Build one from a
/// `SecIdentity` the app imported, or from a key and certificates.
public final class KeychainSigner: SignatureSigner, @unchecked Sendable {
  let key: SecKey
  let chain: [Data]
  let keyAlgorithm: SignatureKeyAlgorithm
  let fieldSize: Int

  public convenience init(identity: SecIdentity) throws {
    var key: SecKey?
    var certificate: SecCertificate?
    guard SecIdentityCopyPrivateKey(identity, &key) == errSecSuccess, let key,
      SecIdentityCopyCertificate(identity, &certificate) == errSecSuccess, let certificate
    else { throw KeychainSignerError.unreadableIdentity }
    try self.init(privateKey: key, certificates: Self.chain(from: certificate))
  }

  /// `certificates` leaf first.
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

  public func certificateChain() throws -> [Data] { chain }
  public func algorithm() -> SignatureKeyAlgorithm { keyAlgorithm }
  public func digest() -> SignatureDigest { .sha256 }

  /// Called on the kernel thread. A Keychain item with an access control
  /// shows its prompt from here; a cancelled prompt throws
  /// `operation-cancelled`.
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
    var outer = DERReader(der)
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

  /// An RSA private key from a PKCS#8 PEM (`-----BEGIN PRIVATE KEY-----`).
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

  /// A certificate from a PEM (`-----BEGIN CERTIFICATE-----`).
  public static func certificate(pem: String) throws -> SecCertificate {
    guard let certificate = SecCertificateCreateWithData(nil, try pemBody(pem) as CFData) else {
      throw KeychainSignerError.unreadableCertificate
    }
    return certificate
  }

  static func pemBody(_ pem: String) throws -> Data {
    let body = pem.split(whereSeparator: \.isNewline).filter { !$0.hasPrefix("-----") }.joined()
    guard let data = Data(base64Encoded: body) else { throw KeychainSignerError.unreadableKey }
    return data
  }

  /// `PrivateKeyInfo { version, algorithm, OCTET STRING privateKey }`.
  static func unwrapPKCS8(_ der: Data) throws -> Data {
    var outer = DERReader(der)
    var info = try outer.take(0x30)
    _ = try info.take(0x02)
    _ = try info.take(0x30)
    return Data(try info.take(0x04).bytes)
  }
}

public enum KeychainSignerError: Error, Sendable {
  case unreadableIdentity, unreadableKey, unreadableCertificate, badSignature
}

/// A DER TLV reader: definite lengths only, which is all a signature or a
/// key container uses.
struct DERReader {
  var bytes: ArraySlice<UInt8>

  init(_ data: Data) { bytes = [UInt8](data)[...] }
  init(_ slice: ArraySlice<UInt8>) { bytes = slice }

  /// The value of the next element, which must carry `tag`.
  mutating func take(_ tag: UInt8) throws -> DERReader {
    guard bytes.popFirst() == tag, let first = bytes.popFirst() else { throw KeychainSignerError.badSignature }
    var length = Int(first)
    if first & 0x80 != 0 {
      length = 0
      for _ in 0..<Int(first & 0x7f) {
        guard let b = bytes.popFirst() else { throw KeychainSignerError.badSignature }
        length = length << 8 | Int(b)
      }
    }
    guard bytes.count >= length else { throw KeychainSignerError.badSignature }
    defer { bytes = bytes.dropFirst(length) }
    return DERReader(bytes.prefix(length))
  }
}

/// Trust anchors the app supplies, DER. iOS can't list the system's roots,
/// so anchors are the app's own (its organisation's CA, say).
public final class TrustAnchors: SignatureTrust, Sendable {
  public let certificates: [Data]

  public init(der: [Data]) { certificates = der }

  public init(pem: [String]) throws { certificates = try pem.map(KeychainSigner.pemBody) }

  public func anchors() throws -> [Data] { certificates }
}
