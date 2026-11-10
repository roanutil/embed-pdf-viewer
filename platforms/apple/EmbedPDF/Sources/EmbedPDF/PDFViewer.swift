public import EmbedPDFBindings
import Foundation

public final class PDFViewer: Sendable {
  let host: ViewerHost
  let registry = StreamRegistry()
  private let stateContinuation: AsyncStream<UInt64>.Continuation
  private let errorContinuation: AsyncStream<PDFError>.Continuation
  private let revisionHub: StateRevisionHub
  private let errorHub: ReportedErrorHub
  private let storedReportedErrors: AsyncStream<PDFError>
  private let storedStateRevisions: AsyncStream<UInt64>

  /// Errors the kernel reports rather than throws. Keeps the newest 16. The
  /// stream supports a single consumer: a second `for await` gets nothing.
  @available(*, deprecated, message: "Supports one consumer. Use observeReportedErrors(), which gives every caller its own stream.")
  public var reportedErrors: AsyncStream<PDFError> { storedReportedErrors }

  /// Revisions of the viewer's state. The stream supports a single consumer:
  /// a second `for await` gets nothing.
  @available(*, deprecated, message: "Supports one consumer. Use observeStateRevisions(), which gives every caller its own stream.")
  public var stateRevisions: AsyncStream<UInt64> { storedStateRevisions }

  /// A fresh subscription installed before this method returns. Read an initial
  /// snapshot after subscribing; subsequent revisions invalidate that snapshot.
  /// Revisions coalesce, cancellation affects this consumer only, and the stream
  /// finishes when the viewer is destroyed.
  public func observeStateRevisions() -> AsyncStream<UInt64> { revisionHub.stream() }

  /// A fresh subscription to the errors the kernel reports rather than throws:
  /// a listener that failed, a foreign handler that threw (`HostError`). It is
  /// installed before this method returns and sees only errors reported after
  /// that, so subscribe before the call that might report one. Each consumer
  /// keeps the newest 16, cancellation affects this consumer only, and the
  /// stream finishes when the viewer is destroyed.
  public func observeReportedErrors() -> AsyncStream<PDFError> { errorHub.stream() }

  public init(_ config: ViewerConfig, documentFetcher: any DocumentFetcher = URLSessionDocumentFetcher()) throws(PDFError) {
    let (stream, continuation) = AsyncStream<UInt64>.makeStream(bufferingPolicy: .bufferingNewest(1))
    let (errors, errorContinuation) = AsyncStream<PDFError>.makeStream(bufferingPolicy: .bufferingNewest(16))
    let revisions = StateRevisionHub()
    let reported = ReportedErrorHub()
    do {
      host = try ViewerHost.newWithFetcher(
        config: config,
        delegate: StateRelay(continuation, revisions: revisions, errors: errorContinuation, errorHub: reported),
        fetcher: documentFetcher
      )
    } catch {
      throw PDFError(error)
    }
    storedStateRevisions = stream
    stateContinuation = continuation
    storedReportedErrors = errors
    self.errorContinuation = errorContinuation
    revisionHub = revisions
    errorHub = reported
  }

  public func start() async throws(PDFError) {
    try await bridged { try await host.start() }
  }

  public func destroy() async throws(PDFError) {
    try await bridged { try await host.destroy() }
    host.shutdown()
    finishStreams()
  }

  /// Bytes that are not a PDF fail the open (`operation-failed`, "failed to
  /// open base document") and leave a `.failed` tab. A wrong `password`
  /// does not fail it: the document opens `.locked`, and its `.locked`
  /// event says a password was given.
  public func open(_ data: Data, name: String? = nil, password: String? = nil, activate: Bool = true) async throws(PDFError) -> DocumentInfo {
    try await measured("open") { () async throws(PDFError) in
      try await bridged {
        try await host.openDocument(bytes: data, options: OpenDocumentOptions(name: name, password: password, activate: activate))
      }
    }
  }


  /// Opens a URL through the kernel's loading tab using the native fetcher.
  /// Cancellation closes the tab and cancels its transport. HTTP failures and
  /// non-PDF responses leave a failed tab that can be retried.
  public func open(_ url: URL, name: String? = nil, password: String? = nil, activate: Bool = true) async throws(PDFError) -> DocumentInfo {
    try await bridgedCancellable { token in
      try await host.openUrl(url: url.absoluteString, options: OpenDocumentOptions(name: name, password: password, activate: activate), cancel: token)
    }
  }

