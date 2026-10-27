public import EmbedPDFBindings

/// Text or a supported regular expression and matching options for a document search.
public typealias SearchQuery = EmbedPDFBindings.SearchQuery
/// One match with a durable page identity, engine character offsets, geometry, and optional snippet.
public typealias SearchHit = EmbedPDFBindings.SearchHit
/// One visual line of a search match in page coordinates.
public typealias TextSegment = EmbedPDFBindings.TextSegment
/// Context around a search hit, present when snippets were served; match offsets count UTF-16 code units.
public typealias SearchSnippet = EmbedPDFBindings.SearchSnippet
/// The state of the current document search session.
public typealias SearchStatus = EmbedPDFBindings.SearchStatus
/// The progress reported while scanning document pages.
public typealias SearchProgress = EmbedPDFBindings.SearchProgress
/// The completion status and hit count of a session search.
public typealias SearchResult = EmbedPDFBindings.SearchResult
/// Whether a search completed, was cancelled, or was superseded.
public typealias SearchResultStatus = EmbedPDFBindings.SearchResultStatus
/// Per-call overrides for revealing a navigated search hit in the stage.
public typealias SearchRevealOptions = EmbedPDFBindings.SearchRevealOptions
/// The horizontal and vertical anchor used to reveal a search hit.
public typealias SearchRevealAnchor = EmbedPDFBindings.SearchRevealAnchor
/// An alignment value for one axis of a search reveal.
public typealias SearchRevealAxis = EmbedPDFBindings.SearchRevealAxis
/// Whether search navigation reveals a hit immediately or smoothly.
public typealias SearchScrollBehavior = EmbedPDFBindings.SearchScrollBehavior
/// Defaults for document search and revealing active hits.
public typealias SearchConfig = EmbedPDFBindings.SearchConfig
/// An event describing search-session progress and active-hit changes.
public typealias SearchEvent = EmbedPDFBindings.SearchEvent
/// An event describing a session search starting.
public typealias SearchStartedEvent = EmbedPDFBindings.SearchStartedEvent
/// An event describing a search progress update.
public typealias SearchProgressChangedEvent = EmbedPDFBindings.SearchProgressChangedEvent
/// An event describing a completed session search.
public typealias SearchCompletedEvent = EmbedPDFBindings.SearchCompletedEvent
/// An event describing a session search cancellation.
public typealias SearchCancelledEvent = EmbedPDFBindings.SearchCancelledEvent
/// The reason a session search stopped before completion.
public typealias SearchCancelReason = EmbedPDFBindings.SearchCancelReason
/// An event describing a session search failure.
public typealias SearchFailedEvent = EmbedPDFBindings.SearchFailedEvent
/// An event describing an active search-hit change.
public typealias SearchActiveHitChangedEvent = EmbedPDFBindings.SearchActiveHitChangedEvent

/// A document's text search: one session (`search`, navigated with
/// `nextHit`, `previousHit` and `goToHit`) and a session-free `findAll`. Get
/// one with `PDFViewer.search(in:)`; the viewer needs
/// `ViewerConfig(search: SearchConfig())`. It keeps the viewer alive. After
/// its document closes, every call throws `instance-closed`.
public struct SearchCapability: Sendable {
  let handle: SearchHandle
  let viewer: PDFViewer

  /// Whether a search would be served now: `doc.text.search`, and with
  /// `snippets: true` also `doc.text.copy`.
  public func canSearch(snippets: Bool? = nil) async throws(PDFError) -> Bool {
    try await bridged { try await handle.canSearch(snippets: snippets) }
  }

  /// Start a new search, superseding a running one. Returns when the scan
  /// completes, is superseded or is cancelled (cancelling the task cancels
  /// the scan and keeps the hits found so far). An empty `query.text` clears
  /// the session. `from` is where the scan starts; `nil` is the stage's
  /// current page when a stage is installed.
  public func search(_ query: SearchQuery, from page: PageRef? = nil) async throws(PDFError)
    -> SearchResult
  {
    try await measured("search") { () async throws(PDFError) in
      try await bridgedCancellable { token in
        try await handle.search(query: query, from: page, cancel: token)
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

  /// Activates the next session hit and optionally reveals it, wrapping through the available
  /// hits; returns nil when there are none.
  public func nextHit(reveal: SearchRevealOptions? = nil) async throws(PDFError) -> SearchHit? {
    try await bridged { try await handle.nextHit(reveal: reveal) }
  }

  /// Activates the previous session hit and optionally reveals it, wrapping through the
  /// available hits; returns nil when there are none.
  public func previousHit(reveal: SearchRevealOptions? = nil) async throws(PDFError) -> SearchHit? {
    try await bridged { try await handle.previousHit(reveal: reveal) }
  }

  /// Wraps modulo the hit count.
  public func goToHit(_ index: UInt32, reveal: SearchRevealOptions? = nil) async throws(PDFError)
    -> SearchHit?
  {
    try await bridged { try await handle.goToHit(index: index, reveal: reveal) }
  }

  /// Reveals the active hit using the configured stage and per-call arrival options.
  public func revealActiveHit(reveal: SearchRevealOptions? = nil) async throws(PDFError) {
    try await bridged { try await handle.revealActiveHit(reveal: reveal) }
  }

  /// Returns the current session query, or nil when the session is clear.
  public func query() async throws(PDFError) -> SearchQuery? {
    try await bridged { try await handle.query() }
  }

  /// Returns the current search session's scan state.
  public func status() async throws(PDFError) -> SearchStatus {
    try await bridged { try await handle.status() }
  }

  /// Hits found so far, or those on one page.
  public func hits(on page: PageRef? = nil) async throws(PDFError) -> [SearchHit] {
    try await measured("hits") { () async throws(PDFError) in
      try await bridged { try await handle.hits(page: page) }
    }
  }

  /// Returns the number of hits found so far, restricted to one page when supplied.
  public func hitCount(on page: PageRef? = nil) async throws(PDFError) -> UInt32 {
    try await bridged { try await handle.hitCount(page: page) }
  }

  /// Returns the page identities on which the current session has found matches.
  public func pagesWithHits() async throws(PDFError) -> [PageRef] {
    try await bridged { try await handle.pagesWithHits() }
  }

  /// `nil` when no hit is active.
  public func activeHitIndex() async throws(PDFError) -> UInt32? {
    try await bridged { try await handle.activeHitIndex() }
  }

  /// Returns the active session match, or nil when no hit is active.
  public func activeHit() async throws(PDFError) -> SearchHit? {
    try await bridged { try await handle.activeHit() }
  }

  /// Returns the current session's page-scanning progress.
  public func progress() async throws(PDFError) -> SearchProgress {
    try await bridged { try await handle.progress() }
  }

  /// The failure that ended the last search, if it failed.
  public func error() async throws(PDFError) -> PluginErrorInfo? {
    try await bridged { try await handle.error() }
  }

  /// Every hit, in page order, touching no session state. `snippets: nil`
  /// asks for snippets and falls back to none when they are denied.
  /// Cancelling the task throws `operation-cancelled`.
  public func findAll(_ query: SearchQuery, snippets: Bool? = nil) async throws(PDFError)
    -> [SearchHit]
  {
    try await bridgedCancellable { token in
      try await handle.findAll(query: query, snippets: snippets, cancel: token)
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
  /// Returns this document's search capability; configure search before requesting it.
  public func search(in document: DocumentInfo) async throws(PDFError) -> SearchCapability {
    let handle = try await bridged { try await host.search(documentId: document.id) }
    return SearchCapability(handle: handle, viewer: self)
  }
}
