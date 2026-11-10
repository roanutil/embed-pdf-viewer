public import EmbedPDFBindings
import Foundation

// Typed create drafts and patches for ink, highlight and note annotations.
//
// Each type encodes to the engine's field bag for its subtype, the JSON
// object `AnnotationCapability.create(_:on:tool:select:)`,
// `update(_:_:)` and `AnnotationToolsCapability.updateDefaults(_:for:)`
// otherwise take as a string. The engine refuses a key its kind doesn't
// declare (`assert_declared_fields` in
// `crates/epdf-engine/src/features/annotation/write/prepare.rs` and
// `write/patch.rs`), so every key here is one the kind declares
// (`kind_fields` in `prepare.rs`), and each value has the shape that
// kind's `read_field` reads. Geometry is page space, as reads hand it out:
// points `{x, y}` and boxes `{x, y, width, height}` from the page's
// top-left; the plugin moves them into the file's coordinates
// (`convert` in `crates/plugins/annotation-core/src/engine/space.rs`).
//
// A member left `nil` is left out of the JSON: on a draft that is the
// engine's default, on a patch it keeps the current value. A member the
// engine can set to nothing is a `Nullable`, whose `.null`
// writes `null`.
//
// The JSON-string overloads stay for every field and subtype these types
// don't cover.

/// A typed create draft: the engine's field bag for one subtype, with its
/// `subtype`. `AnnotationCapability.create(_:on:tool:select:)` takes one.
public protocol AnnotationDraft: Encodable, Sendable {}

/// A typed patch: the members of one subtype to change.
/// `AnnotationCapability.update(_:_:)` and
/// `AnnotationToolsCapability.updateDefaults(_:for:)` take one.
public protocol AnnotationPatch: Encodable, Sendable {}

extension AnnotationDraft {
  /// The field bag as the engine reads it: keys sorted, so the same draft is
  /// always the same bytes. Throws `invalid-input` for a number JSON can't
  /// hold (NaN or an infinity).
  public func json() throws(PDFError) -> String {
    try AnnotationFieldBag.json(of: self)
  }
}

extension AnnotationPatch {
  /// The field bag as the engine reads it: keys sorted, unset members left
  /// out. Throws `invalid-input` for a number JSON can't hold (NaN or an
  /// infinity).
  public func json() throws(PDFError) -> String {
    try AnnotationFieldBag.json(of: self)
  }
}

/// A value, or JSON `null`: a member a draft or patch can set to nothing.
/// `.null` writes `null`. Leaving the member itself `nil` writes nothing,
/// which on a draft is the engine's default and on a patch keeps the
/// current value (naming.md, "Nothing: left out or null").
///
/// A literal of `Value`'s own kind is `.value`: `color: "#ff0000"`,
/// `rotation: 90`, `dashArray: [3, 2]`.
public enum Nullable<Value: Encodable & Sendable & Hashable>: Encodable, Sendable, Hashable {
  case value(Value)
  /// Nothing: no stroke colour, no dash pattern, no rotation.
  case null

  public func encode(to encoder: any Encoder) throws {
    var container = encoder.singleValueContainer()
    switch self {
    case let .value(value): try container.encode(value)
    case .null: try container.encodeNil()
    }
  }
}

extension Nullable: ExpressibleByUnicodeScalarLiteral
where Value: ExpressibleByStringLiteral, Value.StringLiteralType == String {
  public init(unicodeScalarLiteral value: String) { self = .value(Value(stringLiteral: value)) }
}

extension Nullable: ExpressibleByExtendedGraphemeClusterLiteral
where Value: ExpressibleByStringLiteral, Value.StringLiteralType == String {
  public init(extendedGraphemeClusterLiteral value: String) { self = .value(Value(stringLiteral: value)) }
}

extension Nullable: ExpressibleByStringLiteral
where Value: ExpressibleByStringLiteral, Value.StringLiteralType == String {
  public init(stringLiteral value: String) { self = .value(Value(stringLiteral: value)) }
}