  /// Restores a saveLayer artifact over its original PDF base. Cancelling the
  /// task closes its loading tab. The engine validates the artifact and base.
  public func open(base: Data, layer: Data, name: String? = nil, password: String? = nil, activate: Bool = true) async throws(PDFError) -> DocumentInfo {
    try await bridgedCancellable { token in
      try await host.openLayerDocument(base: base, artifact: layer, options: OpenDocumentOptions(name: name, password: password, activate: activate), cancel: token)
    }
  }

  /// Retries a failed source in its tab position without activating it. Use
  /// the returned ready identity: the engine may replace the pending tab id.
  /// Close its loading tab to cancel; task cancellation does not cancel retry.
  public func retry(_ document: DocumentInfo) async throws(PDFError) -> DocumentInfo {
    try await bridged { try await host.retryDocument(documentId: document.id) }
  }

  public func rename(_ document: DocumentInfo, to name: String) async throws(PDFError) {
    try await bridged { try await host.renameDocument(documentId: document.id, name: name) }
  }

  public func closeAll() async throws(PDFError) {
    try await bridged { try await host.closeAllDocuments() }
  }

  /// Includes loading, failed and locked tabs; nil selects the active tab.
  public func document(id: String? = nil) async throws(PDFError) -> DocumentInfo? {
    try await bridged { try await host.document(documentId: id) }
  }

  public func hasDocument(id: String) async throws(PDFError) -> Bool {
    try await bridged { try await host.hasDocument(documentId: id) }
  }

  public func documentCount() async throws(PDFError) -> Int {
    try await bridged { try await host.documentCount() }.toInt("document count")
  }

  @available(*, deprecated, message: "documentCount() returns an Int.")
  @_disfavoredOverload
  public func documentCount() async throws(PDFError) -> UInt64 {
    try await bridged { try await host.documentCount() }
  }

  public func activeDocument() async throws(PDFError) -> DocumentInfo? {
    try await bridged { try await host.activeDocument() }
  }

  public func activeDocumentID() async throws(PDFError) -> String? {
    try await bridged { try await host.activeDocumentId() }
  }

  public func setActive(_ document: DocumentInfo) async throws(PDFError) {
    try await bridged { try await host.setActiveDocument(documentId: document.id) }
  }

  public func documentOrder() async throws(PDFError) -> [String] {
    try await bridged { try await host.documentOrder() }
  }

  /// Must be a permutation of the current order, or throws invalid-input.
  public func setDocumentOrder(_ ids: [String]) async throws(PDFError) {
    try await bridged { try await host.setDocumentOrder(documentIds: ids) }
  }

  /// Clamped into the order.
  public func move(_ document: DocumentInfo, to index: Int) async throws(PDFError) {
    let index: Int64 = try index.toWire("document index")
    try await bridged { try await host.moveDocument(documentId: document.id, toIndex: index) }
  }

  @available(*, deprecated, message: "Pass an Int: move(_:to:) takes an Int index.")
  @_disfavoredOverload
  public func move(_ document: DocumentInfo, to index: Int64) async throws(PDFError) {
    try await bridged { try await host.moveDocument(documentId: document.id, toIndex: index) }
  }

  public func swap(_ first: DocumentInfo, _ second: DocumentInfo) async throws(PDFError) {
    try await bridged { try await host.swapDocuments(first: first.id, second: second.id) }
  }

  /// Uses the active document when none is supplied. Missing or closed refs
  /// return nil, rather than the list-pages not-found error.
  public func page(_ page: PageRef, in document: DocumentInfo? = nil) async throws(PDFError) -> PageInfo? {
    try await bridged { try await host.page(page: page, documentId: document?.id) }
  }

  /// `nil` for an index past the last page, or below zero.
  public func page(at index: Int, in document: DocumentInfo? = nil) async throws(PDFError) -> PageInfo? {
    let index: Int64 = try index.toWire("page index")
    return try await bridged { try await host.pageAt(index: index, documentId: document?.id) }
  }

  @available(*, deprecated, message: "Pass an Int: page(at:in:) takes an Int index.")
  @_disfavoredOverload
  public func page(at index: Int64, in document: DocumentInfo? = nil) async throws(PDFError) -> PageInfo? {
    try await bridged { try await host.pageAt(index: index, documentId: document?.id) }
  }

  /// Returns -1 for a missing page or document.
  public func index(of page: PageRef, in document: DocumentInfo? = nil) async throws(PDFError) -> Int {
    try await bridged { try await host.pageIndex(page: page, documentId: document?.id) }.toInt("page index")
  }

