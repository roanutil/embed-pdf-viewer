/// A tool's id in a document's interaction hub, which every plugin's tools
/// join: the annotation plugin's authoring tools, the form plugin's field
/// tools, and the hub's own. `rawValue` is the wire string, and every string
/// round-trips: one this enum doesn't name becomes `unknown`.
///
/// The set is open. The named cases are the built-in tools; an app adds its
/// own through `InteractionCapability.registerTool(_:replace:)` or the
/// annotation settings' tool table, and passes those as `unknown`.
public enum ToolID: Sendable, Hashable, RawRepresentable, Codable {
  // The hub's own.
  /// `pointer`: selects text, edits annotations, fills forms and follows
  /// links. The default tool.
  case pointer
  case pan
  case zoom
  // The render plugin's.
  case snapshot
  // The form plugin's.
  case formFill
  case formEdit
  case formText
  case formCheckbox
  case formRadio
  case formCombobox
  case formListbox
  case formButton
  case formSignature
  // The annotation plugin's.
  case square
  case circle
  case line
  case distance
  case calibrate
  case polygon
  case polyline
  case perimeter
  case area
  case ink
  case inkHighlight
  case freeText
  case freeTextCallout
  case highlight
  case redact
  case underline
  case strikeout
  case squiggly
  case insertText
  case replaceText
  case stamp
  case link
  case note
  case attachment
  /// A tool this enum doesn't name, such as one an app registered.
  case unknown(String)

  /// Every named case.
  public static let named: [ToolID] = [
    .pointer, .pan, .zoom, .snapshot, .formFill, .formEdit, .formText, .formCheckbox, .formRadio,
    .formCombobox, .formListbox, .formButton, .formSignature, .square, .circle, .line, .distance,
    .calibrate, .polygon, .polyline, .perimeter, .area, .ink, .inkHighlight, .freeText,
    .freeTextCallout, .highlight, .redact, .underline, .strikeout, .squiggly, .insertText,
    .replaceText, .stamp, .link, .note, .attachment,
  ]

  public init(rawValue: String) {
    self = Self.named.first { $0.rawValue == rawValue } ?? .unknown(rawValue)
  }

  public var rawValue: String {
    switch self {
    case .pointer: "pointer"
    case .pan: "pan"
    case .zoom: "zoom"
    case .snapshot: "snapshot"
    case .formFill: "form-fill"
    case .formEdit: "form-edit"
    case .formText: "form-text"
    case .formCheckbox: "form-checkbox"
    case .formRadio: "form-radio"
    case .formCombobox: "form-combobox"
    case .formListbox: "form-listbox"
    case .formButton: "form-button"
    case .formSignature: "form-signature"
    case .square: "square"
    case .circle: "circle"
    case .line: "line"
    case .distance: "distance"
    case .calibrate: "calibrate"
    case .polygon: "polygon"
    case .polyline: "polyline"
    case .perimeter: "perimeter"
    case .area: "area"
    case .ink: "ink"
    case .inkHighlight: "ink-highlight"
    case .freeText: "free-text"
    case .freeTextCallout: "free-text-callout"
    case .highlight: "highlight"
    case .redact: "redact"
    case .underline: "underline"
    case .strikeout: "strikeout"
    case .squiggly: "squiggly"
    case .insertText: "insert-text"
    case .replaceText: "replace-text"
    case .stamp: "stamp"
    case .link: "link"
    case .note: "note"
    case .attachment: "attachment"
    case let .unknown(id): id
    }
  }

  /// `unknown("pan")` and `.pan` are the same tool.
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
