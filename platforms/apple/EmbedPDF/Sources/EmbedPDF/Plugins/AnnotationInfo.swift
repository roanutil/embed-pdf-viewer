public import EmbedPDFBindings
public import Foundation

/// An annotation as the engine reads it, in Swift types: timestamps are
/// `Date`, the subtype is an `AnnotationSubtype`, and ink, highlight and
/// note annotations carry their own members typed in `details`.
/// `AnnotationCapability.get(_:)`, `list(_:)`, `selected()` and
/// `hitTest(at:on:)` return it.
///
/// Nothing the engine reported is lost. `fieldsJSON` is the engine's whole
/// record, every member, byte for byte; each timestamp keeps the engine's
/// text next to its `Date`; and `record` gives back the exact
/// `AnnotationRecord` this was made from, for the calls that take one.
public struct AnnotationInfo: Sendable, Hashable, Identifiable {
  public let annotationRef: AnnotationRef
  public let page: PageRef
  public let subtype: AnnotationSubtype
  /// The annotation's `/Rect`, page space.
  public let rect: Rect
  public let contents: String?
  /// `/T`.
  public let author: String?
  /// `/CreationDate` as an instant. `nil` when the annotation has none, or
  /// when `rawCreatedAt` isn't a date and time this reads; see
  /// `rawCreatedAt`.
  public let createdAt: Date?
  /// `/CreationDate` exactly as the engine reports it: ISO 8601 with whole
  /// seconds, and an offset of `Z`, `±HH:mm` or none. A PDF date without an
  /// offset reads without one, and `createdAt` takes it as UTC, as the
  /// annotation plugin orders comment replies.
  public let rawCreatedAt: String?
  /// `/M` as an instant, on the same terms as `createdAt`.
  public let modifiedAt: Date?
  /// `/M` exactly as the engine reports it.
  public let rawModifiedAt: String?
  public let groupID: String?
  /// `reply.to`: the annotation this one answers.
  public let inReplyTo: AnnotationRef?
  public let canUpdate: Bool
  public let canDelete: Bool
  /// The engine's record, every member, as the engine wrote it. The members
  /// `details` doesn't type are read from here.
  public let fieldsJSON: String
  /// The subtype's own members, typed for ink, highlight and note
  /// annotations.
  public let details: Details

  public init(_ record: AnnotationRecord) {
    annotationRef = record.annotationRef
    page = record.page
    subtype = AnnotationSubtype(rawValue: record.subtype)
    rect = record.rect
    contents = record.contents
    author = record.author
    rawCreatedAt = record.createdAt
    createdAt = record.createdAt.flatMap(Self.instant(of:))
    rawModifiedAt = record.modifiedAt
    modifiedAt = record.modifiedAt.flatMap(Self.instant(of:))
    groupID = record.groupId
    inReplyTo = record.inReplyTo
    canUpdate = record.canUpdate
    canDelete = record.canDelete
    fieldsJSON = record.fieldsJson
    details = Details(subtype: subtype, fieldsJSON: record.fieldsJson)
  }

  /// The engine's record this was made from, member for member.
  public var record: AnnotationRecord {
    AnnotationRecord(
      annotationRef: annotationRef, page: page, subtype: subtype.rawValue, rect: rect,
      contents: contents, author: author, createdAt: rawCreatedAt, modifiedAt: rawModifiedAt,
      groupId: groupID, inReplyTo: inReplyTo, canUpdate: canUpdate, canDelete: canDelete,
      fieldsJson: fieldsJSON)
  }

  /// `annotationRef`, so `ForEach(annotations)` needs no `id:`.
  public var id: AnnotationRef { annotationRef }

  @available(*, deprecated, renamed: "fieldsJSON")
  public var fieldsJson: String { fieldsJSON }

  @available(*, deprecated, renamed: "groupID")
  public var groupId: String? { groupID }

  /// `details`' ink members, or `nil` for any other subtype.
  public var ink: Ink? {
    if case let .ink(ink) = details { ink } else { nil }
  }

  /// `details`' highlight members, or `nil` for any other subtype.
  public var highlight: Highlight? {
    if case let .highlight(highlight) = details { highlight } else { nil }
  }

  /// `details`' note members, or `nil` for any other subtype.
  public var note: Note? {
    if case let .note(note) = details { note } else { nil }
  }
}

// ── subtype members ─────────────────────────────────────────────────────────

extension AnnotationInfo {
  /// The members a subtype adds to the common ones.
  public enum Details: Sendable, Hashable {
    case ink(Ink)
    case highlight(Highlight)
    /// A `text` annotation: a sticky note.
    case note(Note)
    /// A subtype this layer doesn't type, or a record whose members don't
    /// have the shape it reads. Read `fieldsJSON`.
    case untyped

