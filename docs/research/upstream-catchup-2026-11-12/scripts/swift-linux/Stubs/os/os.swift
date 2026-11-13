// os signposts on Linux: intervals that record nothing.
public struct OSSignpostID: Sendable {}
public struct OSSignpostIntervalState: Sendable {}

public struct OSSignposter: Sendable {
  public enum Category: Sendable { case pointsOfInterest }
  public init(subsystem: String, category: Category) {}
  public func makeSignpostID() -> OSSignpostID { OSSignpostID() }
  public func beginInterval(_ name: StaticString, id: OSSignpostID) -> OSSignpostIntervalState { OSSignpostIntervalState() }
  public func endInterval(_ name: StaticString, _ state: OSSignpostIntervalState) {}
}
