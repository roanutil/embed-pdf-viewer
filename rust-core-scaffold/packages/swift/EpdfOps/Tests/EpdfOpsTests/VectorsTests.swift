import Foundation
import Testing
@testable import EpdfOps

/// Same frozen file the Rust tests read: rust-core-scaffold/vectors/report-page4.json.
/// A Swift binding that drifts fails here instead of looking fine on screen.
private struct Vectors {
    let pageIndex: UInt32
    let pageCount: UInt32
    let width: Float
    let height: Float
    let charCount: UInt32
    let text: String
    let searchQuery: String
    let searchCaseSensitive: Bool
    let searchHitCount: Int
    let renderScale: Float
    let renderWidth: UInt32
    let renderHeight: UInt32
    let renderStride: UInt32
    let golden: Data
    let tolerance: Double
    let maxTolerance: UInt8

    static func load() throws -> Vectors {
        let root = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()   // EpdfOpsTests
            .deletingLastPathComponent()   // Tests
            .deletingLastPathComponent()   // EpdfOps
            .deletingLastPathComponent()   // swift
            .deletingLastPathComponent()   // packages
            .deletingLastPathComponent()   // scaffold root
        let json = try JSONSerialization.jsonObject(
            with: try Data(contentsOf: root.appendingPathComponent("vectors/report-page4.json"))
        ) as! [String: Any]
        let size = json["size"] as! [String: Any]
        let render = json["render"] as! [String: Any]
        let search = json["search"] as! [String: Any]
        return Vectors(
            pageIndex: UInt32(json["pageIndex"] as! Int),
            pageCount: UInt32(json["pageCount"] as! Int),
            width: Float(size["width"] as! Double),
            height: Float(size["height"] as! Double),
            charCount: UInt32(json["charCount"] as! Int),
            text: json["text"] as! String,
            searchQuery: search["query"] as! String,
            searchCaseSensitive: search["caseSensitive"] as! Bool,
            searchHitCount: search["hitCount"] as! Int,
            renderScale: Float(render["scale"] as! Double),
            renderWidth: UInt32(render["width"] as! Int),
            renderHeight: UInt32(render["height"] as! Int),
            renderStride: UInt32(render["stride"] as! Int),
            golden: try Data(contentsOf: root.appendingPathComponent("vectors/\(render["golden"] as! String)")),
            tolerance: render["meanAbsDiffTolerance"] as! Double,
            maxTolerance: UInt8(render["maxAbsDiffTolerance"] as! Int)
        )
    }
}

private func meanAbsDiff(_ a: Data, _ b: Data) -> Double {
    precondition(a.count == b.count, "buffer lengths differ: \(a.count) vs \(b.count)")
    var total = 0.0
    for i in a.indices { total += Double(abs(Int(a[i]) - Int(b[i]))) }
    return total / Double(a.count)
}

/// Largest single-byte absolute difference. `meanAbsDiff` averages over the
/// whole buffer, so one badly wrong region can hide under a mean-based
/// tolerance as long as most of the buffer matches. This bounds the worst
/// byte instead, mirroring test-support's max_abs_diff on the Rust side.
private func maxAbsDiff(_ a: Data, _ b: Data) -> UInt8 {
    precondition(a.count == b.count, "buffer lengths differ: \(a.count) vs \(b.count)")
    var worst = 0
    for i in a.indices { worst = max(worst, abs(Int(a[i]) - Int(b[i]))) }
    return UInt8(worst)
}

private func openFixture() throws -> EpdfDocument {
    let root = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent()
        .deletingLastPathComponent().deletingLastPathComponent()
        .deletingLastPathComponent().deletingLastPathComponent()
    // uniffi maps Rust `Vec<u8>` to Swift `Data`, so no [UInt8] conversion.
    let bytes = try Data(contentsOf: root.appendingPathComponent("fixtures/report.pdf"))
    return try EpdfEngine().open(bytes: bytes, password: nil)
}

@Test func matchesTheFrozenVectors() throws {
    let v = try Vectors.load()
    let doc = try openFixture()
    defer { doc.close() }

    #expect(try doc.pageCount() == v.pageCount)

    let size = try doc.pageSize(index: v.pageIndex)
    #expect(abs(size.width - v.width) < 0.001)
    #expect(abs(size.height - v.height) < 0.001)

    #expect(try doc.pageText(index: v.pageIndex) == v.text)

    let hits = try doc.search(index: v.pageIndex, query: v.searchQuery, caseSensitive: v.searchCaseSensitive)
    #expect(hits.count == v.searchHitCount)

    let bitmap = try doc.renderPage(index: v.pageIndex, scale: v.renderScale)
    #expect(bitmap.width == v.renderWidth)
    #expect(bitmap.height == v.renderHeight)
    #expect(bitmap.stride == v.renderStride)

    let mean = meanAbsDiff(bitmap.bgra, v.golden)
    #expect(mean <= v.tolerance, "mean abs diff \(mean) exceeds \(v.tolerance)")
    let worst = maxAbsDiff(bitmap.bgra, v.golden)
    #expect(worst <= v.maxTolerance, "max abs diff \(worst) exceeds \(v.maxTolerance)")
}

@Test func aPagePastTheEndThrows() throws {
    let doc = try openFixture()
    defer { doc.close() }
    let index = try doc.pageCount()
    #expect(throws: OpsError.PageOutOfRange(index: index)) { try doc.pageSize(index: index) }
}

@Test func rendersACGImage() throws {
    let v = try Vectors.load()
    let doc = try openFixture()
    defer { doc.close() }
    let image = try doc.renderPageCGImage(index: v.pageIndex, scale: v.renderScale)
    #expect(image.width == Int(v.renderWidth))
    #expect(image.height == Int(v.renderHeight))
}
