// Security on Linux: no keychain and no key operations. Every call that
// would make or use a key fails, so the signer tests fail as stubs.
@_exported import AppleCFStub

public final class SecKey {}
public final class SecCertificate {}
public final class SecIdentity {}
public final class SecTrust {}
public final class SecPolicy {}

public struct SecKeyAlgorithm: Hashable, Sendable {
  public let rawValue: String
  public static let ecdsaSignatureMessageX962SHA256 = SecKeyAlgorithm(rawValue: "ecdsa-x962-sha256")
  public static let rsaSignatureMessagePKCS1v15SHA256 = SecKeyAlgorithm(rawValue: "rsa-pkcs1v15-sha256")
}

public let errSecSuccess: OSStatus = 0
public let errSecUserCanceled: OSStatus = -128
public let errSecUnimplemented: OSStatus = -4

public let kSecAttrKeyType: CFString = "type"
public let kSecAttrKeySizeInBits: CFString = "bsiz"
public let kSecAttrKeyClass: CFString = "kcls"
public let kSecAttrKeyTypeRSA: CFString = "42"
public let kSecAttrKeyTypeECSECPrimeRandom: CFString = "73"
public let kSecAttrKeyClassPrivate: CFString = "1"

private func unimplemented(_ error: UnsafeMutablePointer<Unmanaged<CFError>?>?) {
  error?.pointee = Unmanaged.passRetained(CFError(code: Int(errSecUnimplemented)))
}

public func SecIdentityCopyPrivateKey(_ identity: SecIdentity, _ key: UnsafeMutablePointer<SecKey?>) -> OSStatus { errSecUnimplemented }
public func SecIdentityCopyCertificate(_ identity: SecIdentity, _ certificate: UnsafeMutablePointer<SecCertificate?>) -> OSStatus { errSecUnimplemented }
public func SecKeyCopyAttributes(_ key: SecKey) -> CFDictionary? { nil }
public func SecCertificateCopyData(_ certificate: SecCertificate) -> CFData { Data() }
public func SecCertificateCreateWithData(_ allocator: AnyObject?, _ data: CFData) -> SecCertificate? { nil }
public func SecKeyCreateWithData(_ data: CFData, _ attributes: CFDictionary, _ error: UnsafeMutablePointer<Unmanaged<CFError>?>?) -> SecKey? { unimplemented(error); return nil }
public func SecKeyCreateRandomKey(_ attributes: CFDictionary, _ error: UnsafeMutablePointer<Unmanaged<CFError>?>?) -> SecKey? { unimplemented(error); return nil }
public func SecKeyCreateSignature(_ key: SecKey, _ algorithm: SecKeyAlgorithm, _ data: CFData, _ error: UnsafeMutablePointer<Unmanaged<CFError>?>?) -> CFData? { unimplemented(error); return nil }
public func SecPolicyCreateBasicX509() -> SecPolicy { SecPolicy() }
public func SecTrustCreateWithCertificates(_ certificates: AnyObject, _ policies: AnyObject?, _ trust: UnsafeMutablePointer<SecTrust?>) -> OSStatus { errSecUnimplemented }
public func SecTrustCopyCertificateChain(_ trust: SecTrust) -> [SecCertificate]? { nil }
