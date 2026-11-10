/// A tool's id in a document's interaction hub, which every plugin's tools
/// join: the annotation plugin's authoring tools, the form plugin's field
/// tools, and the hub's own. `rawValue` is the wire string.
///
/// The set is open, so this is a struct rather than an enum, the way
/// `Notification.Name` is: the constants are the built-in tools, and an app's
/// own tool, registered through `InteractionCapability.registerTool(_:replace:)`
/// or the annotation settings' tool table, is a string literal or
/// `ToolID("laser")`. A tool a newer engine adds needs no new constant to
/// pass, and adding one breaks no caller's `switch`.
public struct ToolID: RawRepresentable, Hashable, Sendable, Codable,
  ExpressibleByStringLiteral, CustomStringConvertible
{
  public let rawValue: String

  public init(rawValue: String) { self.rawValue = rawValue }

  public init(_ rawValue: String) { self.rawValue = rawValue }

  public init(stringLiteral rawValue: String) { self.rawValue = rawValue }

  public var description: String { rawValue }

  // The hub's own.
  /// `pointer`: selects text, edits annotations, fills forms and follows
  /// links. The default tool.
  public static let pointer = ToolID("pointer")
  public static let pan = ToolID("pan")
  public static let zoom = ToolID("zoom")
  // The render plugin's.
  public static let snapshot = ToolID("snapshot")
  // The form plugin's.
  public static let formFill = ToolID("form-fill")
  public static let formEdit = ToolID("form-edit")
  public static let formText = ToolID("form-text")
  public static let formCheckbox = ToolID("form-checkbox")
  public static let formRadio = ToolID("form-radio")
  public static let formCombobox = ToolID("form-combobox")
  public static let formListbox = ToolID("form-listbox")
  public static let formButton = ToolID("form-button")
  public static let formSignature = ToolID("form-signature")
  // The annotation plugin's.
  public static let square = ToolID("square")
  public static let circle = ToolID("circle")
  public static let line = ToolID("line")
  public static let distance = ToolID("distance")
  public static let calibrate = ToolID("calibrate")
  public static let polygon = ToolID("polygon")
  public static let polyline = ToolID("polyline")
  public static let perimeter = ToolID("perimeter")
  public static let area = ToolID("area")
  public static let ink = ToolID("ink")
  public static let inkHighlight = ToolID("ink-highlight")
  public static let freeText = ToolID("free-text")
  public static let freeTextCallout = ToolID("free-text-callout")
  public static let highlight = ToolID("highlight")
  public static let redact = ToolID("redact")
  public static let underline = ToolID("underline")
  public static let strikeout = ToolID("strikeout")
  public static let squiggly = ToolID("squiggly")
  public static let insertText = ToolID("insert-text")
  public static let replaceText = ToolID("replace-text")
  public static let stamp = ToolID("stamp")
  public static let link = ToolID("link")
  public static let note = ToolID("note")
  public static let attachment = ToolID("attachment")

  /// Every constant this type names.
  public static let named: [ToolID] = [
    .pointer, .pan, .zoom, .snapshot, .formFill, .formEdit, .formText, .formCheckbox, .formRadio,
    .formCombobox, .formListbox, .formButton, .formSignature, .square, .circle, .line, .distance,
    .calibrate, .polygon, .polyline, .perimeter, .area, .ink, .inkHighlight, .freeText,
    .freeTextCallout, .highlight, .redact, .underline, .strikeout, .squiggly, .insertText,
    .replaceText, .stamp, .link, .note, .attachment,
  ]
}
