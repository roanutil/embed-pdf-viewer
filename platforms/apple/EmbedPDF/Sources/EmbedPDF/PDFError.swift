internal import EmbedPDFBindings
import Foundation

/// What every viewer and capability call throws.
///
/// `code` is a ``Code``: one of the kernel's nine plugin error codes, the
/// `PluginErrorCode` vocabulary of the TypeScript kernel's `toPluginError`,
/// the bindings' own `invalid-state`, or `unknown` for a code the kernel
/// adds later. See ``Code`` for what each one means.
///
/// An engine's own code (`NotFound`, `DocPasswordIncorrect`) never appears
/// here: it maps as `toPluginError` maps it, and `detail` is the engine's
/// message with no code in front. `name` says what raised the error (`PluginError`,
/// `EngineError`, `AbortError`, or a foreign error's type), and is for
/// logging rather than matching.
public struct PDFError: Error, Sendable, Equatable {
  /// A kernel error code. `rawValue` is the wire string, and every string
  /// round-trips: one this enum doesn't name becomes `unknown`.
  public enum Code: Sendable, Hashable, RawRepresentable {
    /// `permission-denied`: the scope, the document's permissions or its
    /// password refused it (a wrong `unlock` password is this).
    case permissionDenied
    /// `unsupported`: the plugin isn't in the `ViewerConfig`, or the engine
    /// can't do it.
    case unsupported
    /// `not-found`: no such document, page, annotation or other resource.
    case notFound
    /// `not-ready`: the document is still loading, is locked, or failed.
    case notReady
    /// `invalid-input`: an argument, or bytes that aren't a PDF.
    case invalidInput
    /// `conflict`: the document changed under the call.
    case conflict
    /// `instance-closed`: the document closed or the viewer shut down.
    case instanceClosed
    /// `operation-cancelled`: the task was cancelled.
    case operationCancelled
    /// `operation-failed`: anything else.
    case operationFailed
    /// `invalid-state`, the bindings' own code: a value that didn't survive
    /// the trip across the boundary (a record the kernel answered without a
    /// field, a count past `UInt32`, a raster Core Graphics refused). It is
    /// a bug to report, not something a caller did.
    case invalidState
    /// A code this enum doesn't name, kept as the kernel sent it.
    case unknown(String)

    public init(rawValue: String) {
      switch rawValue {
      case "permission-denied": self = .permissionDenied
      case "unsupported": self = .unsupported
      case "not-found": self = .notFound
      case "not-ready": self = .notReady
      case "invalid-input": self = .invalidInput
      case "conflict": self = .conflict
      case "instance-closed": self = .instanceClosed
      case "operation-cancelled": self = .operationCancelled
      case "operation-failed": self = .operationFailed
      case "invalid-state": self = .invalidState
      default: self = .unknown(rawValue)
      }
    }

    public var rawValue: String {
      switch self {
      case .permissionDenied: "permission-denied"
      case .unsupported: "unsupported"
      case .notFound: "not-found"
      case .notReady: "not-ready"
      case .invalidInput: "invalid-input"
      case .conflict: "conflict"
      case .instanceClosed: "instance-closed"
      case .operationCancelled: "operation-cancelled"
      case .operationFailed: "operation-failed"
      case .invalidState: "invalid-state"
      case let .unknown(code): code
      }
    }

    /// `unknown("not-found")` and `.notFound` are the same code.
    public static func == (lhs: Code, rhs: Code) -> Bool { lhs.rawValue == rhs.rawValue }

    public func hash(into hasher: inout Hasher) { hasher.combine(rawValue) }
  }

  public let name: String
  public let code: Code
  public let detail: String

  init(_ error: ViewerError) {
    switch error {
    case let .Kernel(name, code, detail):
      self.name = name
      self.code = code.map(Code.init(rawValue:)) ?? .operationFailed
      self.detail = detail
    case .Closed:
      name = "Closed"
      code = .instanceClosed
      detail = "The viewer has shut down."
    }
  }

  init(_ error: any Error) {
    if let error = error as? ViewerError {
      self.init(error)
    } else if let error = error as? PDFError {
      self = error
    } else if error is CancellationError {
      self = PDFError(name: "AbortError", code: .operationCancelled, detail: "operation cancelled")
    } else {
      self = PDFError(name: String(describing: type(of: error)), code: .operationFailed, detail: readable(error))
    }
  }

  /// For a handler or port that fails with a kernel code of its own, such
  /// as `permission-denied`. The completion it fails passes the code on.
  public init(name: String, code: Code, detail: String) {
    self.name = name
    self.code = code
    self.detail = detail
  }

  /// The pre-``Code`` spelling, for a handler or port that still passes a
  /// string. A `nil` code becomes ``Code/operationFailed``.
  @available(*, deprecated, message: "Pass a PDFError.Code: init(name:code:detail:) with .notFound, or Code(rawValue:) for a string.")
  public init(name: String, code: String?, detail: String) {
    self.init(name: name, code: code.map(Code.init(rawValue:)) ?? .operationFailed, detail: detail)
  }
}

/// A message for an error nobody gave a code: never a struct dump.
func readable(_ error: any Error) -> String {
  if let described = (error as? LocalizedError)?.errorDescription { return described }
  // On the metatype: a value cast to `CustomStringConvertible` always
  // succeeds, through its bridge to `NSError`.
  let type = type(of: error)
  if !(type is NSError.Type), type is any CustomStringConvertible.Type { return String(describing: error) }
  return (error as NSError).localizedDescription
}

func bridged<T>(_ body: () async throws -> T) async throws(PDFError) -> T {
  do {
    return try await body()
  } catch {
    throw PDFError(error)
  }
}

/// `bridged` for a call that takes a `CancelToken`. UniFFI's Swift does not
/// cancel the Rust future when the task is cancelled, so cancelling the task
/// cancels the token instead, which aborts the call on the kernel thread.
/// Pass the token it hands you; a `CancelToken()` made inline is never
/// cancelled.
func bridgedCancellable<T>(_ body: (CancelToken) async throws -> T) async throws(PDFError) -> T {
  let token = CancelToken()
  return try await bridged {
    try await withTaskCancellationHandler {
      try await body(token)
    } onCancel: {
      token.cancel()
    }
  }
}
