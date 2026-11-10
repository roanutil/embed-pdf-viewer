import CoreGraphics
import CustomDump
import EmbedPDF
import Foundation
import Testing

extension BaseSuite {
  @Suite struct AnnotationColorTests {
    @Test func hexReadsEitherCaseAndWritesLowercase() {
      let color = AnnotationColor(hex: "#1F6fEb")
      expectNoDifference(color, AnnotationColor(red: 0x1f, green: 0x6f, blue: 0xeb))
      expectNoDifference(color?.hex, "#1f6feb")
      expectNoDifference(AnnotationColor(red: 0, green: 10, blue: 255).hex, "#000aff")
    }

    @Test(arguments: ["", "#", "1f6feb", "#1f6fe", "#1f6feb0", "#1f6fez", "#ffffff ", " #ffffff", "#ｆｆｆｆｆｆ"])
    func anythingButHashAndSixHexDigitsIsRefused(text: String) {
      expectNoDifference(AnnotationColor(hex: text), nil)
    }

    @Test func aLiteralIsTheColourItNames() {
      let color: AnnotationColor = "#E5484D"
      expectNoDifference(color, AnnotationColor(red: 0xe5, green: 0x48, blue: 0x4d))
      expectNoDifference("\(color)", "#e5484d")
    }

    @Test func itCodesAsItsHexString() throws {
      let data = try JSONEncoder().encode(AnnotationColor(red: 255, green: 0, blue: 0))
      expectNoDifference(String(decoding: data, as: UTF8.self), ##""#ff0000""##)
      expectNoDifference(try JSONDecoder().decode(AnnotationColor.self, from: Data(##""#00FF00""##.utf8)), "#00ff00")
      #expect(throws: DecodingError.self) {
        try JSONDecoder().decode(AnnotationColor.self, from: Data(##""green""##.utf8))
      }
    }

    @Test func cgColorIsOpaqueSRGB() throws {
      let color = AnnotationColor(red: 255, green: 0, blue: 51).cgColor
      expectNoDifference(color.colorSpace?.name, CGColorSpace.sRGB)
      expectNoDifference(color.components, [1, 0, 0.2, 1])
    }

    @Test func aNullableColourTakesALiteral() throws {
      let draft = InkAnnotationDraft(inkList: [[Point(x: 0, y: 0)]], color: "#ABCDEF")
      #expect(try draft.json().contains(##""color":"#abcdef""##))
      let patch = InkAnnotationPatch(color: .null)
      expectNoDifference(try patch.json(), ##"{"color":null}"##)
    }

    @Test func annotationInfoIsIdentifiedByItsRef() {
      let record = AnnotationInfoTests.record()
      expectNoDifference(AnnotationInfo(record).id, record.annotationRef)
    }
  }
}
