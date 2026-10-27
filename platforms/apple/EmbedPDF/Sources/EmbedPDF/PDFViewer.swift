public import EmbedPDFBindings
import Foundation

/// Owns a PDF workspace, its documents, and the configured plugin capabilities.
///
/// Create a viewer with ``ViewerConfig``, await ``start()``, and then open PDF
/// bytes with ``open(_:name:password:)``. Despite its name, this type supplies
/// no UI. Import `EmbedPDFUI` for SwiftUI views or build your own presentation.
///
/// The viewer is `Sendable`: calls reach a dedicated Rust kernel thread, and
/// the native engine runs on its own worker. This does not make application
/// UI state safe to update outside the main actor. Document capabilities
/// retain this viewer; release them when their document is no longer needed.
/// Await ``destroy()`` to close documents and end the workspace explicitly.
///
/// ## Topics
///
/// ### Workspace lifecycle
/// - ``init(_:)``
/// - ``start()``
/// - ``destroy()``
///
/// ### Documents
/// - ``open(_:name:password:)``
/// - ``unlock(_:password:)``
/// - ``documents()``
/// - ``pages(of:)``
/// - ``save(_:mode:)``
/// - ``close(_:)``
///
/// ### Observation
/// - ``documentEvents``
/// - ``stateRevisions``
/// - ``reportedErrors``
public final class PDFViewer: Sendable {
  let host: ViewerHost
  let registry = StreamRegistry()
  private let stateContinuation: AsyncStream<UInt64>.Continuation
  private let errorContinuation: AsyncStream<PDFError>.Continuation

  /// Errors reported by asynchronous callbacks and operations without a throwing return path.
  ///
  /// Examples include listener failures and queued stage camera commands.
  /// This stream buffers the newest 16 errors and supports one consumer.
  /// Errors thrown by awaited calls still need handling at the call site.
  /// The stream finishes after successful destruction or deinitialization.
  public let reportedErrors: AsyncStream<PDFError>

  /// State revision notifications, buffering only the newest revision.
  ///
  /// Use a revision as a signal to read current state rather than as a log of
  /// every intermediate change. The stream supports one consumer and finishes
  /// after successful destruction or deinitialization.
  public let stateRevisions: AsyncStream<UInt64>

  /// Creates the workspace and its configured plugins without starting them.
  ///
  /// - Parameter config: Plugin selection and document access scope. Omitted
  ///   plugins remain unavailable for this viewer's lifetime.
  /// - Throws: A ``PDFError`` if configuration or plugin dependency validation
  ///   fails. Call ``start()`` before using the created workspace.
  public init(_ config: ViewerConfig) throws(PDFError) {
    let (stream, continuation) = AsyncStream<UInt64>.makeStream(bufferingPolicy: .bufferingNewest(1))
    let (errors, errorContinuation) = AsyncStream<PDFError>.makeStream(bufferingPolicy: .bufferingNewest(16))
    do {
      host = try ViewerHost(config: config, delegate: StateRelay(continuation, errors: errorContinuation))
    } catch {
      throw PDFError(error)
    }
    stateRevisions = stream
    stateContinuation = continuation
    reportedErrors = errors
    self.errorContinuation = errorContinuation
  }

  /// Starts workspace plugins and waits for initialization to finish.
  ///
  /// Repeated calls on a usable viewer join the same startup operation.
  /// A destroyed or failed workspace cannot be restarted; create a new viewer.
  ///
  /// - Throws: A ``PDFError`` if plugin initialization fails or the viewer
  ///   has shut down.
  public func start() async throws(PDFError) {
    try await bridged { try await host.start() }
  }

  /// Closes all documents, disposes plugins, stops the kernel, and finishes streams.
  ///
  /// Await this at the end of the workspace's lifetime. Existing document
  /// capabilities become unusable. After this call succeeds, further awaited
  /// operations, including another call to this method, throw `instance-closed`.
  ///
  /// - Throws: A ``PDFError`` if the host has already shut down.
  public func destroy() async throws(PDFError) {
    try await bridged { try await host.destroy() }
    host.shutdown()
    finishStreams()
  }

