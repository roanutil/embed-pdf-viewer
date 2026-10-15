public import EmbedPDFBindings
import Foundation

public typealias CommandDefinition = EmbedPDFBindings.CommandDefinition
public typealias CommandHook = EmbedPDFBindings.CommandHook
public typealias CommandPanel = EmbedPDFBindings.CommandPanel
public typealias CommandContext = EmbedPDFBindings.CommandContext
public typealias CommandRunner = EmbedPDFBindings.CommandRunner
public typealias CommandCompletion = EmbedPDFBindings.CommandCompletion
public typealias CommandSummary = EmbedPDFBindings.CommandSummary
public typealias ResolvedCommand = EmbedPDFBindings.ResolvedCommand
public typealias IconAccent = EmbedPDFBindings.IconAccent
public typealias CommandExecuteResult = EmbedPDFBindings.CommandExecuteResult
public typealias CommandRejectReason = EmbedPDFBindings.CommandRejectReason
public typealias CommandShortcutBinding = EmbedPDFBindings.CommandShortcutBinding
public typealias CommandMenuTarget = EmbedPDFBindings.CommandMenuTarget
public typealias CommandKeyStroke = EmbedPDFBindings.CommandKeyStroke
public typealias CommandsEvent = EmbedPDFBindings.CommandsEvent
public typealias CommandExecuted = EmbedPDFBindings.CommandExecuted
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

  public func derive(context: CommandContext, hook: CommandHook) throws -> Bool {
    switch hook {
    case .enabled: enabled?(context) ?? true
    case .active: active?(context) ?? false
    case .visible: visible?(context) ?? true
    case .run, .iconAccent: true
    }
  }

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

  public func register(_ definitions: [CommandDefinition], replace: Bool = false)
    async throws(PDFError) -> Registration
  {
    Registration(handle: try await bridged { try await handle.registerCommands(definitions: definitions, replace: replace) })
  }

  public func hasCommand(_ id: String) async throws(PDFError) -> Bool {
    try await bridged { try await handle.hasCommand(id: id) }
  }

  public func command(_ id: String) async throws(PDFError) -> CommandSummary? {
    try await bridged { try await handle.command(id: id) }
  }

  public func commandIds() async throws(PDFError) -> [String] {
    try await bridged { try await handle.commandIds() }
  }

  public func resolve(_ id: String, document: PDFDocument? = nil) async throws(PDFError) -> ResolvedCommand? {
    try await bridged { try await handle.resolveCommand(id: id, documentId: document?.id) }
  }

  public func commands(document: PDFDocument? = nil) async throws(PDFError) -> [ResolvedCommand] {
    try await bridged { try await handle.commands(documentId: document?.id) }
  }

  public func search(_ query: String, document: PDFDocument? = nil) async throws(PDFError) -> [ResolvedCommand] {
    try await bridged { try await handle.searchCommands(query: query, documentId: document?.id) }
  }

  public func shortcuts() async throws(PDFError) -> [CommandShortcutBinding] {
    try await bridged { try await handle.shortcuts() }
  }

  /// `args` is JSON, handed back to the command's `run` and to the
  /// `executed` event. Throws `invalid-input` when it isn't valid UTF-8
  /// JSON. Cancelling the task doesn't cancel the command.
  public func execute(_ id: String, document: PDFDocument? = nil, args: Data? = nil)
    async throws(PDFError) -> CommandExecuteResult
  {
    let json = args.map { String(decoding: $0, as: UTF8.self) }
    return try await bridged { try await handle.execute(id: id, documentId: document?.id, argsJson: json) }
  }

  public func canExecute(_ id: String, document: PDFDocument? = nil) async throws(PDFError) -> Bool {
    try await bridged { try await handle.canExecute(id: id, documentId: document?.id) }
  }

  public func disabledCategories() async throws(PDFError) -> [String] {
    try await bridged { try await handle.disabledCategories() }
  }

  public func isCategoryDisabled(_ category: String) async throws(PDFError) -> Bool {
    try await bridged { try await handle.isCategoryDisabled(category: category) }
  }

  public func disableCategory(_ category: String) async throws(PDFError) {
    try await bridged { try await handle.disableCategory(category: category) }
  }

  public func enableCategory(_ category: String) async throws(PDFError) {
    try await bridged { try await handle.enableCategory(category: category) }
  }

  public func setDisabledCategories(_ categories: [String]) async throws(PDFError) {
    try await bridged { try await handle.setDisabledCategories(categories: categories) }
  }

  public func matchStroke(_ stroke: CommandKeyStroke, isMac: Bool) async throws(PDFError) -> String? {
    try await bridged { try await handle.matchStroke(stroke: stroke, isMac: isMac) }
  }

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
  public func commands() async throws(PDFError) -> CommandsCapability {
    let handle = try await bridged { try await host.commands() }
    return CommandsCapability(handle: handle, viewer: self)
  }
}