extension Nullable: ExpressibleByFloatLiteral where Value == Double {
  public init(floatLiteral value: Double) { self = .value(value) }
}

extension Nullable: ExpressibleByIntegerLiteral where Value == Double {
  public init(integerLiteral value: Int) { self = .value(Double(value)) }
}

extension Nullable: ExpressibleByArrayLiteral where Value == [Double] {
  public init(arrayLiteral elements: Double...) { self = .value(elements) }
}

/// The border styles the engine writes for an ink annotation. It reads
/// `beveled` and `inset` too, but refuses to write them on anything but a
/// widget (`read_field`, `"borderStyle"`, in
/// `crates/epdf-engine/src/features/annotation/write/prepare.rs`).
public enum AnnotationBorderStyle: String, Encodable, Sendable, Hashable, CaseIterable {
  case solid
  case dashed
}

/// An ink annotation's `/IT`: the one intent the engine writes for ink
/// (`intents_of` in `prepare.rs`). An ink without one is pen ink.
public enum AnnotationInkIntent: String, Encodable, Sendable, Hashable, CaseIterable {
  /// `ink-highlight`: a freehand highlighter.
  case inkHighlight = "ink-highlight"
}

/// A note's `/Name` icon, as the engine spells it (`NoteIcon` in
/// `crates/epdf-engine/src/features/annotation/vocabulary.rs`).
public enum AnnotationNoteIcon: String, Encodable, Sendable, Hashable, CaseIterable {
  case comment
  case key
  case note
  case help
  case newParagraph = "new-paragraph"
  case paragraph
  case insert
}

// ── ink ─────────────────────────────────────────────────────────────────────

/// A new ink annotation: freehand strokes. Each member's rule is the
/// engine's (`read_field` in `prepare.rs`); a value outside it throws
/// `invalid-input` from the create.
public struct InkAnnotationDraft: AnnotationDraft, Hashable {
  /// `/InkList`, page space: one array of points per stroke. At least one
  /// stroke, and no stroke without points.
  public var inkList: [[Point]]
  /// `/C`. `.null` draws no stroke; left out takes the tool's or the
  /// engine's default.
  public var color: Nullable<AnnotationColor>?
  /// `/CA`, 0 to 1.
  public var opacity: Double?
  /// The line width, above 0.
  public var strokeWidth: Double?
  public var borderStyle: AnnotationBorderStyle?
  /// The dash lengths, each 0 or more, at least one. A dashed border with
  /// this left out takes the engine's default pattern; `.null` is none.
  public var dashArray: Nullable<[Double]>?
  /// `.null` is pen ink.
  public var intent: Nullable<AnnotationInkIntent>?
  /// Degrees. `.null` is none.
  public var rotation: Nullable<Double>?
  /// `/Contents`. `.null` is none.
  public var contents: Nullable<String>?

  public init(
    inkList: [[Point]], color: Nullable<AnnotationColor>? = nil, opacity: Double? = nil,
    strokeWidth: Double? = nil, borderStyle: AnnotationBorderStyle? = nil,
    dashArray: Nullable<[Double]>? = nil,
    intent: Nullable<AnnotationInkIntent>? = nil,
    rotation: Nullable<Double>? = nil, contents: Nullable<String>? = nil
  ) {
    self.inkList = inkList
    self.color = color
    self.opacity = opacity
    self.strokeWidth = strokeWidth
    self.borderStyle = borderStyle
    self.dashArray = dashArray
    self.intent = intent
    self.rotation = rotation
    self.contents = contents
  }

  private enum CodingKeys: String, CodingKey {
    case subtype, inkList, color, opacity, strokeWidth, borderStyle, dashArray, intent, rotation,
      contents
  }

