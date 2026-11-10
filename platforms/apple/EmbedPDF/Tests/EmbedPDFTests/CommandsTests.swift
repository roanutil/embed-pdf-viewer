import CustomDump
import EmbedPDF
import Foundation
import Testing

extension BaseSuite {
  @Suite struct CommandsTests {
    func started() async throws -> (PDFViewer, CommandsCapability) {
      let viewer = try PDFViewer(ViewerConfig(commands: true))
      try await viewer.start()
      return (viewer, try await viewer.commands())
    }

    @Test func aSettingLockDisablesAndReportsItsRefusalUntilReleased() async throws {
      let (_, commands) = try await started()
      let held = LockIsolated(true)
      let ran = LockIsolated(0)
      let handler = CommandHandler(
        run: { _ in ran.withValue { $0 += 1 } },
        active: { _ in true },
        locked: { _ in held.value ? CommandSettingLock(by: "kiosk", reasonKey: nil) : nil }
      )
      _ = try await commands.register(CommandDefinition(
        id: "view:single", labelKey: "Single page", shortcuts: ["Ctrl+L"], handler: handler))
      let locked = try #require(try await commands.resolve("view:single"))
      #expect(locked.visible && locked.active && !locked.enabled)
      let match = try await commands.matchStroke(CommandKeyStroke(key: "l", ctrl: true), isMac: false)
      expectNoDifference(match?.refusal?.reason, .locked)
      let blocked = try await commands.execute("view:single")
      expectNoDifference(blocked, .rejected(reason: .disabled))
      expectNoDifference(ran.value, 0)
      held.withValue { $0 = false }
      let released = try await commands.execute("view:single")
      expectNoDifference(released, .executed)
      expectNoDifference(ran.value, 1)
    }

