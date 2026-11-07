import XCTest

final class PluginControlsTests: XCTestCase {
  @MainActor
  func testImportAndExportAttachment() throws {
    let app = XCUIApplication()
    app.launchArguments = ["--attachment-fixture"]
    app.launch()
    let attachments = app.buttons["showAttachments"]
    XCTAssertTrue(attachments.waitForExistence(timeout: 30))
    attachments.tap()
    let importing = app.buttons["attachmentImport"]
    XCTAssertTrue(importing.waitForExistence(timeout: 10))
    waitUntilEnabled(importing)
    importing.tap()
    let browse = app.descendants(matching: .any)["Browse"].firstMatch
    if browse.waitForExistence(timeout: 3) { browse.tap() }
    let back = app.buttons["DOC.navBarButton.backInHistory"]
    if back.waitForExistence(timeout: 3), back.isEnabled { back.tap() }
    let local = app.descendants(matching: .any)["On My iPhone"].firstMatch
    if local.waitForExistence(timeout: 3) { local.tap() }
    let folder = app.cells["EmbedPDF Demo, Container"]
    if folder.waitForExistence(timeout: 3) { folder.tap() }
    let pickerScreenshot = XCTAttachment(screenshot: app.screenshot())
    pickerScreenshot.name = "Picker after folder navigation"
    pickerScreenshot.lifetime = .keepAlways
    add(pickerScreenshot)
    let fixture = app.cells.matching(NSPredicate(format: "label BEGINSWITH %@", "attachment-fixture")).firstMatch
    guard fixture.waitForExistence(timeout: 10) else { XCTFail(app.debugDescription); return }
    fixture.tap()
    let open = app.navigationBars["FullDocumentManagerViewControllerNavigationBar"].buttons["Open"]
    if open.waitForExistence(timeout: 3) { open.tap() }
    let file = app.buttons.containing(.staticText, identifier: "attachment-fixture.txt").firstMatch
    XCTAssertTrue(file.waitForExistence(timeout: 10))
    file.tap()
    let export = app.buttons["attachmentExport"]
    waitUntilEnabled(export)
    export.tap()
    XCTAssertTrue(app.buttons["Save"].waitForExistence(timeout: 10))
    app.buttons["Save"].tap()
    let replace = app.buttons["Replace"]
    if replace.waitForExistence(timeout: 3) { replace.tap() }
    XCTAssertTrue(file.waitForExistence(timeout: 10))
  }

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