  public func encode(to encoder: any Encoder) throws {
    var container = encoder.container(keyedBy: CodingKeys.self)
    try container.encode(AnnotationSubtype.ink.rawValue, forKey: .subtype)
    try container.encode(inkList.map { $0.map(WirePoint.init) }, forKey: .inkList)
    try container.encodeIfPresent(color, forKey: .color)
    try container.encodeIfPresent(opacity, forKey: .opacity)
    try container.encodeIfPresent(strokeWidth, forKey: .strokeWidth)
    try container.encodeIfPresent(borderStyle, forKey: .borderStyle)
    try container.encodeIfPresent(dashArray, forKey: .dashArray)
    try container.encodeIfPresent(intent, forKey: .intent)
    try container.encodeIfPresent(rotation, forKey: .rotation)
    try container.encodeIfPresent(contents, forKey: .contents)
  }
}

/// A change to an ink annotation, or to an ink tool's defaults. A member
/// left `nil` keeps its value; the rules are the draft's
/// (`InkAnnotationDraft`).
public struct InkAnnotationPatch: AnnotationPatch, Hashable {
  /// Replaces every stroke.
  public var inkList: [[Point]]?
  /// `.null` removes the stroke colour: no stroke.
  public var color: Nullable<AnnotationColor>?
  public var opacity: Double?
  public var strokeWidth: Double?
  public var borderStyle: AnnotationBorderStyle?
  /// `.null` removes the pattern.
  public var dashArray: Nullable<[Double]>?
  /// `.null` makes it pen ink.
  public var intent: Nullable<AnnotationInkIntent>?
  /// `.null` removes the rotation.
  public var rotation: Nullable<Double>?
  /// `.null` removes `/Contents`.
  public var contents: Nullable<String>?

  public init(
    inkList: [[Point]]? = nil, color: Nullable<AnnotationColor>? = nil, opacity: Double? = nil,
    strokeWidth: Double? = nil, borderStyle: AnnotationBorderStyle? = nil,
    dashArray: Nullable<[Double]>? = nil,
    intent: Nullable<AnnotationInkIntent>? = nil,
    rotation: Nullable<Double>? = nil, contents: Nullable<String>? = nil
  ) {
    self.inkList = inkList
    self.color = color
    self.opacity = opacity
    self.strokeWidth = strokeWidth
    self.borderStyle = borderStyle
    self.dashArray = dashArray
    self.intent = intent
    self.rotation = rotation
    self.contents = contents
  }

  private enum CodingKeys: String, CodingKey {
    case inkList, color, opacity, strokeWidth, borderStyle, dashArray, intent, rotation, contents
  }

  public func encode(to encoder: any Encoder) throws {
    var container = encoder.container(keyedBy: CodingKeys.self)
    try container.encodeIfPresent(inkList.map { $0.map { $0.map(WirePoint.init) } }, forKey: .inkList)
    try container.encodeIfPresent(color, forKey: .color)
    try container.encodeIfPresent(opacity, forKey: .opacity)
    try container.encodeIfPresent(strokeWidth, forKey: .strokeWidth)
    try container.encodeIfPresent(borderStyle, forKey: .borderStyle)
    try container.encodeIfPresent(dashArray, forKey: .dashArray)
    try container.encodeIfPresent(intent, forKey: .intent)
    try container.encodeIfPresent(rotation, forKey: .rotation)
    try container.encodeIfPresent(contents, forKey: .contents)
  }
}

// ── highlight ───────────────────────────────────────────────────────────────

/// A new highlight annotation over text quads.
public struct HighlightAnnotationDraft: AnnotationDraft, Hashable {
  /// `/QuadPoints`, page space: one quad per run of marked text, at least
  /// one.
  public var quadPoints: [Quad]
  /// `/C`. A highlight always has one, so there is no `.null`.
  public var color: AnnotationColor?
  /// `/CA`, 0 to 1.
  public var opacity: Double?
  /// `/Contents`. `.null` is none.
  public var contents: Nullable<String>?

  public init(
    quadPoints: [Quad], color: AnnotationColor? = nil, opacity: Double? = nil,
    contents: Nullable<String>? = nil
  ) {
    self.quadPoints = quadPoints
    self.color = color
    self.opacity = opacity
    self.contents = contents
  }

