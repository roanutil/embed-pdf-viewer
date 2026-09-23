import Foundation
import XCTest

@testable import EmbedPDF

/// The Phase 1 iOS smoke run.
///
/// `crates/epdf-uniffi/tests/native_surface.rs` already covers this API on the
/// host. What this adds is the only thing that test cannot say: that the
/// XCFramework links, loads and runs inside an iOS process, against the arm64
/// simulator slice of the fork's own PDFium build. So the assertions here are
/// deliberately the same ones, and any divergence between the two is a
/// distribution problem rather than a logic problem.
final class SmokeTests: XCTestCase {
    private func fixture(_ name: String) throws -> Data {
        let url = try XCTUnwrap(
            Bundle.module.url(forResource: name, withExtension: "pdf", subdirectory: "Fixtures"),
            "fixture \(name).pdf is missing from the test bundle"
        )
        return try Data(contentsOf: url)
    }

    func testADocumentOpensAndReportsItsPages() throws {
        let engine = PdfEngine()
        defer { engine.shutdown() }
        let document = try engine.open(bytes: try fixture("report"), password: nil)
        let pages = try document.pages()

        XCTAssertEqual(pages.count, 14)
        XCTAssertEqual(pages[0].index, 0)
        XCTAssertGreaterThan(pages[0].objectNumber, 0, "an index is not identity")
        XCTAssertGreaterThan(pages[0].widthPoints, 0)
        XCTAssertGreaterThan(pages[0].heightPoints, 0)
    }

    func testAMissingPasswordReportsACodeAFacadeCanPromptOn() throws {
        let engine = PdfEngine()
        defer { engine.shutdown() }
        XCTAssertThrowsError(try engine.open(bytes: try fixture("encrypted-hello"), password: nil)) {
            guard case let PdfError.Engine(code, _) = $0 else {
                return XCTFail("expected a PdfError.Engine, got \($0)")
            }
            XCTAssertEqual(code, "DocPasswordRequired")
        }
    }

    func testGeometryComesBackAsTheSameSnapshotTheWebEngineProduces() throws {
        let engine = PdfEngine()
        defer { engine.shutdown() }
        let document = try engine.open(bytes: try fixture("report"), password: nil)
        let pages = try document.pages()
        let json = try document.pageGeometryJson(pageObjectNumber: pages[0].objectNumber)

        let snapshot = try XCTUnwrap(
            JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any]
        )
        let runs = try XCTUnwrap(snapshot["runs"] as? [[String: Any]])
        let glyphs = runs.reduce(0) { $0 + (($1["glyphs"] as? [Any])?.count ?? 0) }

        // The counts the frozen corpus vectors carry for this page. The web
        // module and this one are reading the same document the same way.
        XCTAssertEqual(runs.count, 238)
        XCTAssertEqual(glyphs, 5087)
    }

    func testAPageRendersToRgbaTheCallerOwns() throws {
        let engine = PdfEngine()
        defer { engine.shutdown() }
        let document = try engine.open(bytes: try fixture("colors"), password: nil)
        let pages = try document.pages()
        let bitmap = try document.renderPage(pageObjectNumber: pages[0].objectNumber, scale: 1.0)

        XCTAssertGreaterThan(bitmap.width, 0)
        XCTAssertGreaterThan(bitmap.height, 0)
        XCTAssertGreaterThanOrEqual(
            bitmap.stride, bitmap.width * 4,
            "stride is explicit because it may exceed the packed row width"
        )
        XCTAssertEqual(bitmap.rgba.count, Int(bitmap.stride * bitmap.height))

        // Channel order, on a fixture with saturated interior colors. A
        // grayscale page cannot tell a correct conversion from a B-to-R swap.
        var sawRed = false
        var sawBlue = false
        bitmap.rgba.withUnsafeBytes { raw in
            for row in 0..<Int(bitmap.height) {
                let start = row * Int(bitmap.stride)
                for x in stride(from: 0, to: Int(bitmap.width) * 4, by: 4) {
                    let (r, g, b) = (raw[start + x], raw[start + x + 1], raw[start + x + 2])
                    if r > 200 && g < 80 && b < 80 { sawRed = true }
                    if b > 200 && g < 80 && r < 80 { sawBlue = true }
                }
            }
        }
        XCTAssertTrue(sawRed, "no red pixel: the channels are probably swapped")
        XCTAssertTrue(sawBlue, "no blue pixel: the channels are probably swapped")

        // The bytes are the caller's, so the image outlives its document.
        let before = bitmap.rgba
        document.shutdown()
        XCTAssertEqual(bitmap.rgba, before)
    }

    func testABadScaleIsRefusedBeforeAnythingIsAllocated() throws {
        let engine = PdfEngine()
        defer { engine.shutdown() }
        let document = try engine.open(bytes: try fixture("report"), password: nil)
        let number = try document.pages()[0].objectNumber
        for scale in [Float(0), -1, .nan, .infinity] {
            XCTAssertThrowsError(
                try document.renderPage(pageObjectNumber: number, scale: scale),
                "scale \(scale) should be refused"
            ) {
                guard case let PdfError.Engine(code, _) = $0 else {
                    return XCTFail("expected a PdfError.Engine, got \($0)")
                }
                XCTAssertEqual(code, "InvalidArg")
            }
        }
    }

    func testClosingTheEngineClosesItsDocumentsAndRefusesNewWork() throws {
        let engine = PdfEngine()
        let document = try engine.open(bytes: try fixture("report"), password: nil)
        _ = try document.pages()

        engine.shutdown()
        XCTAssertTrue(engine.isClosed())
        XCTAssertThrowsError(try document.pages())
        XCTAssertThrowsError(try engine.open(bytes: try fixture("report"), password: nil))

        // Idempotent, per the lifecycle contract.
        engine.shutdown()
    }

    func testEveryEngineCallRunsOffTheCallersThread() throws {
        // The property the whole design exists for, and the one an iOS process
        // is the right place to check: a Swift concurrency pool will hop
        // threads under you, so confinement cannot be a promise the caller
        // keeps. Calling from several queues at once would be a data race if
        // any PDFium work ran on the caller.
        let engine = PdfEngine()
        defer { engine.shutdown() }
        let document = try engine.open(bytes: try fixture("report"), password: nil)

        let done = expectation(description: "concurrent callers")
        done.expectedFulfillmentCount = 4
        for _ in 0..<4 {
            DispatchQueue.global().async {
                XCTAssertEqual(try? document.pages().count, 14)
                done.fulfill()
            }
        }
        wait(for: [done], timeout: 60)
    }

    func testTwoEnginesAreIndependent() throws {
        let first = PdfEngine()
        let second = PdfEngine()
        let a = try first.open(bytes: try fixture("report"), password: nil)
        let b = try second.open(bytes: try fixture("report"), password: nil)

        first.shutdown()
        XCTAssertThrowsError(try a.pages(), "the closed engine's document is gone")
        XCTAssertEqual(try b.pages().count, 14, "the other engine still works")
        second.shutdown()
    }
}
