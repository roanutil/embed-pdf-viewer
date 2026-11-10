/// An annotation's subtype, the PDF's own `/Subtype` as the engine spells
/// it. `rawValue` is the wire string.
///
/// The set is open, so this is a struct rather than an enum: the constants
/// are the engine's subtype catalog, but the annotation plugin takes any kind
/// name, and a newer engine can add subtypes without breaking a caller's
/// `switch`. Any other subtype is a string literal or
/// `AnnotationSubtype("sound")`.
public struct AnnotationSubtype: RawRepresentable, Hashable, Sendable, Codable,
  ExpressibleByStringLiteral, CustomStringConvertible
{
  public let rawValue: String

  public init(rawValue: String) { self.rawValue = rawValue }

  public init(_ rawValue: String) { self.rawValue = rawValue }

  public init(stringLiteral rawValue: String) { self.rawValue = rawValue }

  public var description: String { rawValue }

  public static let text = AnnotationSubtype("text")
  public static let link = AnnotationSubtype("link")
  public static let freeText = AnnotationSubtype("free-text")
  public static let line = AnnotationSubtype("line")
  public static let square = AnnotationSubtype("square")
  public static let circle = AnnotationSubtype("circle")
  public static let polygon = AnnotationSubtype("polygon")
  public static let polyline = AnnotationSubtype("polyline")
  public static let highlight = AnnotationSubtype("highlight")
  public static let underline = AnnotationSubtype("underline")
  public static let squiggly = AnnotationSubtype("squiggly")
  public static let strikeout = AnnotationSubtype("strikeout")
  public static let stamp = AnnotationSubtype("stamp")
  public static let caret = AnnotationSubtype("caret")
  public static let ink = AnnotationSubtype("ink")
  public static let popup = AnnotationSubtype("popup")
  public static let fileAttachment = AnnotationSubtype("file-attachment")
  public static let redact = AnnotationSubtype("redact")
  public static let widget = AnnotationSubtype("widget")
  /// An annotation whose PDF subtype the engine has no reader for.
  public static let unsupported = AnnotationSubtype("unsupported")

  /// Every constant this type names, in the engine catalog's order.
  public static let named: [AnnotationSubtype] = [
    .text, .link, .freeText, .line, .square, .circle, .polygon, .polyline, .highlight, .underline,
    .squiggly, .strikeout, .stamp, .caret, .ink, .popup, .fileAttachment, .redact, .widget,
    .unsupported,
  ]
}