  private enum CodingKeys: String, CodingKey {
    case subtype, quadPoints, color, opacity, contents
  }

  public func encode(to encoder: any Encoder) throws {
    var container = encoder.container(keyedBy: CodingKeys.self)
    try container.encode(AnnotationSubtype.highlight.rawValue, forKey: .subtype)
    try container.encode(quadPoints.map(WireQuad.init), forKey: .quadPoints)
    try container.encodeIfPresent(color, forKey: .color)
    try container.encodeIfPresent(opacity, forKey: .opacity)
    try container.encodeIfPresent(contents, forKey: .contents)
  }
}

/// A change to a highlight annotation, or to a highlight tool's defaults.
/// A member left `nil` keeps its value.
public struct HighlightAnnotationPatch: AnnotationPatch, Hashable {
  /// Replaces every quad.
  public var quadPoints: [Quad]?
  public var color: AnnotationColor?
  public var opacity: Double?
  /// `.null` removes `/Contents`.
  public var contents: Nullable<String>?

  public init(
    quadPoints: [Quad]? = nil, color: AnnotationColor? = nil, opacity: Double? = nil,
    contents: Nullable<String>? = nil
  ) {
    self.quadPoints = quadPoints
    self.color = color
    self.opacity = opacity
    self.contents = contents
  }

  private enum CodingKeys: String, CodingKey {
    case quadPoints, color, opacity, contents
  }

  public func encode(to encoder: any Encoder) throws {
    var container = encoder.container(keyedBy: CodingKeys.self)
    try container.encodeIfPresent(quadPoints.map { $0.map(WireQuad.init) }, forKey: .quadPoints)
    try container.encodeIfPresent(color, forKey: .color)
    try container.encodeIfPresent(opacity, forKey: .opacity)
    try container.encodeIfPresent(contents, forKey: .contents)
  }
}

// ── note ────────────────────────────────────────────────────────────────────

/// A new note: a `text` annotation, a sticky note.
public struct NoteAnnotationDraft: AnnotationDraft, Hashable {
  /// `/Rect`, page space: where the icon sits.
  public var rect: Rect
  /// `/C`. A note always has one, so there is no `.null`.
  public var color: AnnotationColor?
  /// `/CA`, 0 to 1.
  public var opacity: Double?
  public var icon: AnnotationNoteIcon?
  /// `/Open`: whether the note shows open.
  public var open: Bool?
  /// `/State`. A standard state (`accepted`, `marked`, …) brings its own
  /// `stateModel`; any other needs one. `.null` is none.
  public var state: Nullable<String>?
  /// `/StateModel`: `review`, `marked`, or a custom model. `.null` is none.
  public var stateModel: Nullable<String>?
  /// `/Contents`: the note's text. `.null` is none.
  public var contents: Nullable<String>?

  public init(
    rect: Rect, color: AnnotationColor? = nil, opacity: Double? = nil, icon: AnnotationNoteIcon? = nil,
    open: Bool? = nil, state: Nullable<String>? = nil,
    stateModel: Nullable<String>? = nil, contents: Nullable<String>? = nil
  ) {
    self.rect = rect
    self.color = color
    self.opacity = opacity
    self.icon = icon
    self.open = open
    self.state = state
    self.stateModel = stateModel
    self.contents = contents
  }

  private enum CodingKeys: String, CodingKey {
    case subtype, rect, color, opacity, icon, open, state, stateModel, contents
  }

  public func encode(to encoder: any Encoder) throws {
    var container = encoder.container(keyedBy: CodingKeys.self)
    try container.encode(AnnotationSubtype.text.rawValue, forKey: .subtype)
    try container.encode(WireRect(rect), forKey: .rect)
    try container.encodeIfPresent(color, forKey: .color)
    try container.encodeIfPresent(opacity, forKey: .opacity)
    try container.encodeIfPresent(icon, forKey: .icon)
    try container.encodeIfPresent(open, forKey: .open)
    try container.encodeIfPresent(state, forKey: .state)
    try container.encodeIfPresent(stateModel, forKey: .stateModel)
    try container.encodeIfPresent(contents, forKey: .contents)
  }
}

