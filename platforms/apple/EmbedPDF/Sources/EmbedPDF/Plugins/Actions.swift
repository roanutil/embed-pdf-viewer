public import EmbedPDFBindings
import Foundation

public typealias ActionsConfig = EmbedPDFBindings.ActionsConfig
public typealias ActionsJavaScriptConfig = EmbedPDFBindings.ActionsJavaScriptConfig
public typealias ActionsScriptBudget = EmbedPDFBindings.ActionsScriptBudget
public typealias ActionPolicy = EmbedPDFBindings.ActionPolicy
public typealias ActionPolicyRow = EmbedPDFBindings.ActionPolicyRow
public typealias ActionPolicyPatch = EmbedPDFBindings.ActionPolicyPatch
public typealias PartialActionPolicyRow = EmbedPDFBindings.PartialActionPolicyRow
public typealias ActionPolicyDecision = EmbedPDFBindings.ActionPolicyDecision
public typealias TriggerGates = EmbedPDFBindings.TriggerGates
public typealias OpenSequence = EmbedPDFBindings.OpenSequence
public typealias PdfActionTree = EmbedPDFBindings.PdfActionTree
public typealias PdfActionNode = EmbedPDFBindings.PdfActionNode
public typealias PdfObjectRef = EmbedPDFBindings.PdfObjectRef
public typealias ActionOrigin = EmbedPDFBindings.ActionOrigin
public typealias ActionSource = EmbedPDFBindings.ActionSource
public typealias ActionContext = EmbedPDFBindings.ActionContext
public typealias PartialActionContext = EmbedPDFBindings.PartialActionContext
public typealias ActionTriggerEvent = EmbedPDFBindings.ActionTriggerEvent
public typealias ActionTrigger = EmbedPDFBindings.ActionTrigger
public typealias ActionTreeSource = EmbedPDFBindings.ActionTreeSource
public typealias ActionAnnotationTreeEvent = EmbedPDFBindings.ActionAnnotationTreeEvent
public typealias DocumentTriggerEvent = EmbedPDFBindings.DocumentTriggerEvent
public typealias PdfAnnotationEventKind = EmbedPDFBindings.PdfAnnotationEventKind
public typealias PageEventKind = EmbedPDFBindings.PageEventKind
public typealias PdfFieldEventKind = EmbedPDFBindings.PdfFieldEventKind
public typealias ActionNodeStatus = EmbedPDFBindings.ActionNodeStatus
public typealias ActionNodeResult = EmbedPDFBindings.ActionNodeResult
public typealias ActionResultStatus = EmbedPDFBindings.ActionResultStatus
public typealias ActionDispatchResult = EmbedPDFBindings.ActionDispatchResult
public typealias ActionStepResult = EmbedPDFBindings.ActionStepResult
public typealias ActionTriggerResult = EmbedPDFBindings.ActionTriggerResult
public typealias ActionDiagnostic = EmbedPDFBindings.ActionDiagnostic
public typealias ActionDiagnosticCode = EmbedPDFBindings.ActionDiagnosticCode
public typealias ActionDispatchEvent = EmbedPDFBindings.ActionDispatchEvent
public typealias OpenSequenceCompleted = EmbedPDFBindings.OpenSequenceCompleted
public typealias ScriptDiagnostic = EmbedPDFBindings.ScriptDiagnostic
public typealias ScriptDiagnosticCode = EmbedPDFBindings.ScriptDiagnosticCode
public typealias ScriptExecutionError = EmbedPDFBindings.ScriptExecutionError
public typealias ScriptErrorKind = EmbedPDFBindings.ScriptErrorKind
public typealias ActionsEvent = EmbedPDFBindings.ActionsEvent
public typealias ActionExecutor = EmbedPDFBindings.ActionExecutor
public typealias ActionExecutorCompletion = EmbedPDFBindings.ActionExecutorCompletion
public typealias ActionUiHandler = EmbedPDFBindings.ActionUiHandler
public typealias ActionUiSupport = EmbedPDFBindings.ActionUiSupport
public typealias ActionUiContext = EmbedPDFBindings.ActionUiContext
public typealias ActionUriOptions = EmbedPDFBindings.ActionUriOptions
public typealias ActionAlertOptions = EmbedPDFBindings.ActionAlertOptions
public typealias ScriptPhase = EmbedPDFBindings.ScriptPhase
public typealias ActionSubmitHandler = EmbedPDFBindings.ActionSubmitHandler
public typealias ActionSubmitRequest = EmbedPDFBindings.ActionSubmitRequest
public typealias FormSubmissionEntry = EmbedPDFBindings.FormSubmissionEntry
public typealias FormSubmissionValue = EmbedPDFBindings.FormSubmissionValue
public typealias DocumentVerb = EmbedPDFBindings.DocumentVerb

extension PdfActionTree {
  /// A tree in the engine's shape: `{root, incomplete, warningFlags, warnings}`.
  public static func decode(json: String) throws(PDFError) -> PdfActionTree {
    do {
      return try PdfActionTree.fromJson(json: json)
    } catch {
      throw PDFError(error)
    }
  }
}

