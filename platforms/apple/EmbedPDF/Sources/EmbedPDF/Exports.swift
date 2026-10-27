public import EmbedPDFBindings

public typealias ViewerConfig = EmbedPDFBindings.ViewerConfig
public typealias DocumentInfo = EmbedPDFBindings.DocumentInfo
/// The pre-rename name of `DocumentInfo`, kept for one release. It collides
/// with PDFKit's `PDFDocument` in any file that imports both modules.
@available(*, deprecated, renamed: "DocumentInfo")
public typealias PDFDocument = DocumentInfo
public typealias DocumentStatus = EmbedPDFBindings.DocumentStatus
public typealias PageInfo = EmbedPDFBindings.PageInfo
public typealias PageRef = EmbedPDFBindings.PageRef
public typealias Point = EmbedPDFBindings.Point
public typealias Size = EmbedPDFBindings.Size
public typealias Rect = EmbedPDFBindings.Rect
public typealias Quad = EmbedPDFBindings.Quad
public typealias PluginErrorInfo = EmbedPDFBindings.PluginErrorInfo
public typealias HostError = EmbedPDFBindings.HostError
public typealias HostFailure = EmbedPDFBindings.HostFailure
public typealias DocumentEvent = EmbedPDFBindings.DocumentEvent
public typealias SaveMode = EmbedPDFBindings.SaveMode

extension HostFailure {
  /// What a completion reports for `error`: a `PDFError` (or `ViewerError`)
  /// keeps its name, code and message, a `CancellationError` is
  /// `operation-cancelled`, and anything else is `operation-failed` under
  /// its type's name, with its `LocalizedError` description or its
  /// `description` as the message.
  public init(_ error: any Error) {
    switch error {
    case let error as PDFError:
      self.init(name: error.name, code: error.code, detail: error.detail)
    case let error as ViewerError:
      let error = PDFError(error)
      self.init(name: error.name, code: error.code, detail: error.detail)
    case is CancellationError:
      self.init(name: "AbortError", code: "operation-cancelled", detail: "operation cancelled")
    default:
      self.init(name: String(describing: type(of: error)), code: "operation-failed", detail: readable(error))
    }
  }
}
