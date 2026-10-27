public import EmbedPDFBindings

/// A pointer tool's identity, behaviors, and cursor configuration.
public typealias InteractionTool = EmbedPDFBindings.InteractionTool
/// An event describing tool, gesture, and cursor changes.
public typealias InteractionEvent = EmbedPDFBindings.InteractionEvent
/// An event describing an active pointer-tool change.
public typealias InteractionToolChangedEvent = EmbedPDFBindings.InteractionToolChangedEvent
/// An event describing a pointer gesture.
public typealias InteractionGestureEvent = EmbedPDFBindings.InteractionGestureEvent
/// An event describing a resolved cursor change.
public typealias InteractionCursorChangedEvent = EmbedPDFBindings.InteractionCursorChangedEvent
/// Synchronous kernel-thread callbacks that claim and handle pointer gestures.
public typealias InteractionHandler = EmbedPDFBindings.InteractionHandler
/// A pointer phase, positions, modifiers, and optional page hit sent to the interaction hub.
public typealias PointerSample = EmbedPDFBindings.PointerSample
/// The phase of a pointer contact or hover sample.
public typealias PointerPhase = EmbedPDFBindings.PointerPhase
/// The input device category that produced a pointer sample.
public typealias PointerKind = EmbedPDFBindings.PointerKind
/// The gesture information emitted by the pointer hub.
public typealias PointerGesture = EmbedPDFBindings.PointerGesture
/// A pointer's page identity and page-space location.
public typealias PointerPageHit = EmbedPDFBindings.PointerPageHit
/// The keyboard modifiers active for an input sample.
public typealias Modifiers = EmbedPDFBindings.Modifiers

/// What the hub does when a handler leaves a member out.
extension InteractionHandler {
  /// Handles a captured pointer move; the default implementation does nothing.
  public func onMove(sample: PointerSample) {}
  /// Handles the end of a captured pointer contact; the default implementation does nothing.
  public func onUp(sample: PointerSample) {}
  /// Handles cancellation by forwarding the sample to ``onUp(sample:)`` by default.
  public func onCancel(sample: PointerSample) { onUp(sample: sample) }
  /// Handles a hover sample; the default implementation does nothing.
  public func onHover(sample: PointerSample) {}
  /// Returns false by default, allowing the host's touch handling to proceed.
  public func claimsTouch(sample: PointerSample) -> Bool { false }
}

/// A document's pointer hub: the active tool, the cursor, and the handlers
/// gestures route to. Get one with `PDFViewer.interaction(for:)`; the
/// viewer needs `ViewerConfig(interaction: true)`. It keeps the viewer
/// alive. After its document closes, every call throws `instance-closed`.
public struct InteractionCapability: Sendable {
  let handle: InteractionHandle
  let viewer: PDFViewer

  /// Returns the currently active tool's definition.
  public func activeTool() async throws(PDFError) -> InteractionTool {
    try await bridged { try await handle.activeTool() }
  }

  /// Returns the identifier of the currently active tool.
  public func activeToolId() async throws(PDFError) -> String {
    try await bridged { try await handle.activeToolId() }
  }

  /// Returns the identifier of the document's default pointer tool.
  public func defaultToolId() async throws(PDFError) -> String {
    try await bridged { try await handle.defaultToolId() }
  }

  /// Returns the pointer tools currently registered for this document.
  public func tools() async throws(PDFError) -> [InteractionTool] {
    try await bridged { try await handle.tools() }
  }

  /// Returns a tool's definition, or nil for an unknown identifier.
  public func tool(_ id: String) async throws(PDFError) -> InteractionTool? {
    try await bridged { try await handle.tool(id: id) }
  }

  /// Returns whether the supplied tool identifier is registered.
  public func hasTool(_ id: String) async throws(PDFError) -> Bool {
    try await bridged { try await handle.hasTool(id: id) }
  }