extension ActionsConfig {
  /// The config with `utcOffsetMinutes` set from the current time zone. The
  /// offset is fixed for the viewer's lifetime.
  public static func forCurrentTimeZone(
    policy: ActionPolicyPatch? = nil,
    triggers: TriggerGates? = nil,
    openSequence: OpenSequence? = nil,
    javascript: ActionsJavaScriptConfig? = nil
  ) -> ActionsConfig {
    ActionsConfig(
      policy: policy, triggers: triggers, openSequence: openSequence, javascript: javascript,
      utcOffsetMinutes: Int32(TimeZone.current.secondsFromGMT() / 60)
    )
  }
}

extension DocumentVerbCompletion {
  /// Fail the verb with `error`, keeping its code (see `HostFailure.init(_:)`).
  public func fail(_ error: any Error) {
    failWith(failure: HostFailure(error))
  }
}

final class ClosureDocumentVerb: DocumentVerbOperation, Sendable {
  let body: @Sendable () async throws -> Void

  init(_ body: @escaping @Sendable () async throws -> Void) {
    self.body = body
  }

  func run(completion: DocumentVerbCompletion) {
    let body = body
    Task {
      do {
        try await body()
        completion.finish()
      } catch {
        completion.fail(error)
      }
    }
  }
}

/// A document's PDF actions: policy, the dispatcher and its ports. Get one
/// with `PDFViewer.actions(for:)`; the viewer needs
/// `ViewerConfig(actions: ActionsConfig())`. It keeps the viewer alive.
/// After its document closes, every call throws `instance-closed`. No call
/// is cancellable: a dispatch is a transaction on the document's queue.
public struct ActionsCapability: Sendable {
  let handle: ActionsHandle
  let viewer: PDFViewer

  public func policy() async throws(PDFError) -> ActionPolicy {
    try await bridged { try await handle.policy() }
  }

  public func updatePolicy(_ patch: ActionPolicyPatch) async throws(PDFError) {
    try await bridged { try await handle.updatePolicy(patch: patch) }
  }

  public func isScriptingEnabled() async throws(PDFError) -> Bool {
    try await bridged { try await handle.isScriptingEnabled() }
  }

  public func execute(_ tree: PdfActionTree, context: ActionContext) async throws(PDFError) -> ActionDispatchResult {
    try await bridged { try await handle.execute(tree: tree, context: context) }
  }

  public func canExecute(_ tree: PdfActionTree, context: ActionContext) async throws(PDFError) -> Bool {
    try await bridged { try await handle.canExecute(tree: tree, context: context) }
  }

  public func executeNamed(_ name: String, context: PartialActionContext? = nil)
    async throws(PDFError) -> ActionDispatchResult
  {
    try await bridged { try await handle.executeNamed(name: name, context: context) }
  }

  public func actionTree(for source: ActionTreeSource) async throws(PDFError) -> PdfActionTree? {
    try await bridged { try await handle.actionTree(source: source) }
  }

  public func dispatch(_ trigger: ActionTrigger) async throws(PDFError) -> ActionTriggerResult {
    try await bridged { try await handle.dispatch(trigger: trigger) }
  }

  public func canDispatch(_ trigger: ActionTrigger) async throws(PDFError) -> Bool {
    try await bridged { try await handle.canDispatch(trigger: trigger) }
  }

  /// Runs the document's will-close trees. Call before `PDFViewer.close`.
  public func prepareClose() async throws(PDFError) -> ActionTriggerResult {
    try await bridged { try await handle.prepareClose() }
  }

  /// Last wins per node type.
  public func registerExecutor(for actionType: String, _ executor: any ActionExecutor)
    async throws(PDFError) -> Registration
  {
    Registration(handle: try await bridged { try await handle.registerExecutor(actionType: actionType, executor: executor) })
  }

  /// `nil` uninstalls. Installing one releases an `auto` open sequence.
  public func setUIHandler(_ handler: (any ActionUiHandler)?, support: ActionUiSupport = ActionUiSupport())
    async throws(PDFError) -> Registration
  {
    Registration(handle: try await bridged { try await handle.setUiAdapter(handler: handler, support: support) })
  }

  public func setSubmitHandler(_ handler: (any ActionSubmitHandler)?) async throws(PDFError) -> Registration {
    Registration(handle: try await bridged { try await handle.setSubmitHandler(handler: handler) })
  }

  /// The document's will- trees, then `operation`, then its did- trees.
  public func runDocumentVerb(_ verb: DocumentVerb, operation: @escaping @Sendable () async throws -> Void)
    async throws(PDFError)
  {
    try await bridged { try await handle.runDocumentVerb(verb: verb, operation: ClosureDocumentVerb(operation)) }
  }

  /// Finishes when the document closes (at once if it already has) or the
  /// viewer is destroyed.
  public var events: AsyncStream<ActionsEvent> {
    viewer.registry.stream { handle.subscribe(listener: $0) }
  }
}

extension EventRelay: ActionsEventListener where Event == ActionsEvent {
  func onEvent(event: ActionsEvent) {
    continuation.yield(event)
  }

  func onEnd() {
    continuation.finish()
  }
}

extension PDFViewer {
  public func actions(for document: PDFDocument) async throws(PDFError) -> ActionsCapability {
    let handle = try await bridged { try await host.actions(documentId: document.id) }
    return ActionsCapability(handle: handle, viewer: self)
  }
}