  /// Opens PDF bytes in a new active document tab.
  ///
  /// Opening an encrypted PDF without a password can succeed with a `.locked`
  /// status. Call ``unlock(_:password:)`` before requesting its capabilities.
  /// A failed open can leave a `.failed` tab in ``documents()``; close that tab
  /// when the application dismisses the failure.
  ///
  /// - Parameters:
  ///   - data: The complete PDF file. The viewer does not fetch URLs or manage
  ///     security-scoped file access for the application.
  ///   - name: An optional display name, such as the source file's last path component.
  ///   - password: The document password, if known.
  /// - Returns: A snapshot identifying the new document and its status.
  /// - Throws: A ``PDFError`` for an invalid PDF, refused access, or a closed
  ///   workspace. An incorrect supplied password throws `permission-denied`.
  public func open(_ data: Data, name: String? = nil, password: String? = nil) async throws(PDFError) -> DocumentInfo {
    try await measured("open") { () async throws(PDFError) in
      try await bridged {
        try await host.openDocument(bytes: data, options: OpenDocumentOptions(name: name, password: password, activate: true))
      }
    }
  }

  /// Unlock a document that opened `.locked` (password-protected and opened
  /// without one). Throws `permission-denied` for a wrong password (the
  /// detail begins with the engine's `DocPasswordIncorrect`); the tab stays
  /// locked.
  ///
  /// The input snapshot remains unchanged after unlocking. Read ``documents()``
  /// for the updated status and page count.
  ///
  /// - Parameters:
  ///   - document: A locked document belonging to this viewer.
  ///   - password: The password to try. A failed attempt permits another attempt.
  public func unlock(_ document: DocumentInfo, password: String) async throws(PDFError) {
    try await bridged { try await host.unlockDocument(documentId: document.id, password: password) }
  }

  /// Closes a document and releases its plugin sessions.
  ///
  /// Closing an already absent document in a live workspace succeeds. Retained
  /// capabilities for the closed document throw `instance-closed`; reacquire
  /// capabilities after opening a new document instead of reusing old ones.
  ///
  /// - Parameter document: The document to close, identified by its `id`.
  /// - Throws: A ``PDFError`` if the workspace has shut down or closing fails.
  public func close(_ document: DocumentInfo) async throws(PDFError) {
    try await bridged { try await host.closeDocument(documentId: document.id) }
  }

  /// Returns current document snapshots in tab order.
  ///
  /// Includes loading, locked, ready, and failed tabs. Only ready documents
  /// have a populated page count. Previous ``DocumentInfo`` values do not
  /// update automatically when the workspace changes.
  ///
  /// - Throws: A ``PDFError`` if the workspace has shut down.
  public func documents() async throws(PDFError) -> [DocumentInfo] {
    try await bridged { try await host.documents() }
  }

  /// Throws `not-found` for a document the viewer doesn't have.
  ///
  /// - Parameter document: The document whose current page layout to read.
  /// - Returns: Page snapshots in document order. Each `index` is zero-based;
  ///   use its `page` reference as identity across page reordering. A locked
  ///   document has no loaded page layout.
  public func pages(of document: DocumentInfo) async throws(PDFError) -> [PageInfo] {
    try await measured("pages") { () async throws(PDFError) in
      try await bridged { try await host.pages(documentId: document.id) }
    }
  }

  /// The complete document as PDF bytes, for the app to write wherever it
  /// keeps files. `.incremental` (the default) appends the edits after the
  /// original bytes and preserves previously signed byte ranges; `.rewrite` writes a
  /// fresh, smaller file without the revision history, and a signed
  /// document refuses it with `permission-denied`. Needs `doc.download` in
  /// `ViewerConfig.scope`. Cancelling the task throws `operation-cancelled`.
  ///
  /// This method returns bytes; it does not write or replace a file. Preserving
  /// signed bytes does not establish certificate trust or the acceptability of
  /// later edits under a signature's modification policy.
  ///
  /// - Parameters:
  ///   - document: The open document to serialize.
  ///   - mode: Incremental output by default, or a full rewrite for an unsigned document.
  /// - Returns: The complete PDF file, including the original bytes in incremental mode.
  public func save(_ document: DocumentInfo, mode: SaveMode = .incremental) async throws(PDFError) -> Data {
    try await measured("save") { () async throws(PDFError) in
      try await bridgedCancellable { token in
        try await host.save(documentId: document.id, mode: mode, cancel: token)
      }
    }
  }

  /// A new subscription to document lifecycle events, without replaying past events.
  ///
  /// Read this property before the operation whose events you need to observe.
  /// Each access creates an independent stream. Cancelling its consuming task
  /// removes the subscription; successful viewer destruction finishes it.
  public var documentEvents: AsyncStream<DocumentEvent> {
    registry.stream { host.subscribeDocuments(listener: $0) }
  }

  func finishStreams() {
    registry.finishAll()
    stateContinuation.finish()
    errorContinuation.finish()
  }

  deinit {
    host.shutdown()
    finishStreams()
  }
}
