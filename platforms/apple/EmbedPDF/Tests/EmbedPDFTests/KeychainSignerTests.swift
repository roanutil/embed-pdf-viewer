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
  }
}
