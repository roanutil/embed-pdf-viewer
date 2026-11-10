/// A permission `PDFViewer.allows(_:in:)` asks the document's scope about.
/// `rawValue` is the wire string.
///
/// The set is open, so this is a struct rather than an enum: the constants
/// are the engine's document capabilities (`doc.*`) and `annotations:create`,
/// a plugin's own permission is a string literal
/// (`"protected-display.view"`), and a newer engine can add permissions
/// without breaking a caller's `switch`.
public struct DocumentPermission: RawRepresentable, Hashable, Sendable, Codable,
  ExpressibleByStringLiteral, CustomStringConvertible
{
  public let rawValue: String

  public init(rawValue: String) { self.rawValue = rawValue }

  public init(_ rawValue: String) { self.rawValue = rawValue }

  public init(stringLiteral rawValue: String) { self.rawValue = rawValue }

  public var description: String { rawValue }

  public static let open = DocumentPermission("doc.open")
  public static let render = DocumentPermission("doc.render")
  /// The thumbnail pictures only; `render` implies it.
  public static let renderThumbnail = DocumentPermission("doc.render.thumbnail")
  public static let textSelect = DocumentPermission("doc.text.select")
  public static let textCopy = DocumentPermission("doc.text.copy")
  public static let textSearch = DocumentPermission("doc.text.search")
  public static let contentCopy = DocumentPermission("doc.content.copy")
  public static let download = DocumentPermission("doc.download")
  public static let downloadFlattened = DocumentPermission("doc.download.flattened")
  public static let print = DocumentPermission("doc.print")
  public static let printHigh = DocumentPermission("doc.print.high")
  public static let pagesModify = DocumentPermission("doc.pages.modify")
  public static let pagesAssemble = DocumentPermission("doc.pages.assemble")
  public static let formsRead = DocumentPermission("doc.forms.read")
  public static let formsFill = DocumentPermission("doc.forms.fill")
  public static let formsModify = DocumentPermission("doc.forms.modify")
  public static let formsImport = DocumentPermission("doc.forms.import")
  public static let formsScript = DocumentPermission("doc.forms.script")
  public static let formsSubmit = DocumentPermission("doc.forms.submit")
  public static let annotateRead = DocumentPermission("doc.annotate.read")
  public static let annotateModify = DocumentPermission("doc.annotate.modify")
  public static let annotateImport = DocumentPermission("doc.annotate.import")
  public static let metadataModify = DocumentPermission("doc.metadata.modify")
  public static let attachmentsModify = DocumentPermission("doc.attachments.modify")
  public static let outlineModify = DocumentPermission("doc.outline.modify")
  public static let redact = DocumentPermission("doc.redact")
  public static let sign = DocumentPermission("doc.sign")
  public static let signCertify = DocumentPermission("doc.sign.certify")
  /// `annotations:create`: whether this caller may create annotations of
  /// their own.
  public static let annotationsCreate = DocumentPermission("annotations:create")

  /// Every constant this type names, the `doc.*` ones in the engine's order.
  public static let named: [DocumentPermission] = [
    .open, .render, .renderThumbnail, .textSelect, .textCopy, .textSearch, .contentCopy, .download,
    .downloadFlattened, .print, .printHigh, .pagesModify, .pagesAssemble, .formsRead, .formsFill,
    .formsModify, .formsImport, .formsScript, .formsSubmit, .annotateRead, .annotateModify,
    .annotateImport, .metadataModify, .attachmentsModify, .outlineModify, .redact, .sign,
    .signCertify, .annotationsCreate,
  ]
}
