import Foundation
import Testing

@Suite(.serialized, .timeLimit(.minutes(1)))
struct BaseSuite {}

enum Fixture {
  static func data(_ name: String) throws -> Data {
    let url = try #require(Bundle.module.url(forResource: name, withExtension: "pdf", subdirectory: "Fixtures"))
    return try Data(contentsOf: url)
  }
}