    @Test func everyHookReceivesTheDefaultAppStateJSON() async throws {
      let (_, commands) = try await started()
      let seen = LockIsolated<[String]>([])
      let record: @Sendable (CommandContext) -> Void = { context in
        seen.withValue { $0.append(context.appStateJson) }
      }
      let handler = CommandHandler(
        run: { record($0) },
        enabled: { record($0); return true },
        active: { record($0); return false },
        visible: { record($0); return true },
        iconAccent: { record($0); return nil },
        locked: { record($0); return nil }
      )
      _ = try await commands.register(CommandDefinition(id: "app:inspect", labelKey: "Inspect", handler: handler))
      _ = try await commands.resolve("app:inspect")
      _ = try await commands.execute("app:inspect")
      #expect(!seen.value.isEmpty)
      for value in seen.value {
        expectNoDifference(value, #"{"$undefined":true}"#)
      }
    }

    @Test func everyHookReceivesTheWholeReplacementAppState() async throws {
      let (viewer, commands) = try await started()
      let seen = LockIsolated<[String]>([])
      let runs = LockIsolated<[CommandContext]>([])
      let record: @Sendable (CommandContext) -> Void = { context in
        seen.withValue { $0.append(context.appStateJson) }
      }
      let approved: @Sendable (CommandContext) -> Bool = { context in
        context.appStateJson == #"{"status":"approved"}"#
      }
      let handler = CommandHandler(
        run: { context in record(context); runs.withValue { $0.append(context) } },
        enabled: { record($0); return true },
        active: { record($0); return approved($0) },
        visible: { record($0); return true },
        iconAccent: { context in
          record(context)
          return IconAccent(primary: approved(context) ? "#0f0" : "#f00", secondary: nil)
        }
      )
      let registration = try await commands.register(CommandDefinition(id: "app:inspect", labelKey: "Inspect", handler: handler))
      defer { registration.unregister() }
      try await commands.disableCategory("hidden")
      for json in [#"{"omitted":true,"status":"review"}"#, #"{"status":"approved"}"#, "null"] {
        try await commands.setAppState(Data(json.utf8))
        seen.withValue { $0 = [] }
        let resolved = try await commands.resolve("app:inspect")
        let result = try await commands.execute("app:inspect", args: Data(#"{"copy":true}"#.utf8))
        expectNoDifference(result, .executed)
        expectNoDifference(resolved?.active, json == #"{"status":"approved"}"#)
        expectNoDifference(resolved?.iconAccent?.primary, json == #"{"status":"approved"}"# ? "#0f0" : "#f00")
        #expect(seen.value.count >= 9)
        for value in seen.value { expectNoDifference(value, json) }
        expectNoDifference(runs.value.last?.argsJson, #"{"copy":true}"#)
        let disabled = try await commands.disabledCategories()
        expectNoDifference(disabled, ["hidden"])
      }
      try await viewer.destroy()
    }

    @Test(arguments: [false, true]) func appStateUpdatedDuringRunIsCurrentAfterCompletion(fails: Bool) async throws {
      let (viewer, commands) = try await started()
      struct Full: Error {}
      let ran = LockIsolated<[String]>([])
      try await commands.setAppState(Data(#"{"status":"review"}"#.utf8))
      let handler = CommandHandler(
        run: { context in
          ran.withValue { $0.append(context.appStateJson) }
          try await commands.setAppState(Data(#"{"status":"approved"}"#.utf8))
          if fails { throw Full() }
        },
        active: { $0.appStateJson == #"{"status":"approved"}"# },
        locked: { context in
          context.appStateJson == #"{"status":"approved"}"# ? CommandSettingLock(by: "app-state", reasonKey: nil) : nil
        }
      )
      let registration = try await commands.register(CommandDefinition(id: "app:update", labelKey: "Update", shortcuts: ["Ctrl+L"], handler: handler))
      defer { registration.unregister() }
      let before = try await commands.resolve("app:update")
      expectNoDifference(before?.active, false)
      expectNoDifference(before?.enabled, true)
      if fails {
        await #expect { _ = try await commands.execute("app:update") } throws: { error in
          (error as? PDFError)?.code == .operationFailed
        }
      } else {
        let result = try await commands.execute("app:update")
        expectNoDifference(result, .executed)
      }
      expectNoDifference(ran.value, [#"{"status":"review"}"#])
      let after = try await commands.resolve("app:update")
      expectNoDifference(after?.active, true)
      expectNoDifference(after?.enabled, false)
      let match = try await commands.matchStroke(CommandKeyStroke(key: "l", ctrl: true), isMac: false)
      expectNoDifference(match?.refusal?.reason, .locked)
      try await viewer.destroy()
    }

    @Test func invalidAppStatePreservesThePreviousValue() async throws {
      let (viewer, commands) = try await started()
      let seen = LockIsolated<[String]>([])
      let handler = CommandHandler(enabled: { context in
        seen.withValue { $0.append(context.appStateJson) }
        return true
      })
      let registration = try await commands.register(CommandDefinition(id: "app:inspect", labelKey: "Inspect", handler: handler))
      defer { registration.unregister() }
      try await commands.setAppState(Data(#"{"kept":true}"#.utf8))
      for invalid in [Data("{broken".utf8), Data([0x80])] {
        await #expect { try await commands.setAppState(invalid) } throws: { error in
          (error as? PDFError)?.code == .invalidInput
        }
        seen.withValue { $0 = [] }
        _ = try await commands.resolve("app:inspect")
        expectNoDifference(seen.value, [#"{"kept":true}"#])
      }
      try await viewer.destroy()
    }

    @Test func aSwiftCommandRunsAsync() async throws {
      let (_, commands) = try await started()
      let ran = LockIsolated<[String]>([])
      let handler = CommandHandler(run: { context in
        try await Task.sleep(for: .milliseconds(10))
        ran.withValue { $0.append(context.commandId) }
      })
      _ = try await commands.register(CommandDefinition(id: "doc:save", labelKey: "Save", handler: handler))
      let events = commands.events
      let result = try await commands.execute("doc:save", args: Data(#"{"copy":true}"#.utf8))
      expectNoDifference(result, .executed)
      expectNoDifference(ran.value, ["doc:save"])
      for await event in events {
        guard case let .executed(event) = event else { continue }
        expectNoDifference(event.argsJson, #"{"copy":true}"#)
        break
      }
    }

    @Test func argsThatAreNotUTF8AreInvalidInput() async throws {
      let (_, commands) = try await started()
      let ran = LockIsolated(false)
      let handler = CommandHandler(run: { _ in ran.withValue { $0 = true } })
      _ = try await commands.register(CommandDefinition(id: "doc:save", labelKey: "Save", handler: handler))
      // A lone continuation byte inside a JSON string: `String(decoding:)`
      // would have made it U+FFFD and run the command.
      let args = Data(#"{"name":""#.utf8) + Data([0x80]) + Data(#""}"#.utf8)
      await #expect {
        _ = try await commands.execute("doc:save", args: args)
      } throws: { error in
        (error as? PDFError)?.code == .invalidInput
      }
      expectNoDifference(ran.value, false)
    }

    @Test func aThrowingCommandFails() async throws {
      let (_, commands) = try await started()
      struct Full: Error {}
      let handler = CommandHandler(run: { _ in throw Full() })
      _ = try await commands.register(CommandDefinition(id: "doc:save", labelKey: "Save", handler: handler))
      await #expect {
        _ = try await commands.execute("doc:save")
      } throws: { error in
        (error as? PDFError)?.code == .operationFailed
      }
    }

    @Test func aThrownPDFErrorKeepsItsCode() async throws {
      let (_, commands) = try await started()
      let handler = CommandHandler(run: { _ in
        throw PDFError(name: "PluginError", code: .permissionDenied, detail: "no doc.annotate")
      })
      _ = try await commands.register(CommandDefinition(id: "doc:save", labelKey: "Save", handler: handler))
      let events = commands.events
      await #expect {
        _ = try await commands.execute("doc:save")
      } throws: { error in
        (error as? PDFError)?.code == .permissionDenied && (error as? PDFError)?.detail == "no doc.annotate"
      }
      for await event in events {
        guard case let .executionFailed(event) = event else { continue }
        expectNoDifference(event.error.code, "permission-denied")
        expectNoDifference(event.error.message, "no doc.annotate")
        break
      }
    }

    @Test func aCancelledRunIsOperationCancelled() async throws {
      let (_, commands) = try await started()
      let handler = CommandHandler(run: { _ in throw CancellationError() })
      _ = try await commands.register(CommandDefinition(id: "doc:save", labelKey: "Save", handler: handler))
      await #expect {
        _ = try await commands.execute("doc:save")
      } throws: { error in
        (error as? PDFError)?.code == .operationCancelled
      }
    }

    @Test func anUncodedErrorIsOperationFailedWithItsDescription() async throws {
      let (_, commands) = try await started()
      struct DiskFull: LocalizedError {
        let bytes = 512
        var errorDescription: String? { "the disk is full" }
      }
      let handler = CommandHandler(run: { _ in throw DiskFull() })
      _ = try await commands.register(CommandDefinition(id: "doc:save", labelKey: "Save", handler: handler))
      await #expect {
        _ = try await commands.execute("doc:save")
      } throws: { error in
        (error as? PDFError)?.code == .operationFailed && (error as? PDFError)?.detail == "the disk is full"
      }
    }

    @Test func anUncodedErrorWithoutADescriptionIsNotDumped() async throws {
      let (_, commands) = try await started()
      struct Full: Error { let bytes = 512 }
      let handler = CommandHandler(run: { _ in throw Full() })
      _ = try await commands.register(CommandDefinition(id: "doc:save", labelKey: "Save", handler: handler))
      await #expect {
        _ = try await commands.execute("doc:save")
      } throws: { error in
        guard let error = error as? PDFError else { return false }
        return error.code == .operationFailed && !error.detail.contains("bytes: 512")
      }
    }

    @Test func aDerivationGatesExecution() async throws {
      let (_, commands) = try await started()
      let handler = CommandHandler(run: { _ in }, enabled: { _ in false })
      _ = try await commands.register(CommandDefinition(id: "doc:save", labelKey: "Save", handler: handler))
      let result = try await commands.execute("doc:save")
      expectNoDifference(result, .rejected(reason: .disabled))
      let canExecute = try await commands.canExecute("doc:save")
      expectNoDifference(canExecute, false)
    }

    @Test func aShortcutMatches() async throws {
      let (_, commands) = try await started()
      _ = try await commands.register(
        CommandDefinition(id: "zoom:in", labelKey: "Zoom in", shortcuts: ["Mod+="])
      )
      let matched = try await commands.matchStroke(CommandKeyStroke(key: "=", meta: true), isMac: true)
      expectNoDifference(matched, CommandShortcutMatch(commandId: "zoom:in", refusal: nil))
    }

    @Test func unregisterRemovesTheCommand() async throws {
      let (_, commands) = try await started()
      let registration = try await commands.register(CommandDefinition(id: "zoom:in", labelKey: "Zoom in"))
      registration.unregister()
      let has = try await commands.hasCommand("zoom:in")
      expectNoDifference(has, false)
    }

    @Test func unregisteringAHandlerThatHoldsTheCapabilityReleasesTheViewer() async throws {
      weak var released: PDFViewer?
      do {
        let (viewer, commands) = try await started()
        released = viewer
        let handler = CommandHandler(run: { _ in _ = try await commands.commandIds() })
        let registration = try await commands.register(
          CommandDefinition(id: "doc:list", labelKey: "List", handler: handler)
        )
        registration.unregister()
        // A reply after the unregister means it has run.
        _ = try await commands.commandIds()
      }
      #expect(released == nil)
    }

    @Test func droppingTheStreamUnsubscribes() async throws {
      let (_, commands) = try await started()
      _ = try await commands.register(CommandDefinition(id: "zoom:in", labelKey: "Zoom in"))
      let task = Task { for await _ in commands.events {} }
      try await Task.sleep(for: .milliseconds(20))
      task.cancel()
      _ = await task.value
      _ = try await commands.execute("zoom:in")
    }

    @Test func aDestroyedViewersCapabilityThrows() async throws {
      let (viewer, commands) = try await started()
      try await viewer.destroy()
      await #expect {
        _ = try await commands.commandIds()
      } throws: { error in
        (error as? PDFError)?.code == .instanceClosed
      }
    }
  }
}

/// A value shared between a test and a `@Sendable` closure.
final class LockIsolated<Value>: @unchecked Sendable {
  private let lock = NSLock()
  private var _value: Value
  init(_ value: Value) { _value = value }
  var value: Value { lock.withLock { _value } }
  func withValue(_ body: (inout Value) -> Void) { lock.withLock { body(&_value) } }
}
