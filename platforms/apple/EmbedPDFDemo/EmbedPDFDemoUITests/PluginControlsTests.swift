import XCTest

final class PluginControlsTests: XCTestCase {
  @MainActor
  func testAddNoteUndoRedo() throws {
    let app = XCUIApplication()
    app.launch()
    let add = app.buttons["addNote"]
    XCTAssertTrue(add.waitForExistence(timeout: 20))
    let undo = app.buttons["historyUndo"]
    let redo = app.buttons["historyRedo"]
    XCTAssertTrue(undo.waitForExistence(timeout: 10))
    XCTAssertFalse(undo.isEnabled)
    add.tap()
    waitUntilEnabled(undo)
    undo.tap()
    waitUntilEnabled(redo)
    redo.tap()
    waitUntilEnabled(undo)
  }

  @MainActor
  private func waitUntilEnabled(_ element: XCUIElement) {
    let enabled = expectation(for: NSPredicate(format: "enabled == true"), evaluatedWith: element)
    wait(for: [enabled], timeout: 10)
  }
}
