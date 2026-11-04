@testable import EmbedPDFUI
import Testing

@MainActor
@Suite(.serialized, .timeLimit(.minutes(1)))
struct PageRenderTaskTests {
  @Test func replacementWaitsForCancelledWorkToFinish() async {
    let work = PageRenderTask()
    let entered = RenderTaskGate()
    let finish = RenderTaskGate()
    var events: [String] = []
    let first = Task {
      await work.run {
        entered.open()
        await finish.wait()
        events.append("first finished")
      }
    }
    await entered.wait()
    let second = Task { await work.run { events.append("replacement") } }
    for _ in 0..<20 { await Task.yield() }
    #expect(events.isEmpty)
    finish.open()
    await first.value
    await second.value
    #expect(events == ["first finished", "replacement"])
  }

  @Test func theNewestQueuedRequestSupersedesEarlierWaitingWork() async {
    let work = PageRenderTask()
    let entered = RenderTaskGate()
    let finish = RenderTaskGate()
    var events: [String] = []
    let first = Task { await work.run { entered.open(); await finish.wait(); events.append("first") } }
    await entered.wait()
    let second = Task { await work.run { events.append("superseded") } }
    for _ in 0..<20 { await Task.yield() }
    let third = Task { await work.run { events.append("newest") } }
    for _ in 0..<20 { await Task.yield() }
    finish.open()
    await first.value
    await second.value
    await third.value
    #expect(events == ["first", "newest"])
  }

  @Test func cancellingTheCallerCancelsItsQueuedOperation() async {
    let work = PageRenderTask()
    let entered = RenderTaskGate()
    let finish = RenderTaskGate()
    var events: [String] = []
    let first = Task { await work.run { entered.open(); await finish.wait(); events.append("first") } }
    await entered.wait()
    let second = Task { await work.run { events.append("cancelled") } }
    for _ in 0..<20 { await Task.yield() }
    second.cancel()
    finish.open()
    await first.value
    await second.value
    #expect(events == ["first"])
  }

  @Test func backgroundWorkDoesNotCompeteWithAnActiveRequest() async {
    let work = PageRenderTask()
    let entered = RenderTaskGate()
    let finish = RenderTaskGate()
    let backgroundFinished = RenderTaskGate()
    var events: [String] = []
    let first = Task { await work.run { entered.open(); await finish.wait(); events.append("first") } }
    await entered.wait()
    work.startIfIdle { events.append("duplicate") }
    finish.open()
    await first.value
    work.startIfIdle { events.append("background"); backgroundFinished.open() }
    await backgroundFinished.wait()
    #expect(events == ["first", "background"])
  }
}

@MainActor
private final class RenderTaskGate {
  private var isOpen = false
  private var continuation: CheckedContinuation<Void, Never>?

  func wait() async {
    if isOpen { return }
    await withCheckedContinuation { continuation = $0 }
  }

  func open() {
    isOpen = true
    continuation?.resume()
    continuation = nil
  }
}
