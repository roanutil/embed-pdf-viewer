// The plumbing every manual call shares: one C trampoline, a retained box
// for the continuation, and cancellation through the call id.
public import EmbedPDF
internal import EpdfManualFFI
import os

/// What an `EpdfError` said.
public struct ManualError: Error, Sendable, Equatable {
  public let kind: Int32
  public let code: String?
  public let detail: String?

  init(_ error: EpdfError) {
    kind = error.kind
    code = error.code.map { String(cString: $0) }
    detail = error.detail.map { String(cString: $0) }
  }
}

/// The callback's body, retained across the C call and released inside it.
final class Reply: @unchecked Sendable {
  let body: (EpdfError, UnsafeRawPointer?) -> Void

  init(_ body: @escaping (EpdfError, UnsafeRawPointer?) -> Void) {
    self.body = body
  }
}

/// The one `EpdfComplete` every call passes. It runs on the kernel thread.
let trampoline: EpdfComplete = { ctx, error, result in
  let reply = Unmanaged<Reply>.fromOpaque(ctx!).takeRetainedValue()
  reply.body(error, result)
  epdf_error_free(error)
}

/// A value read inside the callback, carried to the awaiting task. The
/// reader decides what is safe to carry.
struct Carried<T>: @unchecked Sendable {
  let value: T
}

/// The continuation's half: start the call with a retained `Reply`.
private func awaitReply<T>(
  _ start: (UnsafeMutableRawPointer) -> Void,
  read: @escaping (UnsafeRawPointer?) -> T
) async throws(ManualError) -> T {
  do {
    let carried = try await withCheckedThrowingContinuation {
      (continuation: CheckedContinuation<Carried<T>, any Error>) in
      let reply = Reply { error, result in
        if error.kind == EPDF_ERROR_OK {
          continuation.resume(returning: Carried(value: read(result)))
        } else {
          continuation.resume(throwing: ManualError(error))
        }
      }
      start(Unmanaged.passRetained(reply).toOpaque())
    }
    return carried.value
  } catch let error as ManualError {
    throw error
  } catch {
    throw ManualError(kind: -1, code: nil, detail: "\(error)")
  }
}

/// Start one C call and await its callback: the manual twin of `bridged`.
/// `read` runs inside the callback, on the kernel thread, while `result` is
/// valid.
func manualCall<T>(
  _ start: (UnsafeMutableRawPointer) -> Void,
  read: @escaping (UnsafeRawPointer?) -> T
) async throws(ManualError) -> T {
  try await awaitReply(start, read: read)
}

/// Where a cancellable call is when its task is cancelled. The C call
/// answers its id only once it returns, and the task can be cancelled
/// before then (or before it starts, which runs `onCancel` first). Public
/// only so the benchmark target, which builds in Release without
/// `@testable`, can test it.
public enum ManualCallState: Sendable, Equatable {
  case starting
  case started(UInt64)
  case cancelled

  /// The task was cancelled: the id to cancel now, if the call has one.
  /// Otherwise the call cancels itself once it has.
  public mutating func cancel() -> UInt64? {
    if case let .started(id) = self { return id }
    self = .cancelled
    return nil
  }

  /// The call answered `id`: `true` when the task was already cancelled,
  /// so the caller cancels the call at once.
  public mutating func start(_ id: UInt64) -> Bool {
    if self == .cancelled { return true }
    self = .started(id)
    return false
  }
}

/// `manualCall` for a call that answers a call id: the twin of
/// `bridgedCancellable`. Cancelling the task cancels the call, also when
/// the cancel comes before the call has its id: `epdf_cancel` then runs
/// right after the call starts, and the kernel thread runs it after the
/// starting job, as it runs jobs in post order.
func manualCallCancellable<T>(
  _ start: (UnsafeMutableRawPointer) -> UInt64,
  read: @escaping (UnsafeRawPointer?) -> T
) async throws(ManualError) -> T {
  let state = OSAllocatedUnfairLock(initialState: ManualCallState.starting)
  do {
    return try await withTaskCancellationHandler {
      try await awaitReply({ ctx in
        let id = start(ctx)
        if state.withLock({ $0.start(id) }) {
          epdf_cancel(id)
        }
      }, read: read)
    } onCancel: {
      if let id = state.withLock({ $0.cancel() }) {
        epdf_cancel(id)
      }
    }
  } catch let error as ManualError {
    throw error
  } catch {
    throw ManualError(kind: -1, code: nil, detail: "\(error)")
  }
}

extension ManualError {
  init(kind: Int32, code: String?, detail: String?) {
    self.kind = kind
    self.code = code
    self.detail = detail
  }
}

/// The manual bridge's two control entry points.
public enum ManualBench {
  /// Returns at once on the calling thread: the C entry cost alone.
  public static func noop() {
    epdf_bench_noop()
  }

  /// An empty job on the kernel thread, answered through the trampoline:
  /// the C entry plus the hop.
  public static func noopHop(_ viewer: PDFViewer) async throws(ManualError) {
    let kernel = viewer.experimentalKernelToken
    try await manualCall({ ctx in
      epdf_bench_noop_hop(kernel, ctx, trampoline)
    }) { _ in () }
  }
}