  @available(*, deprecated, message: "index(of:in:) returns an Int.")
  @_disfavoredOverload
  public func index(of page: PageRef, in document: DocumentInfo? = nil) async throws(PDFError) -> Int64 {
    try await bridged { try await host.pageIndex(page: page, documentId: document?.id) }
  }

  /// Page-list revision, or -1 when the document is absent.
  public func pageRevision(in document: DocumentInfo? = nil) async throws(PDFError) -> Int64 {
    try await bridged { try await host.pageRevision(documentId: document?.id) }
  }

  public func allows(_ capability: DocumentCapability, in document: DocumentInfo? = nil) async throws(PDFError) -> Bool {
    try await bridged { try await host.allows(capability: capability.rawValue, documentId: document?.id) }
  }

  @available(*, deprecated, message: "Pass a DocumentCapability: .download for \"doc.download\", or DocumentCapability(rawValue:) for a string.")
  public func allows(_ capability: String, in document: DocumentInfo? = nil) async throws(PDFError) -> Bool {
    try await allows(DocumentCapability(rawValue: capability), in: document)
  }

  public func canDownload(_ document: DocumentInfo? = nil) async throws(PDFError) -> Bool {
    try await bridged { try await host.canDownload(documentId: document?.id) }
  }

  /// An editing-layer artifact, requiring its original base to restore.
  /// Use save for a standalone PDF. Requires doc.download.
  public func saveLayer(_ document: DocumentInfo) async throws(PDFError) -> Data {
    try await bridgedCancellable { token in
      try await host.saveLayer(documentId: document.id, cancel: token)
    }
  }

  /// Presentation hints for native chrome. CSS strings are returned for the
  /// app to interpret; they do not paint UI or change engine authorization.
  public func settings() async throws(PDFError) -> ViewerSettings {
    try await bridged { try await host.viewerSettings() }
  }

  /// Omitted fields stay unchanged; clear writes null. Observe changes through
  /// observeStateRevisions(). Engine scope is fixed when the viewer is created.
  public func updateSettings(_ patch: ViewerSettingsPatch) async throws(PDFError) {
    try await bridged { try await host.updateViewerSettings(patch: patch) }
  }

  public func resetSettings() async throws(PDFError) {
    try await bridged { try await host.resetViewerSettings() }
  }

  /// Unlock a document that opened `.locked` (password-protected and opened
  /// or with a wrong one). Throws `permission-denied` for a wrong password,
  /// with the engine's own message ("incorrect document password"); the tab
  /// stays locked.
  public func unlock(_ document: DocumentInfo, password: String) async throws(PDFError) {
    try await bridged { try await host.unlockDocument(documentId: document.id, password: password) }
  }

  public func close(_ document: DocumentInfo) async throws(PDFError) {
    try await bridged { try await host.closeDocument(documentId: document.id) }
  }

  public func documents() async throws(PDFError) -> [DocumentInfo] {
    try await bridged { try await host.documents() }
  }

  /// Throws `not-found` for a document the viewer doesn't have.
  public func pages(of document: DocumentInfo) async throws(PDFError) -> [PageInfo] {
    try await measured("pages") { () async throws(PDFError) in
      try await bridged { try await host.pages(documentId: document.id) }
    }
  }

  /// The complete document as PDF bytes, for the app to write wherever it
  /// keeps files. `.incremental` (the default) appends the edits after the
  /// original bytes and keeps every signature valid; `.rewrite` writes a
  /// fresh, smaller file without the revision history, and a signed
  /// document refuses it with `permission-denied`. Needs `doc.download` in
  /// `ViewerConfig.scope`. Cancelling the task throws `operation-cancelled`.
  public func save(_ document: DocumentInfo, mode: SaveMode = .incremental) async throws(PDFError) -> Data {
    try await measured("save") { () async throws(PDFError) in
      try await bridgedCancellable { token in
        try await host.save(documentId: document.id, mode: mode, cancel: token)
      }
    }
  }

  public var documentEvents: AsyncStream<DocumentEvent> {
    registry.stream { host.subscribeDocuments(listener: $0) }
  }

  func finishStreams() {
    registry.finishAll()
    revisionHub.finish()
    errorHub.finish()
    stateContinuation.finish()
    errorContinuation.finish()
  }

  deinit {
    host.shutdown()
    finishStreams()
  }
}
