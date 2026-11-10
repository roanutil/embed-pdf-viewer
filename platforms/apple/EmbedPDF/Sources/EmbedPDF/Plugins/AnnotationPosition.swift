public import EmbedPDFBindings
import Foundation

/// Where `AnnotationCapability.reorder(_:to:)` puts annotations among their
/// page's annotations. A page's annotations paint in list order, later over
/// earlier, so `end` is on top and `start` is underneath. A position names a
/// neighbour rather than an index, so it keeps its meaning while other
/// annotations come and go.
public enum AnnotationPosition: Sendable, Hashable, Codable {
  /// The start of the page's list: under every other annotation.
  case start
  /// The end of the page's list: over every other annotation.
  case end
  /// Right before `annotation`, so painted just under it.
  case before(annotation: AnnotationRef)
  /// Right after `annotation`, so painted just over it.
  case after(annotation: AnnotationRef)

  /// The engine's `AnnotationPosition` JSON: `"start"`, `"end"`,
  /// `{"before":ref}` or `{"after":ref}`, where `ref` is the engine's
  /// annotation ref (`{"kind":"objectNumber","page":…,"objectNumber":…}`).
  public var json: String {
    switch self {
    case .start: #""start""#
    case .end: #""end""#
    case let .before(annotation): #"{"before":"# + Self.json(annotation) + "}"
    case let .after(annotation): #"{"after":"# + Self.json(annotation) + "}"
    }
  }

  private static func json(_ page: PageRef) -> String {
    #"{"kind":"objectNumber","objectNumber":"# + String(page.pageObjectNumber) + "}"
  }

  private static func json(_ annotation: AnnotationRef) -> String {
    switch annotation {
    case let .objectNumber(page, number):
      #"{"kind":"objectNumber","page":"# + json(page) + #","objectNumber":"# + String(number) + "}"
    case let .index(page, index):
      #"{"kind":"baseIndex","page":"# + json(page) + #","baseIndex":"# + String(index) + "}"
    }
  }

  private enum Neighbour: String, CodingKey { case before, after }
  private enum RefKey: String, CodingKey { case kind, page, objectNumber, baseIndex }

  public init(from decoder: any Decoder) throws {
    if let end = try? decoder.singleValueContainer().decode(String.self) {
      switch end {
      case "start": self = .start
      case "end": self = .end
      default:
        throw DecodingError.dataCorrupted(
          .init(codingPath: decoder.codingPath, debugDescription: "not a position: \(end)"))
      }
      return
    }
    let container = try decoder.container(keyedBy: Neighbour.self)
    if container.contains(.before) {
      self = .before(annotation: try Self.ref(container.nestedContainer(keyedBy: RefKey.self, forKey: .before)))
    } else {
      self = .after(annotation: try Self.ref(container.nestedContainer(keyedBy: RefKey.self, forKey: .after)))
    }
  }

  public func encode(to encoder: any Encoder) throws {
    switch self {
    case .start:
      var container = encoder.singleValueContainer()
      try container.encode("start")
    case .end:
      var container = encoder.singleValueContainer()
      try container.encode("end")
    case let .before(annotation):
      var container = encoder.container(keyedBy: Neighbour.self)
      try Self.encode(annotation, into: container.nestedContainer(keyedBy: RefKey.self, forKey: .before))
    case let .after(annotation):
      var container = encoder.container(keyedBy: Neighbour.self)
      try Self.encode(annotation, into: container.nestedContainer(keyedBy: RefKey.self, forKey: .after))
    }
  }

  private static func ref(_ container: KeyedDecodingContainer<RefKey>) throws -> AnnotationRef {
    let pageContainer = try container.nestedContainer(keyedBy: RefKey.self, forKey: .page)
    let page = PageRef(pageObjectNumber: try pageContainer.decode(UInt32.self, forKey: .objectNumber))
    switch try container.decode(String.self, forKey: .kind) {
    case "objectNumber":
      return .objectNumber(page: page, annotObjectNumber: try container.decode(UInt32.self, forKey: .objectNumber))
    case "baseIndex":
      return .index(page: page, index: try container.decode(UInt32.self, forKey: .baseIndex))
    case let kind:
      throw DecodingError.dataCorruptedError(
        forKey: .kind, in: container, debugDescription: "not an annotation ref kind: \(kind)")
    }
  }

  private static func encode(_ annotation: AnnotationRef, into container: KeyedEncodingContainer<RefKey>) throws {
    var container = container
    let page: PageRef
    switch annotation {
    case let .objectNumber(ref, number):
      page = ref
      try container.encode("objectNumber", forKey: .kind)
      try container.encode(number, forKey: .objectNumber)
    case let .index(ref, index):
      page = ref
      try container.encode("baseIndex", forKey: .kind)
      try container.encode(index, forKey: .baseIndex)
    }
    var pageContainer = container.nestedContainer(keyedBy: RefKey.self, forKey: .page)
    try pageContainer.encode("objectNumber", forKey: .kind)
    try pageContainer.encode(page.pageObjectNumber, forKey: .objectNumber)
  }
}