    init(subtype: AnnotationSubtype, fieldsJSON: String) {
      let bytes = Data(fieldsJSON.utf8)
      let decoder = JSONDecoder()
      switch subtype {
      case .ink:
        self = (try? decoder.decode(Ink.self, from: bytes)).map(Details.ink) ?? .untyped
      case .highlight:
        self = (try? decoder.decode(Highlight.self, from: bytes)).map(Details.highlight) ?? .untyped
      case .text:
        self = (try? decoder.decode(Note.self, from: bytes)).map(Details.note) ?? .untyped
      default:
        self = .untyped
      }
    }
  }

  /// An ink annotation's members: freehand strokes.
  public struct Ink: Sendable, Hashable {
    /// `/C`, or `nil` when the annotation has none or an empty one: no
    /// stroke colour.
    public let color: AnnotationColor?
    /// `/CA`, 0 to 1.
    public let opacity: Double
    public let strokeWidth: Double
    /// `solid`, `dashed`, `beveled` or `inset`.
    public let borderStyle: String
    /// The dash pattern, or `nil` when the border isn't dashed or names none.
    public let dashArray: [Double]?
    /// `/InkList`: one array of page-space points per stroke.
    public let inkList: [[Point]]
    /// `/IT`. `nil` for ordinary pen ink; `ink-highlight` for a freehand
    /// highlighter.
    public let intent: String?
    public let rotation: Double?
  }

  /// A highlight annotation's members.
  public struct Highlight: Sendable, Hashable {
    /// `/C`.
    public let color: AnnotationColor
    /// `/CA`, 0 to 1.
    public let opacity: Double
    /// `/QuadPoints`, page space: one quad per run of marked text.
    public let quadPoints: [Quad]
  }

  /// A note's members: a `text` annotation.
  public struct Note: Sendable, Hashable {
    /// `/C`.
    public let color: AnnotationColor
    /// `/CA`, 0 to 1.
    public let opacity: Double
    /// `/Name`: `comment`, `key`, `note`, `help`, `new-paragraph`,
    /// `paragraph` or `insert`.
    public let icon: String
    /// `/Open`: whether the note shows open.
    public let open: Bool
    /// `/State`, lowercased when it is one of the standard names and
    /// verbatim when it is not.
    public let state: String?
    /// `/StateModel`, on the same terms.
    public let stateModel: String?
  }
}

// The typed members decode from the record's JSON; a missing required
// member fails the decode, and `Details` falls back to `untyped`.

private struct WirePoint: Decodable {
  let x: Double
  let y: Double

  var point: Point { Point(x: x, y: y) }
}

private struct WireQuad: Decodable {
  let upperLeft: WirePoint
  let upperRight: WirePoint
  let lowerLeft: WirePoint
  let lowerRight: WirePoint

  var quad: Quad {
    Quad(
      upperLeft: upperLeft.point, upperRight: upperRight.point, lowerLeft: lowerLeft.point,
      lowerRight: lowerRight.point)
  }
}

extension AnnotationInfo.Ink: Decodable {
  private enum CodingKeys: String, CodingKey {
    case color, opacity, strokeWidth, borderStyle, dashArray, inkList, intent, rotation
  }

  public init(from decoder: any Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    color = try container.decodeIfPresent(AnnotationColor.self, forKey: .color)
    opacity = try container.decode(Double.self, forKey: .opacity)
    strokeWidth = try container.decode(Double.self, forKey: .strokeWidth)
    borderStyle = try container.decode(String.self, forKey: .borderStyle)
    dashArray = try container.decodeIfPresent([Double].self, forKey: .dashArray)
    inkList = try container.decode([[WirePoint]].self, forKey: .inkList).map { $0.map(\.point) }
    intent = try container.decodeIfPresent(String.self, forKey: .intent)
    rotation = try container.decodeIfPresent(Double.self, forKey: .rotation)
  }
}

extension AnnotationInfo.Highlight: Decodable {
  private enum CodingKeys: String, CodingKey {
    case color, opacity, quadPoints
  }

  public init(from decoder: any Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    color = try container.decode(AnnotationColor.self, forKey: .color)
    opacity = try container.decode(Double.self, forKey: .opacity)
    quadPoints = try container.decode([WireQuad].self, forKey: .quadPoints).map(\.quad)
  }
}

extension AnnotationInfo.Note: Decodable {
  private enum CodingKeys: String, CodingKey {
    case color, opacity, icon, open, state, stateModel
  }

