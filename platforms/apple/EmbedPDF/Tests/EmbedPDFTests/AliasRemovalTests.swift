import EmbedPDF
import Foundation
import Testing

extension BaseSuite {
  /// The rule: a deprecated shim survives at least one tagged release. The
  /// two renamed types, `PDFDocument` and `Annotation`, are removed in
  /// 0.0.0-swift.6. These tests fail if either is deleted early, or if its
  /// deprecation message stops naming the release.
  @Suite struct AliasRemovalTests {
    static let removalRelease = "0.0.0-swift.6"

    /// Compiles both old spellings. Deprecated, so its own uses don't warn.
    @available(*, deprecated)
    static func oldSpellings() -> (String, String) {
      (String(describing: PDFDocument.self), String(describing: Annotation.self))
    }

    @available(*, deprecated)
    @Test func oldSpellingsStillCompileAndAreTheRenamedTypes() {
      let (document, annotation) = Self.oldSpellings()
      #expect(document == String(describing: DocumentInfo.self))
      #expect(annotation == String(describing: AnnotationRecord.self))
    }

    /// The sources sit beside this package's Tests directory.
    static func source(_ path: String) throws -> String {
      let package = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
      return try String(contentsOf: package.appending(path: "Sources/EmbedPDF/\(path)"), encoding: .utf8)
    }

    @Test(arguments: [
      ("Exports.swift", "PDFDocument", "DocumentInfo"),
      ("Plugins/Annotation.swift", "Annotation", "AnnotationRecord"),
    ])
    func deprecationNamesTheRemovalRelease(file: String, old: String, new: String) throws {
      let text = try Self.source(file)
      let declaration = "public typealias \(old) = \(new)"
      let range = try #require(text.range(of: declaration))
      let attribute = try #require(
        text[..<range.lowerBound].components(separatedBy: "\n").last(where: { $0.hasPrefix("@available") }))
      #expect(attribute.contains("deprecated"))
      #expect(attribute.contains("\(old) is removed in \(Self.removalRelease)"))
    }
  }
}
