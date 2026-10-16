import CustomDump
import EmbedPDF
import Foundation
import Testing

/// Records what reaches it. Captures on down unless `throwsOnDown`.
final class RecordingHandler: InteractionHandler, @unchecked Sendable {
  let lock = NSLock()
  var log: [String] = []
  let throwsOnDown: Bool
  let onDownHook: (@Sendable () -> Void)?

  init(throwsOnDown: Bool = false, onDownHook: (@Sendable () -> Void)? = nil) {
    self.throwsOnDown = throwsOnDown
    self.onDownHook = onDownHook
  }

  struct Refused: Error {}

  func enabledFor(tool: InteractionTool) -> Bool { true }

  func onDown(sample: PointerSample) throws -> Bool {
    lock.withLock { log.append("down") }
    onDownHook?()
    if throwsOnDown { throw Refused() }
    return true
  }

  func onMove(sample: PointerSample) { lock.withLock { log.append("move") } }
  func onUp(sample: PointerSample) { lock.withLock { log.append("up") } }
  func onHover(sample: PointerSample) { lock.withLock { log.append("hover") } }

  var entries: [String] { lock.withLock { log } }
}

func sample(_ phase: PointerPhase, page: PageRef, x: Double = 10) -> PointerSample {
  PointerSample(
    phase: phase,
    viewport: Point(x: x, y: 10),
    page: PointerPageHit(page: page, point: Point(x: x, y: 10)),
    modifiers: Modifiers(),
    pointerType: .touch
  )
}

extension BaseSuite {
  @Suite struct InteractionTests {
    func opened() async throws -> (PDFViewer, DocumentInfo, InteractionCapability, PageRef) {
      let viewer = try PDFViewer(ViewerConfig(interaction: true))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      let page = try await viewer.pages(of: document)[0].page
      return (viewer, document, try await viewer.interaction(for: document), page)
    }

    @Test func activatingAToolIsObserved() async throws {
      let (_, _, interaction, _) = try await opened()
      let ids = try await interaction.tools().map(\.id)
      expectNoDifference(ids, ["pointer", "pan"])
      let events = interaction.events
      try await interaction.activateTool("pan")
      for await event in events {
        guard case let .toolChanged(event) = event else { continue }
        expectNoDifference(event.toolId, "pan")
        expectNoDifference(event.previousToolId, "pointer")
        break
      }
      let active = try await interaction.activeToolId()
      expectNoDifference(active, "pan")
    }

    @Test func anUnknownToolThrowsNotFound() async throws {
      let (_, _, interaction, _) = try await opened()
      await #expect {
        try await interaction.activateTool("laser")
      } throws: { error in
        (error as? PDFError)?.code == "not-found"
      }
    }

    @Test func aSwiftHandlerCapturesAGesture() async throws {
      let (_, _, interaction, page) = try await opened()
      let handler = RecordingHandler()
      _ = try await interaction.registerHandler(handler, id: "recording", priority: 5)
      interaction.dispatchPointer(sample(.down, page: page))
      interaction.dispatchPointer(sample(.move, page: page, x: 20))
      interaction.dispatchPointer(sample(.up, page: page, x: 30))
      // A reply after the fires means they have run.
      _ = try await interaction.cursor()
      expectNoDifference(handler.entries, ["down", "move", "up"])
    }

    @Test func aThrowingHandlerReportsAndFails() async throws {
      let (viewer, _, interaction, page) = try await opened()
      let handler = RecordingHandler(throwsOnDown: true)
      _ = try await interaction.registerHandler(handler, id: "throws", priority: 5)
      interaction.dispatchPointer(sample(.down, page: page))
      interaction.dispatchPointer(sample(.move, page: page, x: 20))
      var reported: PDFError?
      for await error in viewer.reportedErrors {
        reported = error
        break
      }
      expectNoDifference(reported?.name, "HostError")
      _ = try await interaction.cursor()
      // Not captured: the move hovered.
      expectNoDifference(handler.entries, ["down", "hover"])
    }

    @Test func aHandlerThatCallsBackIntoTheViewerDoesNotDeadlock() async throws {
      let (_, _, interaction, page) = try await opened()
      let (stream, continuation) = AsyncStream<String>.makeStream()
      let handler = RecordingHandler {
        Task {
          let cursor = try? await interaction.cursor()
          continuation.yield(cursor ?? "failed")
          continuation.finish()
        }
      }
      _ = try await interaction.registerHandler(handler, id: "reenters", priority: 5)
      interaction.dispatchPointer(sample(.down, page: page))
      var answer: String?
      for await cursor in stream {
        answer = cursor
      }
      expectNoDifference(answer, "default")
    }

    @Test func droppingARegistrationKeepsTheTool() async throws {
      let (_, _, interaction, _) = try await opened()
      do {
        _ = try await interaction.registerTool(
          InteractionTool(id: "ink", cursor: "crosshair", enables: ["annotation-create"]))
      }
      let kept = try await interaction.hasTool("ink")
      expectNoDifference(kept, true)
      let registration = try await interaction.registerTool(
        InteractionTool(id: "ink", cursor: "crosshair", enables: []), replace: true)
      registration.unregister()
      _ = try await interaction.cursor()
      let removed = try await interaction.hasTool("ink")
      expectNoDifference(removed, false)
    }

    @Test func unregisteringAHandlerThatHoldsTheCapabilityReleasesTheViewer() async throws {
      weak var released: PDFViewer?
      do {
        let (viewer, _, interaction, _) = try await opened()
        released = viewer
        let handler = RecordingHandler { _ = interaction }
        let registration = try await interaction.registerHandler(handler, id: "holds", priority: 1)
        registration.unregister()
        // A reply after the unregister means it has run.
        _ = try await interaction.cursor()
      }
      #expect(released == nil)
    }

    @Test func aClosedDocumentsCapabilityThrows() async throws {
      let (viewer, document, interaction, _) = try await opened()
      try await viewer.close(document)
      await #expect {
        _ = try await interaction.activeToolId()
      } throws: { error in
        (error as? PDFError)?.code == "instance-closed"
      }
    }

    @Test func droppingTheStreamUnsubscribes() async throws {
      let (_, _, interaction, _) = try await opened()
      let task = Task { for await _ in interaction.events {} }
      try await Task.sleep(for: .milliseconds(20))
      task.cancel()
      _ = await task.value
      try await interaction.activateTool("pan")
    }
  }
}
