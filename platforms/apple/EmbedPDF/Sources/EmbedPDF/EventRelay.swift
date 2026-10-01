internal import EmbedPDFBindings

final class EventRelay<Event: Sendable>: @unchecked Sendable {
  let continuation: AsyncStream<Event>.Continuation

  init(_ continuation: AsyncStream<Event>.Continuation) {
    self.continuation = continuation
  }

  static func stream(subscribe: (EventRelay<Event>) -> Subscription) -> AsyncStream<Event> {
    let (stream, continuation) = AsyncStream<Event>.makeStream()
    let subscription = subscribe(EventRelay(continuation))
    continuation.onTermination = { _ in subscription.cancel() }
    return stream
  }
}

extension EventRelay: DocumentEventListener where Event == DocumentEvent {
  func onEvent(event: DocumentEvent) {
    continuation.yield(event)
  }
}

final class StateRelay: ViewerDelegate, @unchecked Sendable {
  let continuation: AsyncStream<UInt64>.Continuation

  init(_ continuation: AsyncStream<UInt64>.Continuation) {
    self.continuation = continuation
  }

  func stateChanged(revision: UInt64) {
    continuation.yield(revision)
  }

  func report(error: ViewerError) {}
}
