public import EmbedPDFBindings
import Foundation

/// A command's identity, presentation, routing, and optional handler hooks.
public typealias CommandDefinition = EmbedPDFBindings.CommandDefinition
/// A command behavior supplied by a registered runner.
public typealias CommandHook = EmbedPDFBindings.CommandHook
/// The panel routing information attached to a command.
public typealias CommandPanel = EmbedPDFBindings.CommandPanel
/// The document and invocation context passed to command hooks.
public typealias CommandContext = EmbedPDFBindings.CommandContext
/// The host callbacks that execute commands and derive their presentation state.
public typealias CommandRunner = EmbedPDFBindings.CommandRunner
/// The callback a command runner must finish or fail after execution.
public typealias CommandCompletion = EmbedPDFBindings.CommandCompletion
/// The registered presentation and routing information for a command.
public typealias CommandSummary = EmbedPDFBindings.CommandSummary
/// A command with enabled, active, visible, and accent hooks evaluated for a context.
public typealias ResolvedCommand = EmbedPDFBindings.ResolvedCommand
/// An accent applied to a resolved command's icon.
public typealias IconAccent = EmbedPDFBindings.IconAccent
/// The outcome of attempting to execute or route a command.
public typealias CommandExecuteResult = EmbedPDFBindings.CommandExecuteResult
/// The reason a command execution request was rejected.
public typealias CommandRejectReason = EmbedPDFBindings.CommandRejectReason
/// A registered keyboard shortcut associated with a command.
public typealias CommandShortcutBinding = EmbedPDFBindings.CommandShortcutBinding
/// The menu routing target associated with a command.
public typealias CommandMenuTarget = EmbedPDFBindings.CommandMenuTarget
/// A key and modifier state to match against command shortcuts.
public typealias CommandKeyStroke = EmbedPDFBindings.CommandKeyStroke
/// An event describing command registry and execution changes.
public typealias CommandsEvent = EmbedPDFBindings.CommandsEvent
/// An event describing a command execution.
public typealias CommandExecuted = EmbedPDFBindings.CommandExecuted
/// The error reported when a command runner fails.
public typealias CommandExecutionFailure = EmbedPDFBindings.CommandExecutionFailure

/// A `CommandRunner` from closures. `enabled`, `active`, `visible` and
/// `iconAccent` run on the viewer's kernel thread on every resolve: answer
/// from state you already have, and never wait on the main actor there.
///
/// The viewer keeps a registered handler until you call `unregister()` or
/// destroy the viewer; closing a document doesn't release it. A handler
/// whose closures hold a capability or the `PDFViewer` keeps the viewer
/// alive until then, as a closure that captures its owner does.
public final class CommandHandler: CommandRunner, Sendable {
  let runBody: (@Sendable (CommandContext) async throws -> Void)?
  let enabled: (@Sendable (CommandContext) -> Bool)?
  let active: (@Sendable (CommandContext) -> Bool)?
  let visible: (@Sendable (CommandContext) -> Bool)?
  let accent: (@Sendable (CommandContext) -> IconAccent?)?

  /// Creates a handler from optional execution and state-derivation closures. Only supplied
  /// closures appear in ``hooks``.
  public init(
    run: (@Sendable (CommandContext) async throws -> Void)? = nil,
    enabled: (@Sendable (CommandContext) -> Bool)? = nil,
    active: (@Sendable (CommandContext) -> Bool)? = nil,
    visible: (@Sendable (CommandContext) -> Bool)? = nil,
    iconAccent: (@Sendable (CommandContext) -> IconAccent?)? = nil
  ) {
    self.runBody = run
    self.enabled = enabled
    self.active = active
    self.visible = visible
    self.accent = iconAccent
  }

  /// The hooks this handler implements, for `CommandDefinition.hooks`.
  public var hooks: [CommandHook] {
    var hooks: [CommandHook] = []
    if runBody != nil { hooks.append(.run) }
    if enabled != nil { hooks.append(.enabled) }
    if active != nil { hooks.append(.active) }
    if visible != nil { hooks.append(.visible) }
    if accent != nil { hooks.append(.iconAccent) }
    return hooks
  }