/// A change to a note, or to the note tool's defaults. A member left `nil`
/// keeps its value.
public struct NoteAnnotationPatch: AnnotationPatch, Hashable {
  /// Moves or resizes the icon.
  public var rect: Rect?
  public var color: AnnotationColor?
  public var opacity: Double?
  public var icon: AnnotationNoteIcon?
  public var open: Bool?
  /// A standard state sets its own `stateModel`; any other keeps the
  /// note's, and the note needs one. `.null` removes it.
  public var state: Nullable<String>?
  /// `.null` removes it.
  public var stateModel: Nullable<String>?
  /// `.null` removes `/Contents`.
  public var contents: Nullable<String>?

  public init(
    rect: Rect? = nil, color: AnnotationColor? = nil, opacity: Double? = nil,
    icon: AnnotationNoteIcon? = nil, open: Bool? = nil, state: Nullable<String>? = nil,
    stateModel: Nullable<String>? = nil, contents: Nullable<String>? = nil
  ) {
    self.rect = rect
    self.color = color
    self.opacity = opacity
    self.icon = icon
    self.open = open
    self.state = state
    self.stateModel = stateModel
    self.contents = contents
  }

  private enum CodingKeys: String, CodingKey {
    case rect, color, opacity, icon, open, state, stateModel, contents
  }

  public func encode(to encoder: any Encoder) throws {
    var container = encoder.container(keyedBy: CodingKeys.self)
    try container.encodeIfPresent(rect.map(WireRect.init), forKey: .rect)
    try container.encodeIfPresent(color, forKey: .color)
    try container.encodeIfPresent(opacity, forKey: .opacity)
    try container.encodeIfPresent(icon, forKey: .icon)
    try container.encodeIfPresent(open, forKey: .open)
    try container.encodeIfPresent(state, forKey: .state)
    try container.encodeIfPresent(stateModel, forKey: .stateModel)
    try container.encodeIfPresent(contents, forKey: .contents)
  }
}

// ── encoding ────────────────────────────────────────────────────────────────

enum AnnotationFieldBag {
  /// Sorted keys make the bytes stable, and unescaped slashes keep a
  /// string as the caller wrote it.
  static func json(of value: some Encodable) throws(PDFError) -> String {
    let encoder = JSONEncoder()
    encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
    do {
      return String(decoding: try encoder.encode(value), as: UTF8.self)
    } catch {
      throw PDFError(
        name: "EncodingError", code: .invalidInput,
        detail: "the annotation fields don't encode as JSON: \(error)")
    }
  }
}

/// Page-space geometry as the plugin reads it: `{x, y}` points,
/// `upperLeft`…`lowerRight` quads (`NamedPdfQuad` in
/// `crates/epdf-engine/src/geom.rs`) and `{x, y, width, height}` boxes.
private struct WirePoint: Encodable {
  let x: Double
  let y: Double

  init(_ point: Point) {
    (x, y) = (point.x, point.y)
  }
}

private struct WireQuad: Encodable {
  let upperLeft: WirePoint
  let upperRight: WirePoint
  let lowerLeft: WirePoint
  let lowerRight: WirePoint

  init(_ quad: Quad) {
    upperLeft = WirePoint(quad.upperLeft)
    upperRight = WirePoint(quad.upperRight)
    lowerLeft = WirePoint(quad.lowerLeft)
    lowerRight = WirePoint(quad.lowerRight)
  }
}

private struct WireRect: Encodable {
  let x: Double
  let y: Double
  let width: Double
  let height: Double

  init(_ rect: Rect) {
    (x, y, width, height) = (rect.x, rect.y, rect.width, rect.height)
  }
}
