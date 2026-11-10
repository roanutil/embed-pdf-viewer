/// An annotation's subtype, the PDF's own `/Subtype` as the engine spells
/// it. `rawValue` is the wire string, and every string round-trips: one
/// this enum doesn't name becomes `unknown`.
///
/// The set is open. The named cases are the engine's subtype catalog, but
/// the annotation plugin takes any kind name, and a newer engine can add
/// subtypes.
public enum AnnotationSubtype: Sendable, Hashable, RawRepresentable, Codable {
  case text
  case link
  case freeText
  case line
  case square
  case circle
  case polygon
  case polyline
  case highlight
  case underline
  case squiggly
  case strikeout
  case stamp
  case caret
  case ink
  case popup
  case fileAttachment
  case redact
  case widget
  /// An annotation whose PDF subtype the engine has no reader for.
  case unsupported
  /// A subtype this enum doesn't name, kept as the engine spells it.
  case unknown(String)

  /// Every named case, in the engine catalog's order.
  public static let named: [AnnotationSubtype] = [
    .text, .link, .freeText, .line, .square, .circle, .polygon, .polyline, .highlight, .underline,
    .squiggly, .strikeout, .stamp, .caret, .ink, .popup, .fileAttachment, .redact, .widget,
    .unsupported,
  ]

  public init(rawValue: String) {
    self = Self.named.first { $0.rawValue == rawValue } ?? .unknown(rawValue)
  }

  public var rawValue: String {
    switch self {
    case .text: "text"
    case .link: "link"
    case .freeText: "free-text"
    case .line: "line"
    case .square: "square"
    case .circle: "circle"
    case .polygon: "polygon"
    case .polyline: "polyline"
    case .highlight: "highlight"
    case .underline: "underline"
    case .squiggly: "squiggly"
    case .strikeout: "strikeout"
    case .stamp: "stamp"
    case .caret: "caret"
    case .ink: "ink"
    case .popup: "popup"
    case .fileAttachment: "file-attachment"
    case .redact: "redact"
    case .widget: "widget"
    case .unsupported: "unsupported"
    case let .unknown(subtype): subtype
    }
  }

  /// `unknown("ink")` and `.ink` are the same subtype.
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
