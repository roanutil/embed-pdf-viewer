import Foundation
import Testing

@Suite(.serialized, .timeLimit(.minutes(1)))
struct BaseSuite {}

enum Fixture {
  static func data(_ name: String) throws -> Data {
    let url = try #require(Bundle.module.url(forResource: name, withExtension: "pdf", subdirectory: "Fixtures"))
    return try Data(contentsOf: url)
  }

  static func text(_ name: String, ext: String) throws -> String {
    let url = try #require(Bundle.module.url(forResource: name, withExtension: ext, subdirectory: "Fixtures"))
    return try String(contentsOf: url, encoding: .utf8)
  }
}