  /// Invokes the asynchronous run closure and finishes or fails the supplied completion;
  /// finishes immediately when no run closure was supplied.
  public func run(context: CommandContext, completion: CommandCompletion) {
    guard let runBody else {
      completion.finish()
      return
    }
    Task {
      do {
        try await runBody(context)
        completion.finish()
      } catch {
        completion.fail(error)
      }
    }
  }

  /// Evaluates a Boolean hook synchronously; absent enabled and visible hooks default to
  /// true, and absent active defaults to false.
  public func derive(context: CommandContext, hook: CommandHook) throws -> Bool {
    switch hook {
    case .enabled: enabled?(context) ?? true
    case .active: active?(context) ?? false
    case .visible: visible?(context) ?? true
    case .run, .iconAccent: true
    }
  }

  /// Evaluates the optional accent hook for the supplied context, returning nil when no
  /// accent closure was supplied.
  public func iconAccent(context: CommandContext) throws -> IconAccent? {
    accent?(context)
  }
}

extension CommandCompletion {
  /// Fail the command with `error`, keeping its code (see
  /// `HostFailure.init(_:)`): `execute` throws it and `.executionFailed`
  /// reports it.
  public func fail(_ error: any Error) {
    failWith(failure: HostFailure(error))
  }
}

extension CommandDefinition {
  /// A definition whose hooks come from `handler`. A routing command
  /// (menu, panel or modal, no handler) uses the generated initializer.
  /// `handler` has no default so the two never compete for the same call.
  public init(
    id: String,
    labelKey: String,
    icon: String? = nil,
    shortcuts: [String] = [],
    categories: [String] = [],
    menu: String? = nil,
    panel: CommandPanel? = nil,
    modal: String? = nil,
    handler: CommandHandler
  ) {
    self.init(
      id: id, labelKey: labelKey, icon: icon, shortcuts: shortcuts, categories: categories,
      menu: menu, panel: panel, modal: modal, runner: handler, hooks: handler.hooks
    )
  }
}

/// The workspace command registry. Get it with `PDFViewer.commands()`; the
/// viewer needs `ViewerConfig(commands: true)`.
public struct CommandsCapability: Sendable {
  let handle: CommandsHandle
  let viewer: PDFViewer

  /// Throws `conflict` for a duplicate id unless `replace`. Dropping the
  /// registration keeps the command; `unregister()` removes it. The viewer
  /// holds the definition's handler until then (see `CommandHandler`).
  public func register(_ definition: CommandDefinition, replace: Bool = false)
    async throws(PDFError) -> Registration
  {
    Registration(handle: try await bridged { try await handle.registerCommand(definition: definition, replace: replace) })
  }

  /// Registers command definitions as a group and returns a registration that can explicitly
  /// remove them; replace permits existing identifiers to be replaced.
  public func register(_ definitions: [CommandDefinition], replace: Bool = false)
    async throws(PDFError) -> Registration
  {
    Registration(handle: try await bridged { try await handle.registerCommands(definitions: definitions, replace: replace) })
  }

  /// Returns whether an identifier is registered in the workspace command registry.
  public func hasCommand(_ id: String) async throws(PDFError) -> Bool {
    try await bridged { try await handle.hasCommand(id: id) }
  }

  /// Returns a registered command's summary, or nil for an unknown identifier.
  public func command(_ id: String) async throws(PDFError) -> CommandSummary? {
    try await bridged { try await handle.command(id: id) }
  }

  /// Returns the identifiers currently registered in the command registry.
  public func commandIds() async throws(PDFError) -> [String] {
    try await bridged { try await handle.commandIds() }
  }

  /// Evaluates a command's presentation hooks in the supplied document context, or returns
  /// nil for an unknown identifier.
  public func resolve(_ id: String, document: DocumentInfo? = nil) async throws(PDFError) -> ResolvedCommand? {
    try await bridged { try await handle.resolveCommand(id: id, documentId: document?.id) }
  }

