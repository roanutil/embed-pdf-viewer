public import EmbedPDFBindings

/// Selects workspace plugins and the access scope for documents opened by a viewer.
///
/// All plugin flags default to `false` and optional plugin configurations to
/// `nil`. Installing a plugin does not grant its operations permission.
/// `scope: nil` requests the wildcard scope; supply explicit scope strings
/// when the application needs to restrict operations. Embedded PDF permission
/// bits contribute grants only through the `pdf.permissions` scope; wildcard
/// and explicit capability grants do not enforce those bits. Signature
/// protection applies independently of these grants.
///
/// See <doc:ConfiguringPlugins> for dependencies and working configurations.
public typealias ViewerConfig = EmbedPDFBindings.ViewerConfig
/// A value snapshot of an open document tab, identified by `id`.
///
/// `name` is optional, `pageCount` is populated when ready, and `status` is
/// one of ``DocumentStatus``'s states. A failed tab carries `error` details.
/// Get a fresh value from ``PDFViewer/documents()`` after unlocking or changing
/// the document; mutating this value does not change the viewer.
public typealias DocumentInfo = EmbedPDFBindings.DocumentInfo
/// The pre-rename name of `DocumentInfo`, kept for one release. It collides
/// with PDFKit's `PDFDocument` in any file that imports both modules.
@available(*, deprecated, renamed: "DocumentInfo")
public typealias PDFDocument = DocumentInfo
/// The state of a document tab: `.loading`, `.locked`, `.ready`, or `.failed`.
///
/// A locked tab needs ``PDFViewer/unlock(_:password:)`` before its document
/// plugins are usable. A failed tab's ``DocumentInfo`` supplies the error.
public typealias DocumentStatus = EmbedPDFBindings.DocumentStatus
/// A page layout snapshot containing identity, order, unrotated size, and rotation.
///
/// `page` is the durable ``PageRef`` within the open document, `index` is
/// zero-based, and `width` and `height` are unrotated crop dimensions in PDF
/// points. `rotation` is clockwise degrees: 0, 90, 180, or 270.
/// Refresh the layout after page edits rather than caching indices as identity.
public typealias PageInfo = EmbedPDFBindings.PageInfo
/// A page's PDF object number, stable across reordering within the open document.
///
/// Obtain this from ``PageInfo`` rather than constructing one from a page index.
/// A reference is scoped to its document and does not remain usable after deletion
/// or closure. See <doc:CoordinatesAndPages>.
public typealias PageRef = EmbedPDFBindings.PageRef
/// An `x` and `y` coordinate in the space specified by the receiving operation.
///
/// Page coordinates use the unrotated crop's top-left origin with increasing
/// y downward. Stage viewport coordinates use container layout units instead.
/// See <doc:CoordinatesAndPages> before crossing coordinate spaces.
public typealias Point = EmbedPDFBindings.Point
/// A width and height in the units specified by the receiving operation.
public typealias Size = EmbedPDFBindings.Size
/// An origin and extent with `x`, `y`, `width`, and `height` fields.
///
/// Page rectangles use unrotated, crop-relative, y-down PDF-point coordinates.
/// A stage conversion can return a rectangle in viewport units instead.
public typealias Rect = EmbedPDFBindings.Rect
/// Four page-space corners named in the text's own upright frame.
///
/// `upperLeft` and `upperRight` follow the ascent edge; `lowerLeft` and
/// `lowerRight` follow the opposite edge. Left to right follows reading
/// direction, which need not align with the page's horizontal axis.
public typealias Quad = EmbedPDFBindings.Quad
/// An error snapshot with a stable `code`, human-readable `message`, the
/// reporting `capability`, and an optional `permission` the session lacks.
public typealias PluginErrorInfo = EmbedPDFBindings.PluginErrorInfo
/// A failure returned by an application-provided host callback.
///
/// Use `HostFailure(error)` to preserve a Swift error's supported code and
/// description when completing a foreign callback.
public typealias HostError = EmbedPDFBindings.HostError
/// The name, optional code, and detail reported when a host callback fails.
public typealias HostFailure = EmbedPDFBindings.HostFailure
/// A document lifecycle change: opened, open failed, locked, closed, active changed, or pages changed.
///
/// Consume ``PDFViewer/documentEvents`` before starting the operation to observe
/// its events. A locked event's `passwordProvided` distinguishes an omitted
/// password from an attempt that supplied one.
public typealias DocumentEvent = EmbedPDFBindings.DocumentEvent
/// Chooses incremental serialization or a full rewrite of an unsigned document.
///
/// `.incremental` appends a revision while preserving earlier signed byte
/// ranges. `.rewrite` discards revision history and is refused for signed
/// documents. Neither mode writes a file; ``PDFViewer/save(_:mode:)`` returns bytes.
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
