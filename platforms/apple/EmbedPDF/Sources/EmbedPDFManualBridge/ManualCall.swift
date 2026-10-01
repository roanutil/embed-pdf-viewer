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

/// `manualCall` for a call that answers a call id: the twin of
/// `bridgedCancellable`. Cancelling the task cancels the call.
func manualCallCancellable<T>(
  _ start: (UnsafeMutableRawPointer) -> UInt64,
  read: @escaping (UnsafeRawPointer?) -> T
) async throws(ManualError) -> T {
  let callID = OSAllocatedUnfairLock<UInt64?>(initialState: nil)
  do {
    return try await withTaskCancellationHandler {
      try await awaitReply({ ctx in
        let id = start(ctx)
        callID.withLock { $0 = id }
      }, read: read)
    } onCancel: {
      if let id = callID.withLock({ $0 }) {
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