  /// Resolves the registered commands for the supplied document context.
  public func commands(document: DocumentInfo? = nil) async throws(PDFError) -> [ResolvedCommand] {
    try await bridged { try await handle.commands(documentId: document?.id) }
  }

  /// Searches registered commands for the query and resolves matching commands in the
  /// supplied document context.
  public func search(_ query: String, document: DocumentInfo? = nil) async throws(PDFError) -> [ResolvedCommand] {
    try await bridged { try await handle.searchCommands(query: query, documentId: document?.id) }
  }

  /// Returns the keyboard shortcuts associated with registered commands.
  public func shortcuts() async throws(PDFError) -> [CommandShortcutBinding] {
    try await bridged { try await handle.shortcuts() }
  }

  /// `args` is JSON, handed back to the command's `run` and to the
  /// `executed` event. Throws `invalid-input` when it isn't valid UTF-8
  /// JSON. Cancelling the task doesn't cancel the command.
  public func execute(_ id: String, document: DocumentInfo? = nil, args: Data? = nil)
    async throws(PDFError) -> CommandExecuteResult
  {
    // `String(decoding:)` would replace bad bytes with U+FFFD and pass
    // whatever JSON that leaves; `String(bytes:encoding:)` refuses them.
    var json: String?
    if let args {
      guard let text = String(bytes: args, encoding: .utf8) else {
        throw PDFError(name: "EncodingError", code: "invalid-input", detail: "args are not valid UTF-8")
      }
      json = text
    }
    return try await bridged { try await handle.execute(id: id, documentId: document?.id, argsJson: json) }
  }

  /// Checks whether the command is executable in the supplied document context without running it.
  public func canExecute(_ id: String, document: DocumentInfo? = nil) async throws(PDFError) -> Bool {
    try await bridged { try await handle.canExecute(id: id, documentId: document?.id) }
  }

  /// Returns the command categories currently disabled in the workspace.
  public func disabledCategories() async throws(PDFError) -> [String] {
    try await bridged { try await handle.disabledCategories() }
  }

  /// Returns whether the named command category is disabled.
  public func isCategoryDisabled(_ category: String) async throws(PDFError) -> Bool {
    try await bridged { try await handle.isCategoryDisabled(category: category) }
  }

  /// Disables a command category for subsequent command resolution and execution.
  public func disableCategory(_ category: String) async throws(PDFError) {
    try await bridged { try await handle.disableCategory(category: category) }
  }

  /// Enables a previously disabled command category.
  public func enableCategory(_ category: String) async throws(PDFError) {
    try await bridged { try await handle.enableCategory(category: category) }
  }

  /// Replaces the workspace's disabled command categories with the supplied list.
  public func setDisabledCategories(_ categories: [String]) async throws(PDFError) {
    try await bridged { try await handle.setDisabledCategories(categories: categories) }
  }

  /// Returns the matching command identifier, or nil when no shortcut matches; isMac selects
  /// macOS modifier interpretation.
  public func matchStroke(_ stroke: CommandKeyStroke, isMac: Bool) async throws(PDFError) -> String? {
    try await bridged { try await handle.matchStroke(stroke: stroke, isMac: isMac) }
  }

  /// Returns a command's menu routing target, or nil when it has none.
  public func menuTarget(_ id: String) async throws(PDFError) -> CommandMenuTarget? {
    try await bridged { try await handle.menuTarget(id: id) }
  }

  /// Finishes when the viewer is destroyed.
  public var events: AsyncStream<CommandsEvent> {
    viewer.registry.stream { handle.subscribe(listener: $0) }
  }
}

extension EventRelay: CommandsEventListener where Event == CommandsEvent {
  func onEvent(event: CommandsEvent) {
    continuation.yield(event)
  }
}

extension PDFViewer {
  /// Returns the workspace command registry; enable commands in the viewer configuration
  /// before requesting it.
  public func commands() async throws(PDFError) -> CommandsCapability {
    let handle = try await bridged { try await host.commands() }
    return CommandsCapability(handle: handle, viewer: self)
  }
}
