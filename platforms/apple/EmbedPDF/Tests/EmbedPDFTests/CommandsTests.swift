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

    @Test func aThrowingCommandFails() async throws {
      let (_, commands) = try await started()
      struct Full: Error {}
      let handler = CommandHandler(run: { _ in throw Full() })
      _ = try await commands.register(CommandDefinition(id: "doc:save", labelKey: "Save", handler: handler))
      await #expect {
        _ = try await commands.execute("doc:save")
      } throws: { error in
        (error as? PDFError)?.code == "operation-failed"
      }
    }

    @Test func aThrownPDFErrorKeepsItsCode() async throws {
      let (_, commands) = try await started()
      let handler = CommandHandler(run: { _ in
        throw PDFError(name: "PluginError", code: "permission-denied", detail: "no doc.annotate")
      })
      _ = try await commands.register(CommandDefinition(id: "doc:save", labelKey: "Save", handler: handler))
      let events = commands.events
      await #expect {
        _ = try await commands.execute("doc:save")
      } throws: { error in
        (error as? PDFError)?.code == "permission-denied" && (error as? PDFError)?.detail == "no doc.annotate"
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
        (error as? PDFError)?.code == "operation-cancelled"
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
        (error as? PDFError)?.code == "operation-failed" && (error as? PDFError)?.detail == "the disk is full"
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
        return error.code == "operation-failed" && !error.detail.contains("bytes: 512")
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
      expectNoDifference(matched, "zoom:in")
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
        (error as? PDFError)?.code == "instance-closed"
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
