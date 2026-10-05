import XCTest

/// The profiling scenario: fling through `report.pdf` and back, then search
/// for "trace" and step through ten hits. With EMBEDPDF_PROFILE_DELAY set in
/// the test runner's environment (seconds), it waits that long after the
/// document appears, so Instruments can attach to the app before the
/// scenario starts. With EMBEDPDF_HOLD_AT_END it keeps the app alive that
/// long after the scenario, for a memory or leaks measurement.
final class ScrollAndSearchTests: XCTestCase {
  @MainActor
  func testScrollThenSearch() throws {
    let app = XCUIApplication()
    app.launch()

    let pages = app.scrollViews["pages"]
    XCTAssertTrue(pages.waitForExistence(timeout: 30), "the document never opened")

    if let delay = ProcessInfo.processInfo.environment["EMBEDPDF_PROFILE_DELAY"].flatMap(Double.init) {
      Thread.sleep(forTimeInterval: delay)
    }

    for _ in 0..<10 { pages.swipeUp(velocity: .fast) }
    for _ in 0..<10 { pages.swipeDown(velocity: .fast) }

    let field = app.textFields["searchField"]
    field.tap()
    field.typeText("trace\n")

    let summary = app.staticTexts["searchSummary"]
    let found = NSPredicate(format: "label CONTAINS '/'")
    expectation(for: found, evaluatedWith: summary)
    waitForExpectations(timeout: 30)

    let next = app.buttons["Next"]
    for _ in 0..<10 { next.tap() }

    if let hold = ProcessInfo.processInfo.environment["EMBEDPDF_HOLD_AT_END"].flatMap(Double.init) {
      print("HOLDING \(hold)s for measurement")
      Thread.sleep(forTimeInterval: hold)
    }
  }
}
