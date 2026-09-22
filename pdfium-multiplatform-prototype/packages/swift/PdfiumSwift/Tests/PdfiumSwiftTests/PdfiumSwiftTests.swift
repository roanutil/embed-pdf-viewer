import Foundation
import Testing
@testable import PdfiumSwift

@Test func directSwiftOperationsMatchTheFixture() throws {
    let here = URL(fileURLWithPath: #filePath)
    let fixture = here.deletingLastPathComponent().appendingPathComponent("../../../../../fixtures/report.pdf").standardized
    let document = try PdfiumSwiftEngine().open(Data(contentsOf: fixture))
    #expect(try document.pageCount() == 14)
    let size = try document.pageSize(4)
    #expect(abs(size.width - 612) < 0.001)
    #expect(abs(size.height - 792) < 0.001)
    #expect(try document.pageText(4).utf16.count == 8_106)
    #expect(try document.search(4, query: "the", caseSensitive: false).count == 120)
    let bitmap = try document.renderPage(4, scale: 0.25)
    #expect(bitmap.width == 153)
    #expect(bitmap.height == 198)
    #expect(bitmap.bgra.count == 121_176)
    document.close()
    #expect(throws: PdfiumSwiftError.closed) { try document.pageCount() }
}
