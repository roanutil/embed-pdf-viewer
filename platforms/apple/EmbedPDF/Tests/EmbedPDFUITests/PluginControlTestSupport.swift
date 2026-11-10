import CustomDump
@testable import EmbedPDFUI
import Foundation
import Observation
import Testing

func pluginFixture(_ name: String = "report") throws -> Data {
  let url = try #require(Bundle.module.url(forResource: name, withExtension: "pdf", subdirectory: "Fixtures"))
  return try Data(contentsOf: url)
}

@MainActor
func expectObserved<Value: Equatable>(
  _ read: @escaping @MainActor () -> Value,
  equals expected: Value,
  during action: @MainActor () async throws -> Void
) async throws {
  let (stream, continuation) = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingNewest(1))
  @MainActor func arm() {
    withObservationTracking { _ = read() } onChange: { continuation.yield(()) }
  }
  arm()
  defer { continuation.finish() }
  try await action()
  var iterator = stream.makeAsyncIterator()
  while read() != expected {
    guard await iterator.next() != nil else { throw CancellationError() }
    arm()
  }
  expectNoDifference(read(), expected)
}

/// Holds the first read that finishes on `snapshots` until `release` runs,
/// so a refresh started after it can land first. `reached` yields once the
/// read is held.
@MainActor
func holdingFirstRead(of snapshots: SnapshotRefresh) -> (reached: AsyncStream<Void>, release: @MainActor () -> Void) {
  let (reached, reach) = AsyncStream<Void>.makeStream()
  let (gate, open) = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingNewest(1))
  var held = false
  snapshots.readFinished = {
    guard !held else { return }
    held = true
    reach.yield(())
    for await _ in gate { break }
  }
  return (reached, {
    open.yield(())
    open.finish()
  })
}

/// Whether one observation of `read`, armed before `action` runs, fires
/// during it. Unlike ``expectObserved(_:equals:during:)`` this does not
/// wait: the change must land before `action` returns.
@MainActor
func observationFires<Value>(
  _ read: @escaping @MainActor () -> Value,
  during action: @MainActor () async throws -> Void
) async rethrows -> Bool {
  let fired = armObservation(read)
  try await action()
  return fired.value
}

/// Arms one observation of `read`; the flag turns true when it fires.
@MainActor
func armObservation<Value>(_ read: @escaping @MainActor () -> Value) -> ObservationFlag {
  let fired = ObservationFlag()
  withObservationTracking { _ = read() } onChange: { fired.set() }
  return fired
}

final class ObservationFlag: @unchecked Sendable {
  private let lock = NSLock()
  private var fired = false
  func set() { lock.withLock { fired = true } }
  var value: Bool { lock.withLock { fired } }
}
