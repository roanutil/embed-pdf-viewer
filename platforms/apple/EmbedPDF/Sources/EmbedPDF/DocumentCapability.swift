/// A right `PDFViewer.allows(_:in:)` asks the document's scope about.
/// `rawValue` is the wire string, and every string round-trips: one this
/// enum doesn't name becomes `unknown`.
///
/// The set is open. The named cases are the engine's document capabilities
/// (`doc.*`) and `annotations:create`; a plugin's own permission
/// (`protected-display.view`) passes as `unknown`, and a newer engine can
/// add capabilities.
public enum DocumentCapability: Sendable, Hashable, RawRepresentable, Codable {
  case open
  case render
  /// The thumbnail pictures only; `render` implies it.
  case renderThumbnail
  case textSelect
  case textCopy
  case textSearch
  case contentCopy
  case download
  case downloadFlattened
  case print
  case printHigh
  case pagesModify
  case pagesAssemble
  case formsRead
  case formsFill
  case formsModify
  case formsImport
  case formsScript
  case formsSubmit
  case annotateRead
  case annotateModify
  case annotateImport
  case metadataModify
  case attachmentsModify
  case outlineModify
  case redact
  case sign
  case signCertify
  /// `annotations:create`: whether this caller may create annotations of
  /// their own.
  case annotationsCreate
  /// A capability this enum doesn't name, kept as the engine spells it.
  case unknown(String)

  /// Every named case, the `doc.*` ones in the engine's order.
  public static let named: [DocumentCapability] = [
    .open, .render, .renderThumbnail, .textSelect, .textCopy, .textSearch, .contentCopy, .download,
    .downloadFlattened, .print, .printHigh, .pagesModify, .pagesAssemble, .formsRead, .formsFill,
    .formsModify, .formsImport, .formsScript, .formsSubmit, .annotateRead, .annotateModify,
    .annotateImport, .metadataModify, .attachmentsModify, .outlineModify, .redact, .sign,
    .signCertify, .annotationsCreate,
  ]

  public init(rawValue: String) {
    self = Self.named.first { $0.rawValue == rawValue } ?? .unknown(rawValue)
  }

  public var rawValue: String {
    switch self {
    case .open: "doc.open"
    case .render: "doc.render"
    case .renderThumbnail: "doc.render.thumbnail"
    case .textSelect: "doc.text.select"
    case .textCopy: "doc.text.copy"
    case .textSearch: "doc.text.search"
    case .contentCopy: "doc.content.copy"
    case .download: "doc.download"
    case .downloadFlattened: "doc.download.flattened"
    case .print: "doc.print"
    case .printHigh: "doc.print.high"
    case .pagesModify: "doc.pages.modify"
    case .pagesAssemble: "doc.pages.assemble"
    case .formsRead: "doc.forms.read"
    case .formsFill: "doc.forms.fill"
    case .formsModify: "doc.forms.modify"
    case .formsImport: "doc.forms.import"
    case .formsScript: "doc.forms.script"
    case .formsSubmit: "doc.forms.submit"
    case .annotateRead: "doc.annotate.read"
    case .annotateModify: "doc.annotate.modify"
    case .annotateImport: "doc.annotate.import"
    case .metadataModify: "doc.metadata.modify"
    case .attachmentsModify: "doc.attachments.modify"
    case .outlineModify: "doc.outline.modify"
    case .redact: "doc.redact"
    case .sign: "doc.sign"
    case .signCertify: "doc.sign.certify"
    case .annotationsCreate: "annotations:create"
    case let .unknown(capability): capability
    }
  }

  /// `unknown("doc.print")` and `.print` are the same capability.
  public static func == (lhs: Self, rhs: Self) -> Bool { lhs.rawValue == rhs.rawValue }

  public func hash(into hasher: inout Hasher) { hasher.combine(rawValue) }

  public init(from decoder: any Decoder) throws {
    self.init(rawValue: try decoder.singleValueContainer().decode(String.self))
  }

  public func encode(to encoder: any Encoder) throws {
    var container = encoder.singleValueContainer()
    try container.encode(rawValue)
  }
}
