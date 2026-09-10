import CoreGraphics
import Foundation
import Testing
@testable import EpdfOps

/// Same frozen file the Rust tests read: pdfium-multiplatform-prototype/vectors/report-page4.json.
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

    let text = try doc.pageText(index: v.pageIndex)
    #expect(text == v.text)
    #expect(text.utf16.count == Int(v.charCount))

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

/// The frozen golden is grayscale (B == G == R in every pixel), so it cannot
/// catch a channel-order bug in `bgraToCGImage` -- a B<->R swap still
/// produces a mean and max abs diff of 0 against it. This test uses a
/// synthetic two-pixel buffer with distinct channel values instead, and
/// reads the pixels back out of the CGImage by drawing it into a CGContext
/// with a known (premultiplied-last, default byte order) layout, so the
/// assertion does not depend on `bgraToCGImage`'s own byte-order choice.
@Test func bgraToCGImagePreservesChannelOrder() throws {
    // Pixel 0: B=10, G=20, R=30, A=255. Pixel 1: B=200, G=150, R=100, A=255.
    let bgra = Data([10, 20, 30, 255, 200, 150, 100, 255])
    let image = try bgraToCGImage(bgra: bgra, width: 2, height: 1, stride: 8)

    let width = 2
    let height = 1
    let bytesPerRow = width * 4
    let buffer = UnsafeMutableRawPointer.allocate(byteCount: bytesPerRow * height, alignment: 1)
    defer { buffer.deallocate() }
    buffer.initializeMemory(as: UInt8.self, repeating: 0, count: bytesPerRow * height)

    guard
        let context = CGContext(
            data: buffer,
            width: width,
            height: height,
            bitsPerComponent: 8,
            bytesPerRow: bytesPerRow,
            space: CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
        )
    else {
        Issue.record("could not create a CGContext to read the CGImage back")
        return
    }
    context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))

    // premultipliedLast with the default (big-endian) byte order lays out
    // R,G,B,A per pixel in memory.
    let pixels = buffer.bindMemory(to: UInt8.self, capacity: bytesPerRow * height)
    #expect(pixels[0] == 30)  // R
    #expect(pixels[1] == 20)  // G
    #expect(pixels[2] == 10)  // B
    #expect(pixels[3] == 255)  // A
    #expect(pixels[4] == 100)  // R
    #expect(pixels[5] == 150)  // G
    #expect(pixels[6] == 200)  // B
    #expect(pixels[7] == 255)  // A
}

@Test func rendersExactDeviceRGBColors() throws {
    let root = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent()
        .deletingLastPathComponent().deletingLastPathComponent()
        .deletingLastPathComponent().deletingLastPathComponent()
    let vector = try JSONSerialization.jsonObject(
        with: Data(contentsOf: root.appendingPathComponent("vectors/colors.json"))
    ) as! [String: Any]
    let doc = try EpdfEngine().open(
        bytes: Data(contentsOf: root.appendingPathComponent("fixtures/colors.pdf")), password: nil)
    defer { doc.close() }
    let bitmap = try doc.renderPage(index: 0, scale: Float(truncating: vector["scale"] as! NSNumber))
    #expect(Int(bitmap.width) == vector["width"] as! Int)
    #expect(Int(bitmap.height) == vector["height"] as! Int)
    for sample in vector["samples"] as! [[String: Any]] {
        let offset = (sample["y"] as! Int) * Int(bitmap.stride) + (sample["x"] as! Int) * 4
        #expect(Array(bitmap.bgra[offset..<offset + 4]) == (sample["bgra"] as! [Int]).map { UInt8($0) })
    }
}
