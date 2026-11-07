import XCTest

final class PluginControlsTests: XCTestCase {
  @MainActor
  func testHistoryAtAccessibilityText() throws {
    let app = XCUIApplication()
    app.launchArguments = ["-UIPreferredContentSizeCategoryName", "UICTContentSizeCategoryAccessibilityXXXL"]
    app.launch()
    let undo = app.buttons["historyUndo"]
    XCTAssertTrue(undo.waitForExistence(timeout: 30))
    XCTAssertTrue(undo.isHittable)
    XCTAssertTrue(app.buttons["historyRedo"].isHittable)
    let screenshot = XCTAttachment(screenshot: app.screenshot())
    screenshot.lifetime = .keepAlways
    add(screenshot)
  }

  @MainActor
  func testCreateAndActivateBookmark() throws {
    let app = XCUIApplication()
    app.launch()
    let bookmarks = app.buttons["showBookmarks"]
    XCTAssertTrue(bookmarks.waitForExistence(timeout: 30))
    bookmarks.tap()
    let add = app.buttons["outlineAdd"]
    XCTAssertTrue(add.waitForExistence(timeout: 10))
    waitUntilEnabled(add)
    add.tap()
    let title = app.textFields["outlineTitle"]
    XCTAssertTrue(title.waitForExistence(timeout: 10))
    title.tap()
    title.typeText("My location")
    app.buttons["outlineSave"].tap()
    let bookmark = app.buttons.containing(.staticText, identifier: "My location").firstMatch
    XCTAssertTrue(bookmark.waitForExistence(timeout: 10))
    bookmark.tap()
    XCTAssertTrue(app.images["Current location"].waitForExistence(timeout: 10))
  }

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
