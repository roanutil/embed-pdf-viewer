internal import EmbedPDFBindings
import Foundation

final class EventRelay<Event: Sendable>: @unchecked Sendable {
  let continuation: AsyncStream<Event>.Continuation

  init(_ continuation: AsyncStream<Event>.Continuation) {
    self.continuation = continuation
  }
}

extension EventRelay: DocumentEventListener where Event == DocumentEvent {
  func onEvent(event: DocumentEvent) {
    continuation.yield(event)
  }
}

final class StateRelay: ViewerDelegate, @unchecked Sendable {
  let continuation: AsyncStream<UInt64>.Continuation
  let errors: AsyncStream<PDFError>.Continuation

  init(_ continuation: AsyncStream<UInt64>.Continuation, errors: AsyncStream<PDFError>.Continuation) {
    self.continuation = continuation
    self.errors = errors
  }

  func stateChanged(revision: UInt64) {
    continuation.yield(revision)
  }

  func report(error: ViewerError) {
    errors.yield(PDFError(error))
  }
}

final class StreamRegistry: @unchecked Sendable {
  private let lock = NSLock()
  private var finishers: [UUID: @Sendable () -> Void] = [:]
  private var isFinished = false

  func stream<Event: Sendable>(
    subscribe: (EventRelay<Event>) -> Subscription
  ) -> AsyncStream<Event> {
    let (stream, continuation) = AsyncStream<Event>.makeStream()
    let subscription = subscribe(EventRelay(continuation))
    let id = UUID()
    continuation.onTermination = { [weak self] _ in
      self?.remove(id)
      subscription.cancel()
    }
    if register(id, { continuation.finish() }) {
      continuation.finish()
    }
    return stream
  }

  func finishAll() {
    let pending = lock.withLock {
      isFinished = true
      let pending = Array(finishers.values)
      finishers = [:]
      return pending
    }
    for finish in pending { finish() }
  }

  private func register(_ id: UUID, _ finish: @escaping @Sendable () -> Void) -> Bool {
    lock.withLock {
      if isFinished { return true }
      finishers[id] = finish
      return false
    }
  }

  private func remove(_ id: UUID) {
    lock.withLock { _ = finishers.removeValue(forKey: id) }
  }
}
