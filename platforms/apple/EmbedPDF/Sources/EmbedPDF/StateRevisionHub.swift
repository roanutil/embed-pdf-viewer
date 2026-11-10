import Foundation

/// Fans one producer out to any number of consumers. Each `stream()` call is
/// its own subscription with its own buffer, installed before the call
/// returns, so one consumer's cancellation or slowness never affects another.
/// A subscription only sees values yielded after it was installed.
final class BroadcastHub<Element: Sendable>: @unchecked Sendable {
  private let lock = NSLock()
  private let bufferingPolicy: AsyncStream<Element>.Continuation.BufferingPolicy
  private var continuations: [UUID: AsyncStream<Element>.Continuation] = [:]
  private var isFinished = false

  init(bufferingPolicy: AsyncStream<Element>.Continuation.BufferingPolicy = .bufferingNewest(1)) {
    self.bufferingPolicy = bufferingPolicy
  }

  func stream() -> AsyncStream<Element> {
    let (stream, continuation) = AsyncStream<Element>.makeStream(bufferingPolicy: bufferingPolicy)
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

  func yield(_ element: Element) {
    let subscribers = lock.withLock { Array(continuations.values) }
    for subscriber in subscribers { subscriber.yield(element) }
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

/// State revisions coalesce: a slow consumer keeps only the newest.
typealias StateRevisionHub = BroadcastHub<UInt64>

/// Reported errors keep the newest 16 per consumer, the depth the single
/// consumer `PDFViewer.reportedErrors` has always had.
typealias ReportedErrorHub = BroadcastHub<PDFError>

extension ReportedErrorHub {
  convenience init() { self.init(bufferingPolicy: .bufferingNewest(16)) }
}
