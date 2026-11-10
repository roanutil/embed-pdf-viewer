internal import EmbedPDFBindings
import Foundation

/// What every viewer and capability call throws.
///
/// `code` is a ``Code``: one of the kernel's nine plugin error codes, the
/// `PluginErrorCode` vocabulary of the TypeScript kernel's `toPluginError`,
/// the bindings' own `invalid-state`, or any other string the kernel adds
/// later. See ``Code`` for what each one means.
///
/// An engine's own code (`NotFound`, `DocPasswordIncorrect`) never appears
/// here: it maps as `toPluginError` maps it, and `detail` is the engine's
/// message with no code in front. `name` says what raised the error (`PluginError`,
/// `EngineError`, `AbortError`, or a foreign error's type), and is for
/// logging rather than matching.
///
/// Match a code in a `catch` the way `URLError` allows:
///
/// ```swift
/// do {
///   try await viewer.unlock(document, password: password)
/// } catch PDFError.Code.permissionDenied {
///   // Wrong password.
/// }
/// ```
public struct PDFError: Error, Sendable, Equatable {
  /// A kernel error code. `rawValue` is the wire string.
  ///
  /// The set is open, so this is a struct rather than an enum, the way
  /// `URLError.Code` is: a code the kernel adds later needs no new constant
  /// to round-trip, and adding one breaks no caller's `switch`. A string
  /// literal is a code: `error.code == "quota-exceeded"`.
  public struct Code: RawRepresentable, Hashable, Sendable, Codable, ExpressibleByStringLiteral,
    CustomStringConvertible
  {
    public let rawValue: String

    public init(rawValue: String) { self.rawValue = rawValue }

    public init(_ rawValue: String) { self.rawValue = rawValue }

    public init(stringLiteral rawValue: String) { self.rawValue = rawValue }

    public var description: String { rawValue }

    /// `permission-denied`: the scope, the document's permissions or its
    /// password refused it (a wrong `unlock` password is this).
    public static let permissionDenied = Code("permission-denied")
    /// `unsupported`: the plugin isn't in the `ViewerConfig`, or the engine
    /// can't do it.
    public static let unsupported = Code("unsupported")
    /// `not-found`: no such document, page, annotation or other resource.
    public static let notFound = Code("not-found")
    /// `not-ready`: the document is still loading, is locked, or failed.
    public static let notReady = Code("not-ready")
    /// `invalid-input`: an argument, or bytes that aren't a PDF.
    public static let invalidInput = Code("invalid-input")
    /// `conflict`: the document changed under the call.
    public static let conflict = Code("conflict")
    /// `instance-closed`: the document closed or the viewer shut down.
    public static let instanceClosed = Code("instance-closed")
    /// `operation-cancelled`: the task was cancelled.
    public static let operationCancelled = Code("operation-cancelled")
    /// `operation-failed`: anything else.
    public static let operationFailed = Code("operation-failed")
    /// `invalid-state`, the bindings' own code: a value that didn't survive
    /// the trip across the boundary (a record the kernel answered without a
    /// field, a count past `UInt32`, a raster Core Graphics refused). It is
    /// a bug to report, not something a caller did.
    public static let invalidState = Code("invalid-state")

    /// Matches a thrown error whose code is this one, so a `catch` can name
    /// the code: `catch PDFError.Code.notFound`.
    public static func ~= (code: Code, error: any Error) -> Bool {
      (error as? PDFError)?.code == code
    }

    /// Matches a `PDFError` whose code is this one, for a `catch` in a
    /// `throws(PDFError)` context and a `switch` over the error.
    public static func ~= (code: Code, error: PDFError) -> Bool {
      error.code == code
    }
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
  /// `name` is for logging; most callers keep `PluginError`.
  public init(_ code: Code, detail: String, name: String = "PluginError") {
    self.init(name: name, code: code, detail: detail)
  }

  /// ``init(_:detail:name:)`` with every member named.
  public init(name: String, code: Code, detail: String) {
    self.name = name
    self.code = code
    self.detail = detail
  }

  /// The pre-``Code`` spelling, for a handler or port that still passes a
  /// string. A `nil` code becomes ``Code/operationFailed``.
  @available(*, deprecated, message: "Pass a PDFError.Code: PDFError(.notFound, detail:), or a string literal.")
  @_disfavoredOverload
  public init(name: String, code: String?, detail: String) {
    self.init(name: name, code: code.map(Code.init(rawValue:)) ?? .operationFailed, detail: detail)
  }
}

extension PDFError: LocalizedError {
  /// `detail`, so `localizedDescription` and SwiftUI's `alert(isPresented:error:)`
  /// show the engine's message rather than a bridged error number.
  public var errorDescription: String? { detail }
}

extension PDFError: CustomStringConvertible {
  /// The code and the detail, for logs: `not-found: no page p9`.
  public var description: String { "\(code.rawValue): \(detail)" }
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
