import CustomDump
@testable import EmbedPDFUI
import Testing

@Suite(.timeLimit(.minutes(1)))
@MainActor
struct SnapshotRefreshTests {
  @Test func aSlowerReadCannotOverwriteANewerSnapshot() async {
    let refresh = SnapshotRefresh()
    var applied: [String] = []
    let (started, start) = AsyncStream<Void>.makeStream()
    let (gate, release) = AsyncStream<Void>.makeStream()
    let slow = Task {
      await refresh.run(load: {
        start.yield(())
        for await _ in gate { break }
        return "old"
      }, apply: { applied.append($0) }, failure: { _ in Issue.record("unexpected read failure") })
    }
    var ready = started.makeAsyncIterator()
    _ = await ready.next()
    await refresh.run(load: { "new" }, apply: { applied.append($0) }, failure: { _ in Issue.record("unexpected read failure") })
    release.yield(())
    release.finish()
    await slow.value
    expectNoDifference(applied, ["new"])
  }

  @Test func invalidationDiscardsAPendingFailure() async {
    struct Failure: Error {}
    let refresh = SnapshotRefresh()
    var failures = 0
    let (started, start) = AsyncStream<Void>.makeStream()
    let (gate, release) = AsyncStream<Void>.makeStream()
    let slow = Task {
      await refresh.run(load: { () async throws -> String in
        start.yield(())
        for await _ in gate { break }
        throw Failure()
      }, apply: { _ in Issue.record("unexpected snapshot") }, failure: { _ in failures += 1 })
    }
    var ready = started.makeAsyncIterator()
    _ = await ready.next()
    refresh.invalidate()
    release.yield(())
    release.finish()
    await slow.value
    expectNoDifference(failures, 0)
  }
}
