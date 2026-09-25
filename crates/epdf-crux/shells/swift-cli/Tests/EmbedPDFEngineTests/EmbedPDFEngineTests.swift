// The facade, as a consumer sees it. Nothing here imports a Crux module.
import CoreGraphics
import EmbedPDFEngine
import Foundation
import XCTest

private let fixtures = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent()
    .appendingPathComponent("../../../../../../tooling/engine-conformance/fixtures")
    .standardized

private func fixture(_ name: String) throws -> Data {
    try Data(contentsOf: fixtures.appendingPathComponent(name))
}

final class EmbedPDFEngineTests: XCTestCase {
    func testOpenListsPagesByDurableIdentity() async throws {
        let document = try await PDFEngine().open(data: fixture("annotation_kinds.pdf"))
        XCTAssertEqual(document.pageCount, 1)
        let page = document.page(at: 0)
        XCTAssertGreaterThan(page.objectNumber, 0)
        XCTAssertEqual(page.size, CGSize(width: 612, height: 792))
    }

    func testRenderReturnsAnImageOfTheRequestedSize() async throws {
        let document = try await PDFEngine().open(data: fixture("annotation_kinds.pdf"))
        let image = try await document.page(at: 0).render(scale: 0.5)
        XCTAssertEqual(image.width, 306)
        XCTAssertEqual(image.height, 396)
    }

    func testTextKeepsTheEnginesLineBreaks() async throws {
        let document = try await PDFEngine().open(data: fixture("reverse_and_gapped_text.pdf"))
        let text = try await document.page(at: 0).text()
        XCTAssertTrue(text.hasPrefix("ABCD\r\nE F"), text.debugDescription)
    }

    func testConcurrentRendersEachResumeTheirOwnCaller() async throws {
        // The facade's reason to exist: Crux answers with notifications, and
        // two in flight on one page must still reach the right awaits.
        let page = try await PDFEngine().open(data: fixture("annotation_kinds.pdf")).page(at: 0)
        async let small = page.render(scale: 0.25)
        async let large = page.render(scale: 1)
        let (a, b) = try await (small, large)
        XCTAssertEqual(a.width, 153)
        XCTAssertEqual(b.width, 612)
    }

    func testAMissingPasswordIsATypedErrorAndARetryWithItOpens() async throws {
        let engine = PDFEngine()
        let bytes = try fixture("encrypted-r4-aes-128.pdf")
        do {
            _ = try await engine.open(data: bytes)
            XCTFail("an encrypted document opened without its password")
        } catch let error as PDFError {
            XCTAssertEqual(error.code, .docPasswordRequired)
        }
        let document = try await engine.open(data: bytes, password: "user")
        XCTAssertGreaterThan(document.pageCount, 0)
    }

    func testGarbageIsAnOpenFailureNotACrash() async throws {
        do {
            _ = try await PDFEngine().open(data: Data("not a pdf".utf8))
            XCTFail("garbage opened")
        } catch let error as PDFError {
            XCTAssertEqual(error.code, .docOpenFailed, error.description)
        }
    }

    func testACancelledRenderThrowsAndTheEngineStaysUsable() async throws {
        let page = try await PDFEngine().open(data: fixture("annotation_kinds.pdf")).page(at: 0)
        let cancelled = Task { try await page.render(scale: 4) }
        cancelled.cancel()
        do {
            _ = try await cancelled.value
            // The render may simply have won the race; that is allowed.
        } catch is CancellationError {}
        let image = try await page.render(scale: 0.25)
        XCTAssertEqual(image.width, 153)
    }

    func testReleasingTheEngineStopsItWithoutHanging() async throws {
        weak var released: PDFEngine?
        do {
            let engine = PDFEngine()
            released = engine
            let document = try await engine.open(data: fixture("annotation_kinds.pdf"))
            _ = try await document.page(at: 0).render(scale: 0.25)
        }
        // Effects already queued may hold the loop for a moment; the engine
        // itself has no such reference.
        XCTAssertNil(released, "the engine outlived every reference to it")
    }
}
