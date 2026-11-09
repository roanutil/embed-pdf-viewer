import Foundation

final class StateRevisionHub: @unchecked Sendable {
  private let lock = NSLock()
  private var continuations: [UUID: AsyncStream<UInt64>.Continuation] = [:]
  private var isFinished = false

  func stream() -> AsyncStream<UInt64> {
    let (stream, continuation) = AsyncStream<UInt64>.makeStream(bufferingPolicy: .bufferingNewest(1))
    let id = UUID()
    continuation.onTermination = { [weak self] _ in self?.remove(id) }
    let finished = lock.withLock {
      guard !isFinished else { return true }
      continuations[id] = continuation
      return false
    }
    if finished { continuation.finish() }
    return stream
  }

  func yield(_ revision: UInt64) {
    let subscribers = lock.withLock { Array(continuations.values) }
    for subscriber in subscribers { subscriber.yield(revision) }
  }

  func finish() {
    let subscribers = lock.withLock {
      isFinished = true
      let subscribers = Array(continuations.values)
      continuations.removeAll()
      return subscribers
    }
    for subscriber in subscribers { subscriber.finish() }
  }

  private func remove(_ id: UUID) {
    lock.withLock { _ = continuations.removeValue(forKey: id) }
  }

  deinit { finish() }
}
