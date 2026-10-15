import CustomDump
@testable import EmbedPDF
import Foundation
import Security
import Testing

extension BaseSuite {
  @Suite struct KeychainSignerTests {
    static func testSigner() throws -> KeychainSigner {
      let key = try KeychainSigner.privateKey(pkcs8PEM: Fixture.text("TEST-ONLY-private-key", ext: "pem"))
      let cert = try KeychainSigner.certificate(pem: Fixture.text("TEST-ONLY-signer", ext: "pem"))
      return try KeychainSigner(privateKey: key, certificates: [cert])
    }

    @Test func anRSAKeySignsPKCS1() throws {
      let signer = try Self.testSigner()
      expectNoDifference(signer.algorithm(), .rsaPkcs1)
      expectNoDifference(signer.digest(), .sha256)
      let signature = try signer.sign(message: Data("hello".utf8))
      #expect(signature.count == 256)
      expectNoDifference(try signer.certificateChain().count, 1)
    }

    @Test func anECDSASignatureIsRawRAndS() throws {
      let attributes: [CFString: Any] = [kSecAttrKeyType: kSecAttrKeyTypeECSECPrimeRandom, kSecAttrKeySizeInBits: 256]
      var error: Unmanaged<CFError>?
      let key = try #require(SecKeyCreateRandomKey(attributes as CFDictionary, &error))
      let der = try #require(SecKeyCreateSignature(key, .ecdsaSignatureMessageX962SHA256, Data("x".utf8) as CFData, &error)) as Data
      let raw = try KeychainSigner.rawECDSA(der: der, fieldSize: 32)
      expectNoDifference(raw.count, 64)
    }

    @Test func anECKeySignsRawRAndS() throws {
      let attributes: [CFString: Any] = [kSecAttrKeyType: kSecAttrKeyTypeECSECPrimeRandom, kSecAttrKeySizeInBits: 256]
      var error: Unmanaged<CFError>?
      let key = try #require(SecKeyCreateRandomKey(attributes as CFDictionary, &error))
      let cert = try KeychainSigner.certificate(pem: Fixture.text("TEST-ONLY-signer", ext: "pem"))
      let signer = try KeychainSigner(privateKey: key, certificates: [cert])
      expectNoDifference(signer.algorithm(), .ecdsa)
      expectNoDifference(try signer.sign(message: Data("x".utf8)).count, 64)
    }

    @Test func aMalformedSignatureIsRefused() {
      #expect(throws: KeychainSignerError.self) { try KeychainSigner.rawECDSA(der: Data([0x30, 0x05, 0x02]), fieldSize: 32) }
    }

    /// Long-form lengths that once trapped in `prefix`: eight length bytes
    /// shift into a negative `Int`, and `0x80` is BER's indefinite length.
    @Test(arguments: [
      [0x30, 0x88, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff],
      [0x30, 0xff] + [UInt8](repeating: 0xff, count: 127),
      [0x30, 0x84, 0xff, 0xff, 0xff, 0xff, 0x02],
      [0x30, 0x80, 0x02, 0x01, 0x01],
      [0x30, 0x03, 0x02, 0x88, 0x80],
    ] as [[UInt8]])
    func aMalformedDERLengthThrowsInsteadOfTrapping(der: [UInt8]) {
      #expect(throws: KeychainSignerError.badSignature) {
        try KeychainSigner.rawECDSA(der: Data(der), fieldSize: 32)
      }
    }

    @Test func aMalformedPKCS8KeyIsUnreadable() {
      let body = Data([0x30, 0x88, 0x80, 0, 0, 0, 0, 0, 0, 0]).base64EncodedString()
      let pem = "-----BEGIN PRIVATE KEY-----\n\(body)\n-----END PRIVATE KEY-----"
      #expect(throws: KeychainSignerError.unreadableKey) { try KeychainSigner.privateKey(pkcs8PEM: pem) }
    }

    @Test(arguments: ["not base64 at all!", Data("not a certificate".utf8).base64EncodedString()])
    func aBadAnchorIsAnUnreadableCertificate(body: String) {
      let pem = "-----BEGIN CERTIFICATE-----\n\(body)\n-----END CERTIFICATE-----"
      #expect(throws: KeychainSignerError.unreadableCertificate) { try TrustAnchors(pem: [pem]) }
      #expect(throws: KeychainSignerError.unreadableCertificate) { try KeychainSigner.certificate(pem: pem) }
    }

    @Test func aCertificatePEMIsAnAnchor() throws {
      let anchors = try TrustAnchors(pem: [Fixture.text("TEST-ONLY-signer", ext: "pem")])
      expectNoDifference(anchors.certificates.count, 1)
      expectNoDifference(anchors.certificates.first?.first, 0x30)
    }
  }
}