  /// Returns whether the active tool enables the named behavior.
  public func activeToolEnables(_ behavior: String) async throws(PDFError) -> Bool {
    try await bridged { try await handle.activeToolEnables(behavior: behavior) }
  }

  /// Throws `not-found` for an id nobody registered.
  public func activateTool(_ id: String) async throws(PDFError) {
    try await bridged { try await handle.activateTool(id: id) }
  }

  /// Activates the document's default pointer tool.
  public func activateDefaultTool() async throws(PDFError) {
    try await bridged { try await handle.activateDefaultTool() }
  }

  /// Temporarily activates a tool while retaining the previous tool for ``popTool()``.
  public func pushTool(_ id: String) async throws(PDFError) {
    try await bridged { try await handle.pushTool(id: id) }
  }

  /// Restores the tool saved by a previous ``pushTool(_:)`` call; does nothing with an empty stack.
  ///
  /// - Throws: `not-found` if the saved tool was unregistered before restoration.
  public func popTool() async throws(PDFError) {
    try await bridged { try await handle.popTool() }
  }

  /// Replace a tool's cursors by name; `nil` brings the tool's own back.
  public func setToolCursor(_ id: String, cursors: [String: String]?) async throws(PDFError) {
    try await bridged { try await handle.setToolCursor(id: id, cursors: cursors) }
  }

  /// Throws `conflict` for an id already registered unless `replace`.
  public func registerTool(_ tool: InteractionTool, replace: Bool = false) async throws(PDFError)
    -> Registration
  {
    Registration(handle: try await bridged { try await handle.registerTool(tool: tool, replace: replace) })
  }

  /// `handler` is called on the viewer's kernel thread, synchronously,
  /// for every sample. Return quickly, and don't block waiting on the
  /// viewer from inside it.
  ///
  /// The viewer keeps `handler` until you call `unregister()` or the
  /// document closes. A handler that holds this capability or the
  /// `PDFViewer` keeps the viewer alive until then, as a closure that
  /// captures its owner does.
  public func registerHandler(
    _ handler: some InteractionHandler,
    id: String,
    priority: Double,
    source: String? = nil
  ) async throws(PDFError) -> Registration {
    Registration(
      handle: try await bridged {
        try await handle.registerHandler(id: id, priority: priority, source: source, handler: handler)
      })
  }

  /// `nil` clears the claim.
  public func claimCursor(_ token: String, cursor: String?, priority: Double = 0) async throws(PDFError) {
    try await bridged { try await handle.claimCursor(token: token, cursor: cursor, priority: priority) }
  }

  /// Returns the cursor resolved from the current tool and active cursor claims.
  public func cursor() async throws(PDFError) -> String {
    try await bridged { try await handle.cursor() }
  }

  /// Checks whether an eligible handler would claim the touch sample without dispatching it.
  public func wouldClaimTouch(_ sample: PointerSample) async throws(PDFError) -> Bool {
    try await bridged { try await handle.wouldClaimTouch(sample: sample) }
  }

  /// Returns at once. Samples run in the order they were sent.
  public func dispatchPointer(_ sample: PointerSample) {
    handle.dispatchPointer(sample: sample)
  }

  /// Finishes when the document closes (at once if it already has) or the
  /// viewer is destroyed.
  public var events: AsyncStream<InteractionEvent> {
    viewer.registry.stream { handle.subscribe(listener: $0) }
  }
}

extension EventRelay: InteractionEventListener where Event == InteractionEvent {
  func onEvent(event: InteractionEvent) {
    continuation.yield(event)
  }

  func onEnd() {
    continuation.finish()
  }
}

extension PDFViewer {
  /// Returns this document's pointer hub; enable interaction in the viewer configuration
  /// before requesting it.
  public func interaction(for document: DocumentInfo) async throws(PDFError) -> InteractionCapability {
    let handle = try await bridged { try await host.interaction(documentId: document.id) }
    return InteractionCapability(handle: handle, viewer: self)
  }
}