  public init(from decoder: any Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    color = try container.decode(AnnotationColor.self, forKey: .color)
    opacity = try container.decode(Double.self, forKey: .opacity)
    icon = try container.decode(String.self, forKey: .icon)
    open = try container.decode(Bool.self, forKey: .open)
    state = try container.decodeIfPresent(String.self, forKey: .state)
    stateModel = try container.decodeIfPresent(String.self, forKey: .stateModel)
  }
}

// ── timestamps ──────────────────────────────────────────────────────────────

extension AnnotationInfo {
  /// The instant an engine timestamp names, or `nil` when the text isn't one.
  ///
  /// The engine writes `YYYY-MM-DDTHH:mm:ss` and then `Z`, `±HH:mm` or
  /// nothing (`pdf_date_to_iso` in `crates/epdf-engine/src/clock.rs`). This
  /// also reads what the engine takes as a date and time on the way in
  /// (`is_iso_date_time` there): seconds may be left out, a fraction of a
  /// second may follow them, and an offset may drop its colon. Without an
  /// offset the time is UTC, as the annotation plugin's `date_parse`
  /// (`crates/plugins/annotation/src/engine.rs`) reads it. The day must
  /// exist, and the hour, minute, second and offset must be in range.
  static func instant(of text: String) -> Date? {
    let bytes = Array(text.utf8)
    var index = 0

    func number(_ length: Int) -> Int? {
      guard index + length <= bytes.count else { return nil }
      var value = 0
      for byte in bytes[index..<index + length] {
        guard (UInt8(ascii: "0")...UInt8(ascii: "9")).contains(byte) else { return nil }
        value = value * 10 + Int(byte - UInt8(ascii: "0"))
      }
      index += length
      return value
    }
    func skip(_ character: Unicode.Scalar) -> Bool {
      guard index < bytes.count, bytes[index] == UInt8(ascii: character) else { return false }
      index += 1
      return true
    }

    guard let year = number(4), skip("-"), let month = number(2), skip("-"),
      let day = number(2), skip("T"), let hour = number(2), skip(":"), let minute = number(2)
    else { return nil }
    var second = 0
    var fraction = 0.0
    if skip(":") {
      guard let whole = number(2) else { return nil }
      second = whole
      if skip(".") {
        let start = index
        while index < bytes.count, (UInt8(ascii: "0")...UInt8(ascii: "9")).contains(bytes[index]) {
          index += 1
        }
        guard index > start,
          let value = Double("0." + String(decoding: bytes[start..<index], as: UTF8.self))
        else { return nil }
        fraction = value
      }
    }
    var offsetMinutes = 0
    if skip("Z") {
      // UTC.
    } else if index < bytes.count, bytes[index] == UInt8(ascii: "+") || bytes[index] == UInt8(ascii: "-") {
      let sign = bytes[index] == UInt8(ascii: "-") ? -1 : 1
      index += 1
      guard let hours = number(2) else { return nil }
      _ = skip(":")
      guard let minutes = number(2), hours <= 23, minutes <= 59 else { return nil }
      offsetMinutes = sign * (hours * 60 + minutes)
    }
    guard index == bytes.count, (1...12).contains(month),
      (1...daysIn(month: month, year: year)).contains(day), hour <= 23, minute <= 59, second <= 59
    else { return nil }

    let days = daysFromCivil(year: year, month: month, day: day)
    let seconds = ((days * 24 + hour) * 60 + minute - offsetMinutes) * 60 + second
    return Date(timeIntervalSince1970: Double(seconds) + fraction)
  }

  private static func daysIn(month: Int, year: Int) -> Int {
    switch month {
    case 2: year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) ? 29 : 28
    case 4, 6, 9, 11: 30
    default: 31
    }
  }

  /// Days from 1970-01-01 to a proleptic Gregorian date, the algorithm
  /// `days_from_civil` in `crates/epdf-engine/src/clock.rs` uses.
  private static func daysFromCivil(year: Int, month: Int, day: Int) -> Int {
    let shiftedYear = month <= 2 ? year - 1 : year
    let era = (shiftedYear >= 0 ? shiftedYear : shiftedYear - 399) / 400
    let yearOfEra = shiftedYear - era * 400
    let dayOfYear = (153 * (month > 2 ? month - 3 : month + 9) + 2) / 5 + day - 1
    let dayOfEra = yearOfEra * 365 + yearOfEra / 4 - yearOfEra / 100 + dayOfYear
    return era * 146_097 + dayOfEra - 719_468
  }
}

extension AnnotationChangedEvent {
  /// The created or updated annotation, typed.
  public var annotationInfo: AnnotationInfo { AnnotationInfo(annotation) }
}

extension AnnotationImportResult {
  /// The copied annotations, typed, in `annotations`' order.
  public var annotationInfos: [AnnotationInfo] { annotations.map(AnnotationInfo.init) }
}
