// Entry points named after what a caller wants to do, over capabilities
// named after the engine part that does it. Each one holds only the
// capability it forwards to, which it hands back for everything else, and
// every member is a one-line forward with the capability member's own name,
// signature and errors. `TaskEntryPointTests` keeps the two sides in step.

/// Move through a document's pages: the current page, the page count, and
/// going to a page. Get one with `PDFViewer.pageNavigator(for:)`, which needs
/// what `PDFViewer.stage(for:)` needs and throws what it throws. For
/// everything else (camera, scrolling, layout, coordinates, gestures) use
/// ``stage``.
public struct PageNavigator: Sendable {
  /// The capability every member forwards to.
  public let stage: StageCapability

  public func pageCount() async throws(PDFError) -> Int { try await stage.pageCount() }
  public func currentPageIndex() async throws(PDFError) -> Int { try await stage.currentPageIndex() }
  public func currentPage() async throws(PDFError) -> PageRef? { try await stage.currentPage() }
  public func canGoNext() async throws(PDFError) -> Bool { try await stage.canGoNext() }
  public func canGoPrevious() async throws(PDFError) -> Bool { try await stage.canGoPrevious() }
  /// An index below zero or past `UInt32.max` throws `invalid-input`.
  public func goToPageIndex(_ index: Int, options: StageGoToOptions? = nil) async throws(PDFError) { try await stage.goToPageIndex(index, options: options) }
  public func goToPage(_ page: PageRef, options: StageGoToOptions? = nil) async throws(PDFError) { try await stage.goToPage(page, options: options) }
  public func goToFirstPage(options: StageGoToOptions? = nil) async throws(PDFError) { try await stage.goToFirstPage(options: options) }
  public func goToLastPage(options: StageGoToOptions? = nil) async throws(PDFError) { try await stage.goToLastPage(options: options) }
  public func nextPage(options: StageGoToOptions? = nil) async throws(PDFError) { try await stage.nextPage(options: options) }
  public func previousPage(options: StageGoToOptions? = nil) async throws(PDFError) { try await stage.previousPage(options: options) }
}

/// Zoom a document's view in and out and fit its pages to the viewport.
/// Get one with `PDFViewer.pageZoom(for:)`, which needs what
/// `PDFViewer.stage(for:)` needs and throws what it throws. For everything
/// else (zoom by a factor, around a point, the zoom tool's box, rotation)
/// use ``stage``.
public struct PageZoom: Sendable {
  /// The capability every member forwards to.
  public let stage: StageCapability

  public func zoomLevel() async throws(PDFError) -> Double { try await stage.zoomLevel() }
  public func zoomMode() async throws(PDFError) -> StageZoomMode { try await stage.zoomMode() }
  public func canZoomIn() async throws(PDFError) -> Bool { try await stage.canZoomIn() }
  public func canZoomOut() async throws(PDFError) -> Bool { try await stage.canZoomOut() }
  public func zoomIn() async throws(PDFError) { try await stage.zoomIn() }
  public func zoomOut() async throws(PDFError) { try await stage.zoomOut() }
  public func zoomTo(_ target: StageZoomTarget, around: Point? = nil) async throws(PDFError) { try await stage.zoomTo(target, around: around) }
  public func fitWidth() async throws(PDFError) { try await stage.fitWidth() }
  public func fitPage() async throws(PDFError) { try await stage.fitPage() }
}

/// Pick the tool pointer input goes to: list the tools, read the active
/// one, and switch. Get one with `PDFViewer.toolSwitcher(for:)`, which needs
/// what `PDFViewer.interaction(for:)` needs and throws what it throws. For
/// everything else (registering tools and handlers, cursors, pointer
/// samples, settings) use ``interaction``.
public struct ToolSwitcher: Sendable {
  /// The capability every member forwards to.
  public let interaction: InteractionCapability

  public func tools() async throws(PDFError) -> [InteractionTool] { try await interaction.tools() }
  public func activeToolID() async throws(PDFError) -> ToolID { try await interaction.activeToolID() }
  public func defaultToolID() async throws(PDFError) -> ToolID { try await interaction.defaultToolID() }
  public func hasTool(_ id: ToolID) async throws(PDFError) -> Bool { try await interaction.hasTool(id) }
  /// Throws `not-found` for an id nobody registered.
  public func activateTool(_ id: ToolID) async throws(PDFError) { try await interaction.activateTool(id) }
  public func activateDefaultTool() async throws(PDFError) { try await interaction.activateDefaultTool() }
  public func pushTool(_ id: ToolID) async throws(PDFError) { try await interaction.pushTool(id) }
  public func popTool() async throws(PDFError) { try await interaction.popTool() }
}

extension PDFViewer {
  public func pageNavigator(for document: DocumentInfo) async throws(PDFError) -> PageNavigator {
    PageNavigator(stage: try await stage(for: document))
  }

  public func pageZoom(for document: DocumentInfo) async throws(PDFError) -> PageZoom {
    PageZoom(stage: try await stage(for: document))
  }

  public func toolSwitcher(for document: DocumentInfo) async throws(PDFError) -> ToolSwitcher {
    ToolSwitcher(interaction: try await interaction(for: document))
  }
}
