public import EmbedPDFBindings
import Foundation

/// Configuration that installs PDF action dispatch, policy, triggers, and optional scripting.
public typealias ActionsConfig = EmbedPDFBindings.ActionsConfig
/// Settings and engine factory for executing document JavaScript.
public typealias ActionsJavaScriptConfig = EmbedPDFBindings.ActionsJavaScriptConfig
/// Execution limits requested for a document script.
public typealias ActionsScriptBudget = EmbedPDFBindings.ActionsScriptBudget
/// The effective policy used to admit or block PDF actions.
public typealias ActionPolicy = EmbedPDFBindings.ActionPolicy
/// Policy decisions for one action category and its origins.
public typealias ActionPolicyRow = EmbedPDFBindings.ActionPolicyRow
/// A partial update to the effective action policy.
public typealias ActionPolicyPatch = EmbedPDFBindings.ActionPolicyPatch
/// Selected decisions to replace within an action policy row.
public typealias PartialActionPolicyRow = EmbedPDFBindings.PartialActionPolicyRow
/// The policy decision for an action at dispatch time.
public typealias ActionPolicyDecision = EmbedPDFBindings.ActionPolicyDecision
/// Settings that enable or suppress automatic PDF action triggers.
public typealias TriggerGates = EmbedPDFBindings.TriggerGates
/// When the document's opening action sequence may run.
public typealias OpenSequence = EmbedPDFBindings.OpenSequence
/// An engine-decoded PDF action tree, including incomplete-read warnings.
public typealias PdfActionTree = EmbedPDFBindings.PdfActionTree
/// One PDF action and its chained child actions.
public typealias PdfActionNode = EmbedPDFBindings.PdfActionNode
/// An indirect PDF object's number and generation.
public typealias PdfObjectRef = EmbedPDFBindings.PdfObjectRef
/// The origin used when evaluating an action's policy.
public typealias ActionOrigin = EmbedPDFBindings.ActionOrigin
/// The document location from which an action originated.
public typealias ActionSource = EmbedPDFBindings.ActionSource
/// The trigger and source context supplied to an action dispatcher.
public typealias ActionContext = EmbedPDFBindings.ActionContext
/// Optional context overrides for a named action.
public typealias PartialActionContext = EmbedPDFBindings.PartialActionContext
/// An event describing a dispatched PDF action trigger.
public typealias ActionTriggerEvent = EmbedPDFBindings.ActionTriggerEvent
/// A document, page, annotation, or field trigger to dispatch.
public typealias ActionTrigger = EmbedPDFBindings.ActionTrigger
/// A location in the document from which to retrieve an action tree.
public typealias ActionTreeSource = EmbedPDFBindings.ActionTreeSource
/// An event describing an annotation action tree.
public typealias ActionAnnotationTreeEvent = EmbedPDFBindings.ActionAnnotationTreeEvent
/// An event describing a document action trigger.
public typealias DocumentTriggerEvent = EmbedPDFBindings.DocumentTriggerEvent
/// The annotation event whose action tree is requested.
public typealias PdfAnnotationEventKind = EmbedPDFBindings.PdfAnnotationEventKind
/// The page lifecycle event whose action tree is requested.
public typealias PageEventKind = EmbedPDFBindings.PageEventKind
/// The field event whose action tree is requested.
public typealias PdfFieldEventKind = EmbedPDFBindings.PdfFieldEventKind
/// The execution outcome of one action node.
public typealias ActionNodeStatus = EmbedPDFBindings.ActionNodeStatus
/// An action node's status and diagnostic information.
public typealias ActionNodeResult = EmbedPDFBindings.ActionNodeResult
/// The overall outcome of dispatching an action tree.
public typealias ActionResultStatus = EmbedPDFBindings.ActionResultStatus
/// The dispatch outcome and results of individual action nodes.
public typealias ActionDispatchResult = EmbedPDFBindings.ActionDispatchResult
/// The outcome of one step within a trigger sequence.
public typealias ActionStepResult = EmbedPDFBindings.ActionStepResult
/// The outcomes of the action trees associated with a trigger.
public typealias ActionTriggerResult = EmbedPDFBindings.ActionTriggerResult
/// A reason an action was rejected, skipped, or failed.
public typealias ActionDiagnostic = EmbedPDFBindings.ActionDiagnostic
/// A machine-readable category for an action diagnostic.
public typealias ActionDiagnosticCode = EmbedPDFBindings.ActionDiagnosticCode
/// An event describing completion of the document opening sequence.
public typealias OpenSequenceCompleted = EmbedPDFBindings.OpenSequenceCompleted
/// Diagnostic information reported by document scripting.
public typealias ScriptDiagnostic = EmbedPDFBindings.ScriptDiagnostic
/// A machine-readable category for a scripting diagnostic.
public typealias ScriptDiagnosticCode = EmbedPDFBindings.ScriptDiagnosticCode
/// Details of a JavaScript execution failure.
public typealias ScriptExecutionError = EmbedPDFBindings.ScriptExecutionError
/// The category of a JavaScript execution failure.
public typealias ScriptErrorKind = EmbedPDFBindings.ScriptErrorKind
/// An event describing PDF action dispatch and scripting.
public typealias ActionsEvent = EmbedPDFBindings.ActionsEvent
/// A host implementation that executes a registered PDF action type and completes its callback.
public typealias ActionExecutor = EmbedPDFBindings.ActionExecutor
/// The completion callback an action executor must finish or fail.
public typealias ActionExecutorCompletion = EmbedPDFBindings.ActionExecutorCompletion
/// The host callbacks for action-driven UI, including URLs, alerts, printing, and navigation.
public typealias ActionUiHandler = EmbedPDFBindings.ActionUiHandler
/// The UI operations a host action handler advertises as supported.
public typealias ActionUiSupport = EmbedPDFBindings.ActionUiSupport
/// Document action context supplied to a host UI callback.
public typealias ActionUiContext = EmbedPDFBindings.ActionUiContext
/// Options supplied when a PDF action asks the host to open a URI.
public typealias ActionUriOptions = EmbedPDFBindings.ActionUriOptions
/// Presentation options supplied with a PDF script alert.
public typealias ActionAlertOptions = EmbedPDFBindings.ActionAlertOptions
/// The stage of a document script's execution.
public typealias ScriptPhase = EmbedPDFBindings.ScriptPhase
/// The host callback that handles PDF form submission requests.
public typealias ActionSubmitHandler = EmbedPDFBindings.ActionSubmitHandler
/// A form submission target, options, and collected values for the host to send.
public typealias ActionSubmitRequest = EmbedPDFBindings.ActionSubmitRequest
/// One named value collected for a PDF form submission.
public typealias FormSubmissionEntry = EmbedPDFBindings.FormSubmissionEntry
/// The value representation of a submitted form field.
public typealias FormSubmissionValue = EmbedPDFBindings.FormSubmissionValue
/// A document operation bracketed by its will- and did-action trees.
public typealias DocumentVerb = EmbedPDFBindings.DocumentVerb

