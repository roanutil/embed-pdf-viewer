internal import EmbedPDFBindings
import Foundation

/// What every viewer and capability call throws.
///
/// `code` is one of the kernel's nine plugin error codes, the
/// `PluginErrorCode` vocabulary of the TypeScript kernel's `toPluginError`:
///
/// - `permission-denied`: the scope, the document's permissions or its
///   password refused it (a wrong `unlock` password is this).
/// - `unsupported`: the plugin isn't in the `ViewerConfig`, or the engine
///   can't do it.
/// - `not-found`: no such document, page, annotation or other resource.
/// - `not-ready`: the document is still loading, is locked, or failed.
/// - `invalid-input`: an argument, or bytes that aren't a PDF.
/// - `conflict`: the document changed under the call.
/// - `instance-closed`: the document closed or the viewer shut down.
/// - `operation-cancelled`: the task was cancelled.
/// - `operation-failed`: anything else.
///
/// One more code is the bindings' own: `invalid-state`, a value that didn't
/// survive the trip across the boundary (a record the kernel answered
/// without a field, a count past `UInt32`, a raster Core Graphics refused).
/// It is a bug to report, not something a caller did.
///
/// An engine's own code (`NotFound`, `DocPasswordIncorrect`) never appears
/// here: it maps as `toPluginError` maps it, and a wire error's `detail`
/// still begins with it. `name` says what raised the error (`PluginError`,
/// `EngineError`, `AbortError`, or a foreign error's type), and is for
/// logging rather than matching. `code` is optional only so a handler can
/// build one without a code; nothing the viewer throws leaves it `nil`.
public struct PDFError: Error, Sendable, Equatable {
  public let name: String
  public let code: String?
  public let detail: String

  init(_ error: ViewerError) {
    switch error {
    case let .Kernel(name, code, detail):
      self.name = name
      self.code = code
      self.detail = detail
    case .Closed:
      name = "Closed"
      code = "instance-closed"
      detail = "The viewer has shut down."
    }
  }

  init(_ error: any Error) {
    if let error = error as? ViewerError {
      self.init(error)
    } else if let error = error as? PDFError {
      self = error
    } else if error is CancellationError {
      self = PDFError(name: "AbortError", code: "operation-cancelled", detail: "operation cancelled")
    } else {
      self = PDFError(name: String(describing: type(of: error)), code: "operation-failed", detail: readable(error))
    }
  }

  /// For a handler or port that fails with a kernel code of its own, such
  /// as `permission-denied`. The completion it fails passes the code on.
  public init(name: String, code: String?, detail: String) {
    self.name = name
    self.code = code
    self.detail = detail
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
