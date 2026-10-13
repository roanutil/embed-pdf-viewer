public import EmbedPDFBindings

public typealias InteractionTool = EmbedPDFBindings.InteractionTool
public typealias InteractionEvent = EmbedPDFBindings.InteractionEvent
public typealias InteractionToolChangedEvent = EmbedPDFBindings.InteractionToolChangedEvent
public typealias InteractionGestureEvent = EmbedPDFBindings.InteractionGestureEvent
public typealias InteractionCursorChangedEvent = EmbedPDFBindings.InteractionCursorChangedEvent
public typealias InteractionHandler = EmbedPDFBindings.InteractionHandler
public typealias PointerSample = EmbedPDFBindings.PointerSample
public typealias PointerPhase = EmbedPDFBindings.PointerPhase
public typealias PointerKind = EmbedPDFBindings.PointerKind
public typealias PointerGesture = EmbedPDFBindings.PointerGesture
public typealias PointerPageHit = EmbedPDFBindings.PointerPageHit
public typealias Modifiers = EmbedPDFBindings.Modifiers

/// What the hub does when a handler leaves a member out.
extension InteractionHandler {
  public func onMove(sample: PointerSample) {}
  public func onUp(sample: PointerSample) {}
  public func onCancel(sample: PointerSample) { onUp(sample: sample) }
  public func onHover(sample: PointerSample) {}
  public func claimsTouch(sample: PointerSample) -> Bool { false }
}

/// A document's pointer hub: the active tool, the cursor, and the handlers
/// gestures route to. Get one with `PDFViewer.interaction(for:)`; the
/// viewer needs `ViewerConfig(interaction: true)`. It keeps the viewer
/// alive. After its document closes, every call throws `instance-closed`.
public struct InteractionCapability: Sendable {
  let handle: InteractionHandle
  let viewer: PDFViewer

  public func activeTool() async throws(PDFError) -> InteractionTool {
    try await bridged { try await handle.activeTool() }
  }

  public func activeToolId() async throws(PDFError) -> String {
    try await bridged { try await handle.activeToolId() }
  }

  public func defaultToolId() async throws(PDFError) -> String {
    try await bridged { try await handle.defaultToolId() }
  }

  public func tools() async throws(PDFError) -> [InteractionTool] {
    try await bridged { try await handle.tools() }
  }

  public func tool(_ id: String) async throws(PDFError) -> InteractionTool? {
    try await bridged { try await handle.tool(id: id) }
  }

  public func hasTool(_ id: String) async throws(PDFError) -> Bool {
    try await bridged { try await handle.hasTool(id: id) }
  }

  public func activeToolEnables(_ behavior: String) async throws(PDFError) -> Bool {
    try await bridged { try await handle.activeToolEnables(behavior: behavior) }
  }

  /// Throws `not-found` for an id nobody registered.
  public func activateTool(_ id: String) async throws(PDFError) {
    try await bridged { try await handle.activateTool(id: id) }
  }

  public func activateDefaultTool() async throws(PDFError) {
    try await bridged { try await handle.activateDefaultTool() }
  }

  public func pushTool(_ id: String) async throws(PDFError) {
    try await bridged { try await handle.pushTool(id: id) }
  }

  public func popTool() async throws(PDFError) {
    try await bridged { try await handle.popTool() }
  }

  /// `nil` removes the skin.
  public func setToolCursor(_ id: String, skin: [String: String]?) async throws(PDFError) {
    try await bridged { try await handle.setToolCursor(id: id, skin: skin) }
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

  public func cursor() async throws(PDFError) -> String {
    try await bridged { try await handle.cursor() }
  }

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
  public func interaction(for document: PDFDocument) async throws(PDFError) -> InteractionCapability {
    let handle = try await bridged { try await host.interaction(documentId: document.id) }
    return InteractionCapability(handle: handle, viewer: self)
  }
}