extension PdfActionTree {
  /// Decodes a tree in the engine's shape: `{root, incomplete, warningFlags, warnings}`.
  ///
  /// - Parameter json: UTF-8 JSON text containing the engine action-tree object.
  /// - Throws: A `PDFError` when the JSON cannot be decoded as an action tree.
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

  /// Returns the effective action policy for this document.
  public func policy() async throws(PDFError) -> ActionPolicy {
    try await bridged { try await handle.policy() }
  }

  /// Applies selected policy changes to subsequent action dispatches.
  public func updatePolicy(_ patch: ActionPolicyPatch) async throws(PDFError) {
    try await bridged { try await handle.updatePolicy(patch: patch) }
  }

  /// Returns whether this document's action dispatcher has scripting enabled.
  public func isScriptingEnabled() async throws(PDFError) -> Bool {
    try await bridged { try await handle.isScriptingEnabled() }
  }

  /// Dispatches an action tree using the supplied origin and trigger context; inspect the
  /// returned node results for blocked or failed actions.
  public func execute(_ tree: PdfActionTree, context: ActionContext) async throws(PDFError) -> ActionDispatchResult {
    try await bridged { try await handle.execute(tree: tree, context: context) }
  }

  /// Checks whether the supplied tree can execute in this context without dispatching it.
  public func canExecute(_ tree: PdfActionTree, context: ActionContext) async throws(PDFError) -> Bool {
    try await bridged { try await handle.canExecute(tree: tree, context: context) }
  }

  /// Dispatches a PDF named action with optional context overrides.
  public func executeNamed(_ name: String, context: PartialActionContext? = nil)
    async throws(PDFError) -> ActionDispatchResult
  {
    try await bridged { try await handle.executeNamed(name: name, context: context) }
  }

  /// Returns the tree at the requested document location, or nil when no tree is present.
  public func actionTree(for source: ActionTreeSource) async throws(PDFError) -> PdfActionTree? {
    try await bridged { try await handle.actionTree(source: source) }
  }

  /// Dispatches the action trees associated with a document, page, annotation, or field trigger.
  public func dispatch(_ trigger: ActionTrigger) async throws(PDFError) -> ActionTriggerResult {
    try await bridged { try await handle.dispatch(trigger: trigger) }
  }

  /// Checks whether a trigger can dispatch under the current policy and trigger gates.
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

  /// Installs the host form-submission callback, or removes it when nil; unregister the
  /// returned registration to withdraw it.
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
  /// Returns this document's action dispatcher; enable actions in the viewer configuration
  /// before requesting it.
  public func actions(for document: DocumentInfo) async throws(PDFError) -> ActionsCapability {
    let handle = try await bridged { try await host.actions(documentId: document.id) }
    return ActionsCapability(handle: handle, viewer: self)
  }
}
