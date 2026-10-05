public import EmbedPDFBindings

public typealias SearchQuery = EmbedPDFBindings.SearchQuery
public typealias SearchHit = EmbedPDFBindings.SearchHit
public typealias TextSegment = EmbedPDFBindings.TextSegment
public typealias SearchSnippet = EmbedPDFBindings.SearchSnippet
public typealias SearchStatus = EmbedPDFBindings.SearchStatus
public typealias SearchMode = EmbedPDFBindings.SearchMode
public typealias SearchProgress = EmbedPDFBindings.SearchProgress
public typealias SearchResult = EmbedPDFBindings.SearchResult
public typealias SearchResultStatus = EmbedPDFBindings.SearchResultStatus
public typealias SearchRevealOptions = EmbedPDFBindings.SearchRevealOptions
public typealias SearchRevealAnchor = EmbedPDFBindings.SearchRevealAnchor
public typealias SearchRevealAxis = EmbedPDFBindings.SearchRevealAxis
public typealias SearchScrollBehavior = EmbedPDFBindings.SearchScrollBehavior
public typealias SearchConfig = EmbedPDFBindings.SearchConfig
public typealias SearchEvent = EmbedPDFBindings.SearchEvent
public typealias SearchStartedEvent = EmbedPDFBindings.SearchStartedEvent
public typealias SearchProgressEvent = EmbedPDFBindings.SearchProgressEvent
public typealias SearchCompletedEvent = EmbedPDFBindings.SearchCompletedEvent
public typealias SearchCancelledEvent = EmbedPDFBindings.SearchCancelledEvent
public typealias SearchCancelReason = EmbedPDFBindings.SearchCancelReason
public typealias SearchFailedEvent = EmbedPDFBindings.SearchFailedEvent
public typealias SearchActiveHitChangedEvent = EmbedPDFBindings.SearchActiveHitChangedEvent

/// A document's text search: one session (`search`, navigated with
/// `nextHit`, `previousHit` and `goToHit`) and a session-free `findAll`. Get
/// one with `PDFViewer.search(in:)`; the viewer needs
/// `ViewerConfig(search: SearchConfig())`. It keeps the viewer alive. After
/// its document closes, every call throws `instance-closed`.
public struct SearchCapability: Sendable {
  let handle: SearchHandle
  let viewer: PDFViewer

  /// Whether a search would be served now. `nil` or `.rects` needs
  /// `doc.text.search`; `.full` also `doc.text.copy`.
  public func canSearch(mode: SearchMode? = nil) async throws(PDFError) -> Bool {
    try await bridged { try await handle.canSearch(mode: mode) }
  }

  /// Start a new search, superseding a running one. Returns when the scan
  /// completes, is superseded or is cancelled (cancelling the task cancels
  /// the scan and keeps the hits found so far). An empty `query.text` clears
  /// the session.
  public func search(_ query: SearchQuery, startPage: PageRef? = nil) async throws(PDFError)
    -> SearchResult
  {
    try await measured("search") { () async throws(PDFError) in
      try await bridgedCancellable { token in
        try await handle.search(query: query, startPage: startPage, cancel: token)
      }
    }
  }

  /// Re-run the current query from scratch.
  public func refresh() async throws(PDFError) -> SearchResult {
    try await bridgedCancellable { token in try await handle.refresh(cancel: token) }
  }

  /// Stop the scan and keep the hits found so far.
  public func cancel() async throws(PDFError) {
    try await bridged { try await handle.cancel() }
  }

  /// Stop the scan and drop the session.
  public func clear() async throws(PDFError) {
    try await bridged { try await handle.clear() }
  }

  public func nextHit(reveal: SearchRevealOptions? = nil) async throws(PDFError) -> SearchHit? {
    try await bridged { try await handle.nextHit(reveal: reveal) }
  }

  public func previousHit(reveal: SearchRevealOptions? = nil) async throws(PDFError) -> SearchHit? {
    try await bridged { try await handle.previousHit(reveal: reveal) }
  }

  /// Wraps modulo the hit count.
  public func goToHit(_ index: UInt32, reveal: SearchRevealOptions? = nil) async throws(PDFError)
    -> SearchHit?
  {
    try await bridged { try await handle.goToHit(index: index, reveal: reveal) }
  }

  public func revealActiveHit(reveal: SearchRevealOptions? = nil) async throws(PDFError) {
    try await bridged { try await handle.revealActiveHit(reveal: reveal) }
  }

  public func query() async throws(PDFError) -> SearchQuery? {
    try await bridged { try await handle.query() }
  }

  public func status() async throws(PDFError) -> SearchStatus {
    try await bridged { try await handle.status() }
  }

  /// Hits found so far, or those on one page.
  public func hits(on page: PageRef? = nil) async throws(PDFError) -> [SearchHit] {
    try await measured("hits") { () async throws(PDFError) in
      try await bridged { try await handle.hits(page: page) }
    }
  }

  public func hitCount(on page: PageRef? = nil) async throws(PDFError) -> UInt32 {
    try await bridged { try await handle.hitCount(page: page) }
  }

  public func pagesWithHits() async throws(PDFError) -> [PageRef] {
    try await bridged { try await handle.pagesWithHits() }
  }

  /// `nil` when no hit is active.
  public func activeHitIndex() async throws(PDFError) -> UInt32? {
    try await bridged { try await handle.activeHitIndex() }
  }

  public func activeHit() async throws(PDFError) -> SearchHit? {
    try await bridged { try await handle.activeHit() }
  }

  public func progress() async throws(PDFError) -> SearchProgress {
    try await bridged { try await handle.progress() }
  }

  /// The failure that ended the last search, if it failed.
  public func error() async throws(PDFError) -> PluginErrorInfo? {
    try await bridged { try await handle.error() }
  }

  /// Every hit, in page order, touching no session state. Cancelling the
  /// task throws `operation-cancelled`.
  public func findAll(_ query: SearchQuery, mode: SearchMode? = nil) async throws(PDFError)
    -> [SearchHit]
  {
    try await bridgedCancellable { token in
      try await handle.findAll(query: query, mode: mode, cancel: token)
    }
  }

  /// Session events. Finishes when the document closes (at once if it
  /// already has) or the viewer is destroyed.
  public var events: AsyncStream<SearchEvent> {
    viewer.registry.stream { handle.subscribe(listener: $0) }
  }
}

extension EventRelay: SearchEventListener where Event == SearchEvent {
  func onEvent(event: SearchEvent) {
    continuation.yield(event)
  }

  func onEnd() {
    continuation.finish()
  }
}

extension PDFViewer {
  public func search(in document: PDFDocument) async throws(PDFError) -> SearchCapability {
    let handle = try await bridged { try await host.search(documentId: document.id) }
    return SearchCapability(handle: handle, viewer: self)
  }
}
